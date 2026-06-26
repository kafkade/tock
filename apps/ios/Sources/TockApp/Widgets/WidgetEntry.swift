import Foundation

// MARK: - Widget Snapshot

/// Pre-fetched snapshot of app state for widget rendering.
///
/// Timeline providers load this once and pass subsets to each entry.
/// In production, this will be read from the App Group container.
struct WidgetSnapshot: Codable, Sendable {
    let todayTasks: [WidgetTask]
    let inboxTasks: [WidgetTask]
    let habits: [WidgetHabit]
    let activeTimer: WidgetTimer?
    let activeFocus: WidgetFocus?
    let isVaultLocked: Bool
    let dueCount: Int
}

// MARK: - Lightweight widget models

/// Task data subset for widget display. Avoids pulling full TaskItem.
struct WidgetTask: Codable, Identifiable, Hashable, Sendable {
    let id: String
    let sid: UInt32
    let title: String
    let priority: Priority?
    let deadline: Date?
    let evening: Bool
    let urgency: Double
}

/// Habit data subset for widget display.
struct WidgetHabit: Codable, Identifiable, Hashable, Sendable {
    let id: String
    let title: String
    let direction: HabitDirection
    let streakCurrent: UInt32
    let streakBest: UInt32
    let level: UInt32
}

/// Active timer data for widget display.
struct WidgetTimer: Codable, Sendable {
    let title: String
    let startedAt: Date
    let taskId: String?
}

/// Active focus session data for widget display.
struct WidgetFocus: Codable, Sendable {
    let completedCycles: UInt32
    let plannedCycles: UInt32
    let state: FocusState
    let workMinutes: UInt32
    let taskId: String?
}

// MARK: - Widget Snapshot Store

/// Data source protocol for widget timeline providers.
///
/// In production, reads from the App Group shared container.
/// For development, `MockWidgetSnapshotStore` provides static data.
protocol WidgetSnapshotStore: Sendable {
    func loadSnapshot() async -> WidgetSnapshot
}

/// Mock implementation for SwiftUI previews and development.
struct MockWidgetSnapshotStore: WidgetSnapshotStore {
    static let shared = MockWidgetSnapshotStore()

    func loadSnapshot() async -> WidgetSnapshot {
        WidgetSnapshot(
            todayTasks: [
                WidgetTask(id: "t1", sid: 1, title: "Review pull request #48",
                           priority: .high, deadline: Calendar.current.date(byAdding: .day, value: 1, to: Date()),
                           evening: false, urgency: 8.5),
                WidgetTask(id: "t3", sid: 3, title: "Write architecture doc",
                           priority: .high, deadline: Calendar.current.date(byAdding: .day, value: 3, to: Date()),
                           evening: false, urgency: 9.1),
                WidgetTask(id: "t4", sid: 4, title: "Update dependencies",
                           priority: .low, deadline: nil,
                           evening: false, urgency: 2.3),
                WidgetTask(id: "t6", sid: 6, title: "Plan sprint retrospective",
                           priority: .medium, deadline: Date(),
                           evening: false, urgency: 6.4),
                WidgetTask(id: "t7", sid: 7, title: "Draft blog post",
                           priority: nil, deadline: Calendar.current.date(byAdding: .day, value: 2, to: Date()),
                           evening: true, urgency: 3.8),
                WidgetTask(id: "t8", sid: 8, title: "Refactor auth module",
                           priority: .medium, deadline: nil,
                           evening: false, urgency: 4.1),
            ],
            inboxTasks: [
                WidgetTask(id: "t2", sid: 2, title: "Buy groceries",
                           priority: .medium, deadline: nil,
                           evening: true, urgency: 4.2),
            ],
            habits: [
                WidgetHabit(id: "h1", title: "Meditate",
                            direction: .build, streakCurrent: 12, streakBest: 21, level: 3),
                WidgetHabit(id: "h2", title: "Read 30 min",
                            direction: .build, streakCurrent: 45, streakBest: 45, level: 5),
                WidgetHabit(id: "h3", title: "No social media",
                            direction: .breakHabit, streakCurrent: 5, streakBest: 14, level: 2),
            ],
            activeTimer: WidgetTimer(
                title: "Code review",
                startedAt: Calendar.current.date(byAdding: .minute, value: -30, to: Date())!,
                taskId: "t1"
            ),
            activeFocus: nil,
            isVaultLocked: false,
            dueCount: 3
        )
    }
}
