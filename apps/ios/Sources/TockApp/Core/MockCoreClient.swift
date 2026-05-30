import Foundation

/// Mock implementation of `CoreClient` for SwiftUI previews and development.
///
/// Returns static sample data. No persistence, no vault, no Rust calls.
final class MockCoreClient: CoreClient, @unchecked Sendable {

    static let shared = MockCoreClient()

    // MARK: - Sample data

    static let sampleTasks: [TaskItem] = [
        TaskItem(
            id: "t1", sid: 1, title: "Review pull request #48",
            notes: "Check UniFFI bindings", status: .pending,
            projectId: "p1", deadline: Calendar.current.date(byAdding: .day, value: 1, to: Date()),
            priority: .high, evening: false, tags: ["code-review"],
            dependsOn: [], urgency: 8.5,
            createdAt: Date(), modifiedAt: Date(), doneAt: nil
        ),
        TaskItem(
            id: "t2", sid: 2, title: "Buy groceries",
            notes: nil, status: .inbox,
            projectId: nil, deadline: nil,
            priority: .medium, evening: true, tags: ["errands", "home"],
            dependsOn: [], urgency: 4.2,
            createdAt: Date(), modifiedAt: Date(), doneAt: nil
        ),
        TaskItem(
            id: "t3", sid: 3, title: "Write architecture doc",
            notes: "Cover sync protocol design", status: .started,
            projectId: "p1", deadline: Calendar.current.date(byAdding: .day, value: 3, to: Date()),
            priority: .high, evening: false, tags: ["docs"],
            dependsOn: [], urgency: 9.1,
            createdAt: Date(), modifiedAt: Date(), doneAt: nil
        ),
        TaskItem(
            id: "t4", sid: 4, title: "Update dependencies",
            notes: nil, status: .pending,
            projectId: "p1", deadline: nil,
            priority: .low, evening: false, tags: ["maintenance"],
            dependsOn: ["t3"], urgency: 2.3,
            createdAt: Date(), modifiedAt: Date(), doneAt: nil
        ),
        TaskItem(
            id: "t5", sid: 5, title: "Plan vacation",
            notes: "Research destinations", status: .someday,
            projectId: nil, deadline: nil,
            priority: nil, evening: false, tags: ["personal"],
            dependsOn: [], urgency: 0.5,
            createdAt: Date(), modifiedAt: Date(), doneAt: nil
        ),
    ]

    static let sampleProjects: [ProjectItem] = [
        ProjectItem(
            id: "p1", sid: 1, name: "Tock v1.0",
            notes: "Ship the first stable release", status: .active,
            areaId: "a1", deadline: nil,
            createdAt: Date(), modifiedAt: Date()
        ),
        ProjectItem(
            id: "p2", sid: 2, name: "Home renovation",
            notes: nil, status: .active,
            areaId: "a2", deadline: nil,
            createdAt: Date(), modifiedAt: Date()
        ),
    ]

    static let sampleAreas: [AreaItem] = [
        AreaItem(id: "a1", name: "Work", color: "#3B82F6", createdAt: Date()),
        AreaItem(id: "a2", name: "Personal", color: "#10B981", createdAt: Date()),
    ]

    static let sampleTags: [TagItem] = [
        TagItem(id: "tg1", name: "code-review", color: "#EF4444"),
        TagItem(id: "tg2", name: "errands", color: "#F59E0B"),
        TagItem(id: "tg3", name: "docs", color: "#8B5CF6"),
        TagItem(id: "tg4", name: "maintenance", color: "#6B7280"),
        TagItem(id: "tg5", name: "home", color: "#10B981"),
        TagItem(id: "tg6", name: "personal", color: "#EC4899"),
    ]

    static let sampleTimeBlocks: [TimeBlockItem] = [
        TimeBlockItem(
            id: "tb1", sid: 1, title: "Deep work: architecture doc",
            startedAt: Calendar.current.date(byAdding: .hour, value: -2, to: Date())!,
            endedAt: Calendar.current.date(byAdding: .hour, value: -1, to: Date())!,
            taskId: "t3", projectId: "p1", notes: nil, billable: false
        ),
        TimeBlockItem(
            id: "tb2", sid: 2, title: "Code review",
            startedAt: Calendar.current.date(byAdding: .minute, value: -30, to: Date())!,
            endedAt: nil,
            taskId: "t1", projectId: "p1", notes: nil, billable: false
        ),
    ]

    static let sampleHabits: [HabitItem] = [
        HabitItem(
            id: "h1", sid: 1, title: "Morning meditation",
            identity: "I am a person who starts each day with clarity",
            direction: .build, level: 3, xp: 89,
            streakCurrent: 12, streakBest: 21, levelName: "Practitioner",
            areaId: "a2", createdAt: Date(), archivedAt: nil
        ),
        HabitItem(
            id: "h2", sid: 2, title: "Read for 30 minutes",
            identity: "I am a lifelong learner",
            direction: .build, level: 5, xp: 233,
            streakCurrent: 45, streakBest: 45, levelName: "Devoted",
            areaId: "a2", createdAt: Date(), archivedAt: nil
        ),
        HabitItem(
            id: "h3", sid: 3, title: "No social media before noon",
            identity: "I am in control of my attention",
            direction: .breakHabit, level: 2, xp: 34,
            streakCurrent: 5, streakBest: 14, levelName: "Apprentice",
            areaId: nil, createdAt: Date(), archivedAt: nil
        ),
    ]

    // MARK: - CoreClient

    func listTasks(filter: TaskFilter) async throws -> [TaskItem] {
        switch filter {
        case .today:
            return Self.sampleTasks.filter { $0.status == .started || $0.status == .pending }
                .filter { $0.deadline != nil || $0.priority == .high }
        case .inbox:
            return Self.sampleTasks.filter { $0.status == .inbox }
        case .upcoming:
            return Self.sampleTasks.filter { $0.deadline != nil && $0.status != .done }
        case .someday:
            return Self.sampleTasks.filter { $0.status == .someday }
        case .logbook:
            return Self.sampleTasks.filter { $0.status == .done || $0.status == .cancelled }
        case .project(let id):
            return Self.sampleTasks.filter { $0.projectId == id }
        case .anytime:
            return Self.sampleTasks.filter {
                $0.status == .pending || $0.status == .started
            }
        case .all:
            return Self.sampleTasks
        }
    }

    func addTask(_ input: NewTaskInput) async throws -> TaskItem {
        TaskItem(
            id: UUID().uuidString, sid: UInt32.random(in: 100...999),
            title: input.title, notes: input.notes, status: .inbox,
            projectId: input.projectId, deadline: input.deadline,
            priority: input.priority, evening: input.evening,
            tags: input.tags, dependsOn: [], urgency: 1.0,
            createdAt: Date(), modifiedAt: Date(), doneAt: nil
        )
    }

    func completeTask(id: String) async throws {}
    func deleteTask(id: String) async throws {}

    func listProjects() async throws -> [ProjectItem] { Self.sampleProjects }

    func addProject(_ input: NewProjectInput) async throws -> ProjectItem {
        ProjectItem(
            id: UUID().uuidString, sid: UInt32.random(in: 100...999),
            name: input.name, notes: input.notes, status: .active,
            areaId: input.areaId, deadline: nil,
            createdAt: Date(), modifiedAt: Date()
        )
    }

    func listAreas() async throws -> [AreaItem] { Self.sampleAreas }
    func listTags() async throws -> [TagItem] { Self.sampleTags }

    func startTimer(title: String, taskId: String?) async throws -> TimeBlockItem {
        TimeBlockItem(
            id: UUID().uuidString, sid: UInt32.random(in: 100...999),
            title: title, startedAt: Date(), endedAt: nil,
            taskId: taskId, projectId: nil, notes: nil, billable: false
        )
    }

    func stopTimer() async throws -> TimeBlockItem? { nil }
    func currentTimer() async throws -> TimeBlockItem? {
        Self.sampleTimeBlocks.first { $0.isRunning }
    }
    func listTimeBlocks() async throws -> [TimeBlockItem] { Self.sampleTimeBlocks }

    func startFocus(taskId: String?, cycles: UInt32) async throws -> FocusSessionItem {
        FocusSessionItem(
            id: UUID().uuidString, sid: UInt32.random(in: 100...999),
            startedAt: Date(), endedAt: nil,
            taskId: taskId, projectId: nil,
            plannedCycles: cycles, completedCycles: 0, state: .working,
            workMinutes: 25, shortBreakMinutes: 5, longBreakMinutes: 15
        )
    }

    func focusStatus() async throws -> FocusSessionItem? { nil }
    func completeFocusCycle() async throws -> FocusSessionItem {
        FocusSessionItem(
            id: "f1", sid: 1, startedAt: Date(), endedAt: nil,
            taskId: nil, projectId: nil,
            plannedCycles: 4, completedCycles: 1, state: .shortBreak,
            workMinutes: 25, shortBreakMinutes: 5, longBreakMinutes: 15
        )
    }
    func skipBreak() async throws -> FocusSessionItem { try await completeFocusCycle() }
    func pauseFocus() async throws -> FocusSessionItem { try await completeFocusCycle() }
    func resumeFocus() async throws -> FocusSessionItem { try await completeFocusCycle() }
    func abortFocus() async throws -> FocusSessionItem { try await completeFocusCycle() }

    func listHabits() async throws -> [HabitItem] { Self.sampleHabits }

    func addHabit(title: String, identity: String?) async throws -> HabitItem {
        HabitItem(
            id: UUID().uuidString, sid: UInt32.random(in: 100...999),
            title: title, identity: identity,
            direction: .build, level: 1, xp: 0,
            streakCurrent: 0, streakBest: 0, levelName: "Beginner",
            areaId: nil, createdAt: Date(), archivedAt: nil
        )
    }

    func logHabit(id: String, notes: String?) async throws -> HabitEntryItem {
        HabitEntryItem(
            id: UUID().uuidString, habitId: id,
            occurredAt: Date(), notes: notes, slip: false
        )
    }

    func vaultPath() -> String { "/mock/vault.tock" }
}
