import Foundation

// MARK: - Pending Capture

/// A task captured via the share extension, waiting to be synced to the vault.
///
/// Serialized as JSON in UserDefaults. The main app drains pending captures
/// on launch and creates real tasks. In production, both the extension and
/// the app use `UserDefaults(suiteName: appGroupId)` for cross-process access.
struct PendingCapture: Codable, Identifiable, Sendable {
    var id: UUID = UUID()
    var title: String
    var notes: String?
    var urlString: String?
    var projectId: String?
    var tags: [String]
    var priority: String?
    var destination: String
    var sourceKind: String
    var capturedAt: Date

    /// Convert to `NewTaskInput` when draining the pending queue.
    func toNewTaskInput() -> NewTaskInput {
        NewTaskInput(
            title: title,
            notes: notes,
            projectId: projectId,
            deadline: nil,
            priority: priority.flatMap { Priority(rawValue: $0) },
            evening: destination == QuickCaptureDestination.evening.rawValue,
            tags: tags
        )
    }
}

// MARK: - Share Pending Store

/// Persistent queue for share extension captures.
///
/// Stores pending captures as a JSON array in UserDefaults.
/// The main app drains this queue on launch to create tasks.
///
/// - Development: uses `UserDefaults.standard`
/// - Production: uses `UserDefaults(suiteName: "group.com.tock.app")`
final class SharePendingStore: Sendable {
    private static let storageKey = "pendingShareCaptures"

    /// In production, this would be `UserDefaults(suiteName: "group.com.tock.app")`
    private let defaults: UserDefaults

    static let shared = SharePendingStore()

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    /// Save a capture to the pending queue.
    func save(_ capture: PendingCapture) {
        var pending = loadAll()
        pending.append(capture)
        persist(pending)
    }

    /// Create a pending capture from share content and user choices.
    func saveCapture(
        content: ShareContent,
        destination: QuickCaptureDestination,
        projectId: String?,
        tags: [String],
        priority: Priority?,
        editedTitle: String,
        editedNotes: String?
    ) {
        let capture = PendingCapture(
            title: editedTitle,
            notes: editedNotes,
            urlString: content.url?.absoluteString,
            projectId: projectId,
            tags: tags,
            priority: priority?.rawValue,
            destination: destination.rawValue,
            sourceKind: content.kind.rawValue,
            capturedAt: Date()
        )
        save(capture)
    }

    /// Load all pending captures.
    func loadAll() -> [PendingCapture] {
        guard let data = defaults.data(forKey: Self.storageKey) else {
            return []
        }
        return (try? JSONDecoder().decode([PendingCapture].self, from: data)) ?? []
    }

    /// Drain all pending captures, removing them from storage.
    ///
    /// Called by the main app on launch to create tasks from captures.
    func drainAll() -> [PendingCapture] {
        let captures = loadAll()
        defaults.removeObject(forKey: Self.storageKey)
        return captures
    }

    /// Remove a specific capture by ID.
    func remove(id: UUID) {
        var pending = loadAll()
        pending.removeAll { $0.id == id }
        persist(pending)
    }

    /// Number of pending captures.
    var count: Int {
        loadAll().count
    }

    private func persist(_ captures: [PendingCapture]) {
        if let data = try? JSONEncoder().encode(captures) {
            defaults.set(data, forKey: Self.storageKey)
        }
    }
}
