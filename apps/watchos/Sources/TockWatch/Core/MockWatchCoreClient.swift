import Foundation

/// Mock implementation of `WatchCoreClient` for SwiftUI previews and development.
///
/// Returns static sample data. No persistence, no WatchConnectivity.
final class MockWatchCoreClient: WatchCoreClient, @unchecked Sendable {

    static let shared = MockWatchCoreClient()

    // MARK: - Sample data

    static let sampleTasks: [TaskItem] = [
        TaskItem(
            id: "t1", sid: 1, title: "Review pull request #48",
            status: .pending, deadline: Calendar.current.date(byAdding: .day, value: 1, to: Date()),
            priority: .high, evening: false, urgency: 8.5
        ),
        TaskItem(
            id: "t2", sid: 2, title: "Write architecture doc",
            status: .started, deadline: Calendar.current.date(byAdding: .day, value: 3, to: Date()),
            priority: .high, evening: false, urgency: 9.1
        ),
        TaskItem(
            id: "t3", sid: 3, title: "Buy groceries",
            status: .pending, deadline: nil,
            priority: .medium, evening: true, urgency: 4.2
        ),
        TaskItem(
            id: "t4", sid: 4, title: "Update dependencies",
            status: .pending, deadline: nil,
            priority: .low, evening: false, urgency: 2.3
        ),
        TaskItem(
            id: "t5", sid: 5, title: "Plan sprint retrospective",
            status: .pending, deadline: Date(),
            priority: .medium, evening: false, urgency: 6.4
        ),
    ]

    static let sampleHabits: [HabitItem] = [
        HabitItem(
            id: "h1", sid: 1, title: "Morning meditation",
            identity: "I am a person who starts each day with clarity",
            direction: .build, level: 3, streakCurrent: 12,
            streakBest: 21, levelName: "Practitioner"
        ),
        HabitItem(
            id: "h2", sid: 2, title: "Read for 30 minutes",
            identity: "I am a lifelong learner",
            direction: .build, level: 5, streakCurrent: 45,
            streakBest: 45, levelName: "Devoted"
        ),
        HabitItem(
            id: "h3", sid: 3, title: "No social media before noon",
            identity: "I am in control of my attention",
            direction: .breakHabit, level: 2, streakCurrent: 5,
            streakBest: 14, levelName: "Apprentice"
        ),
    ]

    static let sampleTimer = TimeBlockItem(
        id: "tb1", sid: 1, title: "Code review",
        startedAt: Calendar.current.date(byAdding: .minute, value: -30, to: Date())!,
        endedAt: nil, taskId: "t1"
    )

    // MARK: - WatchCoreClient

    func listTodayTasks() async throws -> [TaskItem] {
        Self.sampleTasks
    }

    func completeTask(id: String) async throws {}

    func currentTimer() async throws -> TimeBlockItem? {
        Self.sampleTimer
    }

    func startTimer(title: String, taskId: String?) async throws -> TimeBlockItem {
        TimeBlockItem(
            id: UUID().uuidString, sid: UInt32.random(in: 100...999),
            title: title, startedAt: Date(), endedAt: nil, taskId: taskId
        )
    }

    func stopTimer() async throws -> TimeBlockItem? { nil }

    func focusStatus() async throws -> FocusSessionItem? { nil }

    func startFocus(taskId: String?, cycles: UInt32) async throws -> FocusSessionItem {
        FocusSessionItem(
            id: UUID().uuidString, sid: UInt32.random(in: 100...999),
            startedAt: Date(), endedAt: nil,
            taskId: taskId, plannedCycles: cycles, completedCycles: 0,
            state: .working, workMinutes: 25,
            shortBreakMinutes: 5, longBreakMinutes: 15
        )
    }

    func completeFocusCycle() async throws -> FocusSessionItem {
        FocusSessionItem(
            id: "f1", sid: 1, startedAt: Date(), endedAt: nil,
            taskId: nil, plannedCycles: 4, completedCycles: 1,
            state: .shortBreak, workMinutes: 25,
            shortBreakMinutes: 5, longBreakMinutes: 15
        )
    }

    func skipBreak() async throws -> FocusSessionItem { try await completeFocusCycle() }
    func pauseFocus() async throws -> FocusSessionItem { try await completeFocusCycle() }
    func resumeFocus() async throws -> FocusSessionItem { try await completeFocusCycle() }
    func abortFocus() async throws -> FocusSessionItem { try await completeFocusCycle() }

    func listHabits() async throws -> [HabitItem] {
        Self.sampleHabits
    }

    func logHabit(id: String, notes: String?) async throws -> HabitEntryItem {
        HabitEntryItem(
            id: UUID().uuidString, habitId: id,
            occurredAt: Date(), notes: notes, slip: false
        )
    }
}
