import Foundation

/// Live `WatchCoreClient` backed by the read-replica snapshot from the iPhone.
///
/// Reads (today tasks, habits, timer, focus) come from ``LiveSnapshotStore``,
/// which the phone keeps up to date over WatchConnectivity. Mutations are
/// **forwarded** to the phone as intents via ``WatchSessionManager`` — the
/// phone owns the vault and event log. Methods that must return a value reply
/// with an optimistic projection derived from the current snapshot; the
/// authoritative state arrives in the next snapshot push.
struct LiveWatchCoreClient: WatchCoreClient {

    private var snapshot: WatchSnapshotDTO? { LiveSnapshotStore.shared.current }

    private func send(_ type: IntentQueue.IntentType, _ payload: [String: String] = [:]) {
        WatchSessionManager.shared.sendIntent(IntentQueue.Intent(type: type, payload: payload))
    }

    // MARK: Tasks

    func listTodayTasks() async throws -> [TaskItem] {
        (snapshot?.todayTasks ?? []).map { dto in
            TaskItem(
                id: dto.id, sid: dto.sid, title: dto.title,
                status: TaskStatus(rawValue: dto.status) ?? .pending,
                deadline: dto.deadline,
                priority: dto.priority.flatMap(Priority.init(rawValue:)),
                evening: dto.evening, urgency: dto.urgency
            )
        }
    }

    func completeTask(id: String) async throws {
        send(.completeTask, ["taskId": id])
    }

    // MARK: Time tracking

    func currentTimer() async throws -> TimeBlockItem? {
        snapshot?.activeTimer.map { dto in
            TimeBlockItem(id: dto.id, sid: dto.sid, title: dto.title,
                          startedAt: dto.startedAt, endedAt: nil, taskId: dto.taskId)
        }
    }

    func startTimer(title: String, taskId: String?) async throws -> TimeBlockItem {
        var payload = ["title": title]
        if let taskId { payload["taskId"] = taskId }
        send(.startTimer, payload)
        return TimeBlockItem(id: UUID().uuidString, sid: 0, title: title,
                             startedAt: Date(), endedAt: nil, taskId: taskId)
    }

    func stopTimer() async throws -> TimeBlockItem? {
        send(.stopTimer)
        guard let dto = snapshot?.activeTimer else { return nil }
        return TimeBlockItem(id: dto.id, sid: dto.sid, title: dto.title,
                             startedAt: dto.startedAt, endedAt: Date(), taskId: dto.taskId)
    }

    // MARK: Focus

    func focusStatus() async throws -> FocusSessionItem? {
        snapshot?.activeFocus.map(Self.focusItem(from:))
    }

    func startFocus(taskId: String?, cycles: UInt32) async throws -> FocusSessionItem {
        var payload = ["cycles": String(cycles)]
        if let taskId { payload["taskId"] = taskId }
        send(.startFocus, payload)
        return FocusSessionItem(
            id: UUID().uuidString, sid: 0, startedAt: Date(), endedAt: nil,
            taskId: taskId, plannedCycles: cycles, completedCycles: 0,
            state: .working, workMinutes: 25, shortBreakMinutes: 5, longBreakMinutes: 15
        )
    }

    func completeFocusCycle() async throws -> FocusSessionItem {
        send(.completeFocusCycle)
        return currentFocusOrFallback()
    }

    func skipBreak() async throws -> FocusSessionItem {
        send(.skipBreak)
        return currentFocusOrFallback()
    }

    func pauseFocus() async throws -> FocusSessionItem {
        send(.pauseFocus)
        return currentFocusOrFallback(state: .paused)
    }

    func resumeFocus() async throws -> FocusSessionItem {
        send(.resumeFocus)
        return currentFocusOrFallback(state: .working)
    }

    func abortFocus() async throws -> FocusSessionItem {
        send(.abortFocus)
        return currentFocusOrFallback(state: .aborted)
    }

    // MARK: Habits

    func listHabits() async throws -> [HabitItem] {
        (snapshot?.habits ?? []).map { dto in
            HabitItem(
                id: dto.id, sid: dto.sid, title: dto.title, identity: dto.identity,
                direction: HabitDirection(rawValue: dto.direction) ?? .build,
                level: dto.level, streakCurrent: dto.streakCurrent,
                streakBest: dto.streakBest, levelName: dto.levelName
            )
        }
    }

    func logHabit(id: String, notes: String?) async throws -> HabitEntryItem {
        var payload = ["habitId": id]
        if let notes { payload["notes"] = notes }
        send(.logHabit, payload)
        return HabitEntryItem(id: UUID().uuidString, habitId: id,
                              occurredAt: Date(), notes: notes, slip: false)
    }

    // MARK: - Helpers

    private static func focusItem(from dto: WatchSnapshotDTO.FocusDTO) -> FocusSessionItem {
        FocusSessionItem(
            id: dto.id, sid: dto.sid, startedAt: dto.startedAt, endedAt: nil,
            taskId: dto.taskId, plannedCycles: dto.plannedCycles,
            completedCycles: dto.completedCycles,
            state: FocusState(rawValue: dto.state) ?? .working,
            workMinutes: dto.workMinutes, shortBreakMinutes: dto.shortBreakMinutes,
            longBreakMinutes: dto.longBreakMinutes
        )
    }

    private func currentFocusOrFallback(state: FocusState? = nil) -> FocusSessionItem {
        if let dto = snapshot?.activeFocus {
            var item = Self.focusItem(from: dto)
            if let state { item.state = state }
            return item
        }
        return FocusSessionItem(
            id: UUID().uuidString, sid: 0, startedAt: Date(), endedAt: nil,
            taskId: nil, plannedCycles: 0, completedCycles: 0,
            state: state ?? .working, workMinutes: 25,
            shortBreakMinutes: 5, longBreakMinutes: 15
        )
    }
}
