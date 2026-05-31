import Foundation

/// Persistent queue of intents waiting to sync to the paired iPhone.
///
/// When the watch performs a mutation (complete task, log habit, start timer)
/// while the iPhone is unreachable, the intent is stored here and replayed
/// when connectivity resumes.
///
/// Persisted to UserDefaults so intents survive app termination and
/// watchOS background kills.
///
/// Each intent has a stable UUID for idempotency — the iPhone can
/// deduplicate replayed intents using this ID.
final class IntentQueue: @unchecked Sendable {

    /// A queued mutation intent.
    struct Intent: Codable, Identifiable, Sendable {
        let id: String
        let type: IntentType
        let payload: [String: String]
        let createdAt: Date

        init(type: IntentType, payload: [String: String] = [:]) {
            self.id = UUID().uuidString
            self.type = type
            self.payload = payload
            self.createdAt = Date()
        }
    }

    enum IntentType: String, Codable, Sendable {
        case completeTask
        case startTimer
        case stopTimer
        case startFocus
        case completeFocusCycle
        case skipBreak
        case pauseFocus
        case resumeFocus
        case abortFocus
        case logHabit
    }

    static let shared = IntentQueue()

    private let defaults = UserDefaults.standard
    private let storageKey = "com.kafkade.tock.watch.intentQueue"
    private let decoder = JSONDecoder()
    private let encoder = JSONEncoder()
    private let lock = NSLock()

    /// All pending intents, ordered by creation time.
    var pending: [Intent] {
        lock.withLock {
            loadFromDefaults()
        }
    }

    /// Number of pending intents.
    var count: Int { pending.count }

    /// Enqueue a new intent for later sync.
    func enqueue(_ intent: Intent) {
        lock.withLock {
            var queue = loadFromDefaults()
            queue.append(intent)
            saveToDefaults(queue)
        }
    }

    /// Remove an intent after successful acknowledgement from iPhone.
    func remove(id: String) {
        lock.withLock {
            var queue = loadFromDefaults()
            queue.removeAll { $0.id == id }
            saveToDefaults(queue)
        }
    }

    /// Remove all intents (e.g., after a full reconciliation).
    func removeAll() {
        lock.withLock {
            saveToDefaults([])
        }
    }

    // MARK: - Persistence

    private func loadFromDefaults() -> [Intent] {
        guard let data = defaults.data(forKey: storageKey) else { return [] }
        return (try? decoder.decode([Intent].self, from: data)) ?? []
    }

    private func saveToDefaults(_ queue: [Intent]) {
        if let data = try? encoder.encode(queue) {
            defaults.set(data, forKey: storageKey)
        }
    }
}
