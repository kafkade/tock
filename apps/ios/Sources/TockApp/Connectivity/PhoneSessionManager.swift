import Foundation

#if canImport(WatchConnectivity)
import WatchConnectivity

/// iPhone side of the watch bridge.
///
/// Owns the WatchConnectivity session on the phone and:
/// - **publishes** a ``WatchSnapshotDTO`` (today tasks, habits, active
///   timer/focus, vault status) built from the live `CoreClient` whenever the
///   app unlocks, mutates data, or backgrounds, and
/// - **applies** mutation intents forwarded from the watch to the same live
///   `CoreClient`, then re-publishes so the watch reflects the result.
///
/// The phone owns the vault, so the manager only acts while the app holds an
/// unlocked client (set via ``setClient(_:)``). When locked it publishes a
/// locked snapshot and leaves watch intents un-acked so the watch retries on
/// reconnect.
final class PhoneSessionManager: NSObject, WCSessionDelegate, @unchecked Sendable {

    static let shared = PhoneSessionManager()

    private let session: WCSession = .default
    private let lock = NSLock()
    private var liveClient: (any CoreClient)?

    private override init() {
        super.init()
    }

    // MARK: - Lifecycle

    /// Activate the session. Call once at app launch.
    func activate() {
        guard WCSession.isSupported() else { return }
        session.delegate = self
        session.activate()
    }

    /// Provide the unlocked client (call on unlock) or clear it (call on lock).
    func setClient(_ client: (any CoreClient)?) {
        lock.withLock { liveClient = client }
    }

    private var client: (any CoreClient)? {
        lock.withLock { liveClient }
    }

    // MARK: - Publishing snapshots

    /// Build and push a snapshot from the currently held client. Publishes a
    /// locked snapshot when no client is available.
    func pushSnapshot() {
        guard WCSession.isSupported() else { return }
        let client = self.client
        Task {
            let dto: WatchSnapshotDTO
            if let client {
                dto = (try? await Self.buildSnapshot(from: client)) ?? .locked
            } else {
                dto = .locked
            }
            self.send(dto)
        }
    }

    private func send(_ dto: WatchSnapshotDTO) {
        guard let data = try? WatchSync.encoder.encode(dto) else { return }
        let payload: [String: Any] = [WatchSync.snapshotKey: data]

        // Latest-wins background state.
        try? session.updateApplicationContext(payload)

        // Immediate delivery when the watch app is in the foreground.
        if session.isReachable {
            var message = payload
            message[WatchSync.messageTypeKey] = WatchSync.snapshotType
            session.sendMessage(message, replyHandler: nil, errorHandler: nil)
        }
    }

    static func buildSnapshot(from client: any CoreClient) async throws -> WatchSnapshotDTO {
        let today = try await client.listTasks(filter: .today)
        let habits = try await client.listHabits()
        let timer = try await client.currentTimer()
        let focus = try await client.focusStatus()

        let tomorrowStart = Calendar.current.startOfDay(for: Date()).addingTimeInterval(86_400)
        let dueCount = today.filter { ($0.deadline.map { $0 <= tomorrowStart }) ?? false }.count

        return WatchSnapshotDTO(
            vaultLocked: false,
            generatedAt: Date(),
            dueCount: dueCount,
            todayTasks: today.map { task in
                .init(id: task.id, sid: task.sid, title: task.title,
                      status: task.status.rawValue, deadline: task.deadline,
                      priority: task.priority?.rawValue, evening: task.evening,
                      urgency: task.urgency)
            },
            habits: habits.map { habit in
                .init(id: habit.id, sid: habit.sid, title: habit.title,
                      identity: habit.identity, direction: habit.direction.rawValue,
                      level: habit.level, streakCurrent: habit.streakCurrent,
                      streakBest: habit.streakBest, levelName: habit.levelName)
            },
            activeTimer: timer.map { block in
                .init(id: block.id, sid: block.sid, title: block.title,
                      startedAt: block.startedAt, taskId: block.taskId)
            },
            activeFocus: focus.map { session in
                .init(id: session.id, sid: session.sid, startedAt: session.startedAt,
                      taskId: session.taskId, plannedCycles: session.plannedCycles,
                      completedCycles: session.completedCycles, state: session.state.rawValue,
                      workMinutes: session.workMinutes,
                      shortBreakMinutes: session.shortBreakMinutes,
                      longBreakMinutes: session.longBreakMinutes)
            }
        )
    }

    // MARK: - Applying watch intents

    private func handleIntent(_ message: [String: Any], reply: (([String: Any]) -> Void)?) {
        guard
            message[WatchSync.messageTypeKey] as? String == WatchSync.intentType,
            let intentId = message[WatchSync.intentIdKey] as? String,
            let intentType = message[WatchSync.intentTypeKey] as? String
        else {
            reply?([:])
            return
        }
        let payload = (message[WatchSync.payloadKey] as? [String: String]) ?? [:]

        guard let client = self.client else {
            // Locked — don't ack so the watch keeps the intent queued.
            reply?([:])
            return
        }

        Task {
            do {
                try await Self.apply(intentType, payload: payload, to: client)
                reply?([WatchSync.ackKey: intentId])
                await WidgetSnapshotWriter.publish(from: client)
                self.pushSnapshot()
            } catch {
                reply?([:])
            }
        }
    }

    private static func apply(
        _ intentType: String, payload: [String: String], to client: any CoreClient
    ) async throws {
        switch intentType {
        case "completeTask":
            if let id = payload["taskId"] { try await client.completeTask(id: id) }
        case "startTimer":
            _ = try await client.startTimer(title: payload["title"] ?? "Untitled",
                                            taskId: payload["taskId"])
        case "stopTimer":
            _ = try await client.stopTimer()
        case "startFocus":
            let cycles = payload["cycles"].flatMap(UInt32.init) ?? 1
            _ = try await client.startFocus(taskId: payload["taskId"], cycles: cycles)
        case "completeFocusCycle":
            _ = try await client.completeFocusCycle()
        case "skipBreak":
            _ = try await client.skipBreak()
        case "pauseFocus":
            _ = try await client.pauseFocus()
        case "resumeFocus":
            _ = try await client.resumeFocus()
        case "abortFocus":
            _ = try await client.abortFocus()
        case "logHabit":
            if let id = payload["habitId"] {
                _ = try await client.logHabit(id: id, notes: payload["notes"])
            }
        default:
            break
        }
    }

    // MARK: - WCSessionDelegate

    func session(
        _ session: WCSession,
        activationDidCompleteWith activationState: WCSessionActivationState,
        error: Error?
    ) {
        if activationState == .activated {
            pushSnapshot()
        }
    }

    func session(_ session: WCSession, didReceiveMessage message: [String: Any]) {
        handleIntent(message, reply: nil)
    }

    func session(
        _ session: WCSession,
        didReceiveMessage message: [String: Any],
        replyHandler: @escaping ([String: Any]) -> Void
    ) {
        handleIntent(message, reply: replyHandler)
    }

    func session(_ session: WCSession, didReceiveUserInfo userInfo: [String: Any]) {
        handleIntent(userInfo, reply: nil)
    }

    func sessionDidBecomeInactive(_ session: WCSession) {}

    func sessionDidDeactivate(_ session: WCSession) {
        // Re-activate for a newly paired watch.
        session.activate()
    }

    func sessionReachabilityDidChange(_ session: WCSession) {
        if session.isReachable { pushSnapshot() }
    }
}
#else
final class PhoneSessionManager: @unchecked Sendable {

    static let shared = PhoneSessionManager()

    private init() {}

    func activate() {}

    func setClient(_ client: (any CoreClient)?) {}

    func pushSnapshot() {}
}
#endif
