// NOTE: Shared with apps/ios — extract to shared package when apps/shared is created.

import Foundation

// MARK: - Enums

enum TaskStatus: String, Sendable, CaseIterable {
    case inbox, pending, started, done, cancelled, someday
}

enum Priority: String, Sendable, CaseIterable, Comparable {
    case low, medium, high

    static func < (lhs: Priority, rhs: Priority) -> Bool {
        let order: [Priority] = [.low, .medium, .high]
        return order.firstIndex(of: lhs)! < order.firstIndex(of: rhs)!
    }
}

enum ProjectStatus: String, Sendable {
    case active, paused, someday, done, cancelled
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
    var notes: String?
    var status: TaskStatus
    var projectId: String?
    var deadline: Date?
    var priority: Priority?
    var evening: Bool
    var tags: [String]
    var dependsOn: [String]
    var urgency: Double
    let createdAt: Date
    var modifiedAt: Date
    var doneAt: Date?
}

// MARK: - Project

struct ProjectItem: Identifiable, Hashable, Sendable {
    let id: String
    let sid: UInt32
    var name: String
    var notes: String?
    var status: ProjectStatus
    var areaId: String?
    var deadline: Date?
    let createdAt: Date
    var modifiedAt: Date
}

// MARK: - Area

struct AreaItem: Identifiable, Hashable, Sendable {
    let id: String
    var name: String
    var color: String?
    let createdAt: Date
}

// MARK: - Tag

struct TagItem: Identifiable, Hashable, Sendable {
    let id: String
    var name: String
    var color: String?
}

// MARK: - Time Block

struct TimeBlockItem: Identifiable, Hashable, Sendable {
    let id: String
    let sid: UInt32
    var title: String
    var startedAt: Date
    var endedAt: Date?
    var taskId: String?
    var projectId: String?
    var notes: String?
    var billable: Bool

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
    var projectId: String?
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
    var xp: UInt32
    var streakCurrent: UInt32
    var streakBest: UInt32
    var levelName: String
    var areaId: String?
    let createdAt: Date
    var archivedAt: Date?
}

struct HabitEntryItem: Identifiable, Sendable {
    let id: String
    let habitId: String
    var occurredAt: Date
    var notes: String?
    var slip: Bool
}

// MARK: - Input types

struct NewTaskInput: Sendable {
    var title: String
    var notes: String?
    var projectId: String?
    var deadline: Date?
    var priority: Priority?
    var evening: Bool = false
    var tags: [String] = []
}

struct NewProjectInput: Sendable {
    var name: String
    var notes: String?
    var areaId: String?
}

// MARK: - Filter

enum TaskFilter: Sendable {
    case today
    case inbox
    case upcoming
    case anytime
    case someday
    case logbook
    case project(id: String)
    case all
}
