import AppIntents
import Foundation

// MARK: - Task Entity

/// Makes tasks first-class in Siri and the Shortcuts app.
///
/// Users can reference tasks by name in voice commands:
/// "Mark dentist done in Tock" — Siri resolves "dentist" via `TaskEntityQuery`.
struct TaskEntity: AppEntity {
    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "Task")
    static var defaultQuery = TaskEntityQuery()

    var id: String
    var title: String
    var priority: String?
    var projectName: String?

    var displayRepresentation: DisplayRepresentation {
        var subtitle: String? = nil
        if let projectName {
            subtitle = projectName
        }
        return DisplayRepresentation(title: "\(title)", subtitle: subtitle.map { "\($0)" })
    }

    /// Convenience initializer for widget usage.
    init(id: String, title: String, priority: String? = nil, projectName: String? = nil) {
        self.id = id
        self.title = title
        self.priority = priority
        self.projectName = projectName
    }
}

/// String-based query for resolving tasks by name.
///
/// Supports Siri natural-language matching ("Mark dentist done")
/// and Shortcuts entity picker. Resolves tasks via the App Group vault
/// through `VaultGateway`.
struct TaskEntityQuery: EntityStringQuery {
    func entities(for identifiers: [String]) async throws -> [TaskEntity] {
        let client = try await VaultGateway.shared.client()
        let allTasks = try await client.listTasks(filter: .all)
        return allTasks
            .filter { identifiers.contains($0.id) }
            .map { TaskEntity(id: $0.id, title: $0.title, priority: $0.priority?.rawValue) }
    }

    func entities(matching query: String) async throws -> [TaskEntity] {
        let client = try await VaultGateway.shared.client()
        let allTasks = try await client.listTasks(filter: .all)
        let lowered = query.lowercased()
        return allTasks
            .filter { $0.title.lowercased().contains(lowered) }
            .map { TaskEntity(id: $0.id, title: $0.title, priority: $0.priority?.rawValue) }
    }

    func suggestedEntities() async throws -> [TaskEntity] {
        let client = try await VaultGateway.shared.client()
        let todayTasks = try await client.listTasks(filter: .today)
        return todayTasks.map {
            TaskEntity(id: $0.id, title: $0.title, priority: $0.priority?.rawValue)
        }
    }
}

// MARK: - Habit Entity

/// Makes habits first-class in Siri and Shortcuts.
///
/// "Log meditation 10 minutes" — Siri resolves "meditation" via `HabitEntityQuery`.
struct HabitEntity: AppEntity {
    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "Habit")
    static var defaultQuery = HabitEntityQuery()

    var id: String
    var title: String
    var streakCurrent: Int
    var levelName: String

    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(
            title: "\(title)",
            subtitle: "🔥 \(streakCurrent) · \(levelName)"
        )
    }

    init(id: String, title: String, streakCurrent: Int = 0, levelName: String = "") {
        self.id = id
        self.title = title
        self.streakCurrent = streakCurrent
        self.levelName = levelName
    }
}

struct HabitEntityQuery: EntityStringQuery {
    func entities(for identifiers: [String]) async throws -> [HabitEntity] {
        let client = try await VaultGateway.shared.client()
        let habits = try await client.listHabits()
        return habits
            .filter { identifiers.contains($0.id) }
            .map { HabitEntity(id: $0.id, title: $0.title, streakCurrent: Int($0.streakCurrent), levelName: $0.levelName) }
    }

    func entities(matching query: String) async throws -> [HabitEntity] {
        let client = try await VaultGateway.shared.client()
        let habits = try await client.listHabits()
        let lowered = query.lowercased()
        return habits
            .filter { $0.title.lowercased().contains(lowered) }
            .map { HabitEntity(id: $0.id, title: $0.title, streakCurrent: Int($0.streakCurrent), levelName: $0.levelName) }
    }

    func suggestedEntities() async throws -> [HabitEntity] {
        let client = try await VaultGateway.shared.client()
        let habits = try await client.listHabits()
        return habits.map {
            HabitEntity(id: $0.id, title: $0.title, streakCurrent: Int($0.streakCurrent), levelName: $0.levelName)
        }
    }
}

// MARK: - Project Entity

/// Makes projects first-class in Siri and Shortcuts.
///
/// Used as an optional parameter in `AddTaskIntent` to specify the target project.
struct ProjectEntity: AppEntity {
    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "Project")
    static var defaultQuery = ProjectEntityQuery()

    var id: String
    var name: String

    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(title: "\(name)")
    }

    init(id: String, name: String) {
        self.id = id
        self.name = name
    }
}

struct ProjectEntityQuery: EntityStringQuery {
    func entities(for identifiers: [String]) async throws -> [ProjectEntity] {
        let client = try await VaultGateway.shared.client()
        let projects = try await client.listProjects()
        return projects
            .filter { identifiers.contains($0.id) }
            .map { ProjectEntity(id: $0.id, name: $0.name) }
    }

    func entities(matching query: String) async throws -> [ProjectEntity] {
        let client = try await VaultGateway.shared.client()
        let projects = try await client.listProjects()
        let lowered = query.lowercased()
        return projects
            .filter { $0.name.lowercased().contains(lowered) }
            .map { ProjectEntity(id: $0.id, name: $0.name) }
    }

    func suggestedEntities() async throws -> [ProjectEntity] {
        let client = try await VaultGateway.shared.client()
        let projects = try await client.listProjects()
        return projects.map { ProjectEntity(id: $0.id, name: $0.name) }
    }
}

// MARK: - Report Entity

/// Makes custom reports available in Siri and Shortcuts.
///
/// "Run standup report" — Siri resolves "standup" via `ReportEntityQuery`.
/// Currently provides a static mock catalog; production will query the core.
struct ReportEntity: AppEntity {
    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "Report")
    static var defaultQuery = ReportEntityQuery()

    var id: String
    var name: String
    var summary: String

    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(title: "\(name)", subtitle: "\(summary)")
    }

    init(id: String, name: String, summary: String = "") {
        self.id = id
        self.name = name
        self.summary = summary
    }
}

struct ReportEntityQuery: EntityStringQuery {
    /// Mock report catalog — mirrors built-in views until custom reports are wired.
    private static let mockReports: [ReportEntity] = [
        ReportEntity(id: "r-standup", name: "Standup", summary: "Yesterday + Today + Blocked"),
        ReportEntity(id: "r-weekly", name: "Weekly Review", summary: "Completed this week"),
        ReportEntity(id: "r-time", name: "Time Summary", summary: "Hours tracked today"),
    ]

    func entities(for identifiers: [String]) async throws -> [ReportEntity] {
        Self.mockReports.filter { identifiers.contains($0.id) }
    }

    func entities(matching query: String) async throws -> [ReportEntity] {
        let lowered = query.lowercased()
        return Self.mockReports.filter { $0.name.lowercased().contains(lowered) }
    }

    func suggestedEntities() async throws -> [ReportEntity] {
        Self.mockReports
    }
}
