import Foundation
import WatchConnectivity

/// Manages the WatchConnectivity session between the watch and paired iPhone.
///
/// Responsibilities:
/// - Receives `applicationContext` updates (today tasks, habits, timer/focus state)
///   from the iPhone and updates the shared `ComplicationSnapshot`.
/// - Sends mutation intents to the iPhone via `sendMessage` (foreground) or
///   `transferUserInfo` (background).
/// - Maintains a persistent `IntentQueue` for offline mutations.
/// - Triggers `WidgetCenter.shared.reloadAllTimelines()` when data changes.
///
/// The watch keeps a **read replica** of the actionable surface. All mutations
/// flow through the phone, which owns the vault and event log.
final class WatchSessionManager: NSObject, WCSessionDelegate, @unchecked Sendable {

    static let shared = WatchSessionManager()

    private let session: WCSession
    private let intentQueue = IntentQueue.shared
    private let snapshotStore = ComplicationSnapshotStore.shared

    /// Callback invoked on the main actor when new data arrives.
    @MainActor var onDataUpdate: (() -> Void)?

    override init() {
        self.session = WCSession.default
        super.init()
    }

    /// Activate the WatchConnectivity session. Call once at app launch.
    func activate() {
        guard WCSession.isSupported() else { return }
        session.delegate = self
        session.activate()
    }

    /// Whether the paired iPhone is currently reachable.
    var isReachable: Bool {
        session.isReachable
    }

    // MARK: - Sending intents

    /// Send a mutation intent to the iPhone.
    ///
    /// If the phone is reachable, sends immediately via `sendMessage`.
    /// Otherwise, queues via `transferUserInfo` for background delivery
    /// and persists in the `IntentQueue` for retry.
    func sendIntent(_ intent: IntentQueue.Intent) {
        let message: [String: Any] = [
            "type": "intent",
            "intentId": intent.id,
            "intentType": intent.type.rawValue,
            "payload": intent.payload,
        ]

        if session.isReachable {
            session.sendMessage(message, replyHandler: { reply in
                // Ack received — remove from queue
                if let acked = reply["ack"] as? String, acked == intent.id {
                    self.intentQueue.remove(id: intent.id)
                }
            }, errorHandler: { _ in
                // Delivery failed — keep in queue for retry
                self.intentQueue.enqueue(intent)
            })
        } else {
            // Queue for background delivery
            intentQueue.enqueue(intent)
            session.transferUserInfo(message)
        }
    }

    /// Replay all pending intents. Called when connectivity resumes.
    func replayPendingIntents() {
        let pending = intentQueue.pending
        for intent in pending {
            sendIntent(intent)
        }
    }

    // MARK: - WCSessionDelegate

    func session(
        _ session: WCSession,
        activationDidCompleteWith activationState: WCSessionActivationState,
        error: Error?
    ) {
        if activationState == .activated {
            replayPendingIntents()
        }
    }

    /// Receives application context updates from the iPhone.
    ///
    /// The iPhone sends a snapshot of the actionable surface whenever
    /// mutations occur:
    /// - `todayTasks`: JSON-encoded array of today's tasks
    /// - `habits`: JSON-encoded array of habits
    /// - `activeTimer`: JSON-encoded timer (or nil)
    /// - `activeFocus`: JSON-encoded focus session (or nil)
    /// - `vaultStatus`: "locked" or "unlocked"
    func session(
        _ session: WCSession,
        didReceiveApplicationContext applicationContext: [String: Any]
    ) {
        processIncomingContext(applicationContext)
    }

    /// Receives foreground messages from the iPhone.
    func session(
        _ session: WCSession,
        didReceiveMessage message: [String: Any]
    ) {
        // Handle acks for previously sent intents
        if let ack = message["ack"] as? String {
            intentQueue.remove(id: ack)
        }

        // Handle data updates
        if message["type"] as? String == "snapshot" {
            processIncomingContext(message)
        }
    }

    /// Receives foreground messages with reply handler.
    func session(
        _ session: WCSession,
        didReceiveMessage message: [String: Any],
        replyHandler: @escaping ([String: Any]) -> Void
    ) {
        self.session(session, didReceiveMessage: message)
        replyHandler(["received": true])
    }

    // MARK: - Context processing

    /// Decode and store an incoming snapshot from the iPhone.
    private func processIncomingContext(_ context: [String: Any]) {
        // Update the snapshot store for complications
        snapshotStore.update(from: context)

        // Reload complication timelines
        reloadComplications()

        // Notify the app UI
        Task { @MainActor in
            onDataUpdate?()
        }
    }

    private func reloadComplications() {
        // WidgetKit complication reload is triggered by the snapshot store
        // when it detects data changes. This is a no-op placeholder until
        // the WidgetKit extension is wired up in an Xcode project.
        //
        // In production:
        // WidgetCenter.shared.reloadAllTimelines()
    }
}
