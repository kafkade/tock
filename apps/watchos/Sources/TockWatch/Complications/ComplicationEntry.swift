import Foundation

// MARK: - Complication Snapshot

/// Pre-fetched snapshot of app state for complication rendering.
///
/// Timeline providers read this once from the shared snapshot store
/// and pass subsets to each complication entry.
struct ComplicationSnapshot: Sendable {
    let todayTasks: [ComplicationTask]
    let habits: [ComplicationHabit]
    let activeTimer: ComplicationTimer?
    let activeFocus: ComplicationFocus?
    let isVaultLocked: Bool
    let dueCount: Int
}

/// Task data subset for complication display.
struct ComplicationTask: Identifiable, Hashable, Sendable {
    let id: String
    let title: String
    let priority: Priority?
    let deadline: Date?
    let urgency: Double
}

/// Habit data subset for complication display.
struct ComplicationHabit: Identifiable, Hashable, Sendable {
    let id: String
    let title: String
    let direction: HabitDirection
    let streakCurrent: UInt32
    let streakBest: UInt32
}

/// Active timer data for complication display.
struct ComplicationTimer: Sendable {
    let title: String
    let startedAt: Date
}

/// Active focus data for complication display.
struct ComplicationFocus: Sendable {
    let completedCycles: UInt32
    let plannedCycles: UInt32
    let state: FocusState
}

// MARK: - Snapshot Store

/// Shared data source for complication timeline providers.
///
/// Updated by `WatchSessionManager` when new data arrives from iPhone.
/// In development, `MockComplicationSnapshotStore` provides static data.
///
/// Uses UserDefaults in an app group container so the complication
/// widget extension (separate process) can read the same data.
final class ComplicationSnapshotStore: @unchecked Sendable {

    static let shared = ComplicationSnapshotStore()

    private let defaults = UserDefaults.standard
    private let storageKey = "com.kafkade.tock.watch.complicationSnapshot"

    /// Load the current snapshot. Returns mock data in development.
    func loadSnapshot() -> ComplicationSnapshot {
        // TODO: Decode from UserDefaults when live data is available.
        // For now, return mock data.
        Self.mockSnapshot
    }

    /// Update the stored snapshot from a WatchConnectivity context dictionary.
    ///
    /// Called by `WatchSessionManager` when new data arrives from iPhone.
    func update(from context: [String: Any]) {
        // TODO: Decode context values into snapshot and persist.
        // In development, this is a no-op — mock data is always used.
        _ = context
    }

    // MARK: - Mock data

    static let mockSnapshot = ComplicationSnapshot(
        todayTasks: [
            ComplicationTask(id: "t1", title: "Review pull request #48",
                             priority: .high, deadline: Calendar.current.date(byAdding: .day, value: 1, to: Date()),
                             urgency: 8.5),
            ComplicationTask(id: "t2", title: "Write architecture doc",
                             priority: .high, deadline: Calendar.current.date(byAdding: .day, value: 3, to: Date()),
                             urgency: 9.1),
            ComplicationTask(id: "t3", title: "Buy groceries",
                             priority: .medium, deadline: nil, urgency: 4.2),
        ],
        habits: [
            ComplicationHabit(id: "h1", title: "Meditate",
                              direction: .build, streakCurrent: 12, streakBest: 21),
            ComplicationHabit(id: "h2", title: "Read 30 min",
                              direction: .build, streakCurrent: 45, streakBest: 45),
            ComplicationHabit(id: "h3", title: "No social media",
                              direction: .breakHabit, streakCurrent: 5, streakBest: 14),
        ],
        activeTimer: ComplicationTimer(
            title: "Code review",
            startedAt: Calendar.current.date(byAdding: .minute, value: -30, to: Date())!
        ),
        activeFocus: nil,
        isVaultLocked: false,
        dueCount: 3
    )
}

// MARK: - Complication Entry

/// Timeline entry shared by all complication providers.
struct ComplicationEntry: Sendable {
    let date: Date
    let snapshot: ComplicationSnapshot
}

// MARK: - Placeholder

extension ComplicationSnapshot {
    /// Placeholder snapshot with redacted content for complication gallery.
    static let placeholder = ComplicationSnapshot(
        todayTasks: [
            ComplicationTask(id: "p1", title: "Loading…", priority: .high,
                             deadline: Date(), urgency: 8.0),
            ComplicationTask(id: "p2", title: "Loading…", priority: .medium,
                             deadline: nil, urgency: 5.0),
            ComplicationTask(id: "p3", title: "Loading…", priority: nil,
                             deadline: nil, urgency: 3.0),
        ],
        habits: [
            ComplicationHabit(id: "ph1", title: "Loading…", direction: .build,
                              streakCurrent: 7, streakBest: 14),
        ],
        activeTimer: nil,
        activeFocus: nil,
        isVaultLocked: false,
        dueCount: 0
    )
}
