import Foundation

// MARK: - Enums

enum TaskStatus: String, Sendable, CaseIterable {
    case inbox, pending, started, done, cancelled, someday
}

enum Priority: String, Sendable, CaseIterable, Comparable {
    case low, medium, high

    static func < (lhs: Priority, rhs: Priority) -> Bool {
        let order: [Priority] = [.low, .medium, .high]
        // swiftlint:disable:next force_unwrapping
        return order.firstIndex(of: lhs)! < order.firstIndex(of: rhs)!
    }
}

enum FocusState: String, Sendable {
    case working, shortBreak, longBreak, paused, aborted, completed
}

enum HabitDirection: String, Sendable {
    case build, breakHabit
}

// MARK: - Task

struct TaskItem: Identifiable, Hashable, Sendable {
    let id: String
    let sid: UInt32
    var title: String
    var status: TaskStatus
    var deadline: Date?
    var priority: Priority?
    var evening: Bool
    var urgency: Double
}

// MARK: - Time Block

struct TimeBlockItem: Identifiable, Hashable, Sendable {
    let id: String
    let sid: UInt32
    var title: String
    var startedAt: Date
    var endedAt: Date?
    var taskId: String?

    var isRunning: Bool { endedAt == nil }

    var duration: TimeInterval {
        let end = endedAt ?? Date()
        return end.timeIntervalSince(startedAt)
    }
}

// MARK: - Focus Session

struct FocusSessionItem: Identifiable, Hashable, Sendable {
    let id: String
    let sid: UInt32
    var startedAt: Date
    var endedAt: Date?
    var taskId: String?
    var plannedCycles: UInt32
    var completedCycles: UInt32
    var state: FocusState
    var workMinutes: UInt32
    var shortBreakMinutes: UInt32
    var longBreakMinutes: UInt32
}

// MARK: - Habit

struct HabitItem: Identifiable, Hashable, Sendable {
    let id: String
    let sid: UInt32
    var title: String
    var identity: String?
    var direction: HabitDirection
    var level: UInt32
    var streakCurrent: UInt32
    var streakBest: UInt32
    var levelName: String
}

struct HabitEntryItem: Identifiable, Sendable {
    let id: String
    let habitId: String
    var occurredAt: Date
    var notes: String?
    var slip: Bool
}
