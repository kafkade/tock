import AppIntents
import Foundation

// MARK: - Add Task Intent

/// Siri: "Add task buy milk tomorrow to Tock"
///
/// Creates a new task with optional project, due date, and tags.
/// Donations enable Siri to learn the user's phrasing patterns.
struct AddTaskIntent: AppIntent {
    static var title: LocalizedStringResource = "Add Task"
    static var description: IntentDescription = "Creates a new task in tock."
    static var openAppWhenRun = false

    @Parameter(title: "Title")
    var taskTitle: String

    @Parameter(title: "Project")
    var project: ProjectEntity?

    @Parameter(title: "Due Date")
    var dueDate: Date?

    @Parameter(title: "Tags")
    var tags: [String]?

    @Parameter(title: "Priority")
    var priority: TaskPriorityParam?

    func perform() async throws -> some IntentResult & ProvidesDialog {
        // TODO: In production, delegate to CoreActor via App Group
        let client = MockCoreClient.shared
        let input = NewTaskInput(
            title: taskTitle,
            notes: nil,
            projectId: project?.id,
            deadline: dueDate,
            priority: priority?.toPriority,
            evening: false,
            tags: tags ?? []
        )
        let task = try await client.addTask(input)

        var message = "Added '\(task.title)'"
        if let projectName = project?.name {
            message += " to \(projectName)"
        }
        if let dueDate {
            let formatter = DateFormatter()
            formatter.dateStyle = .medium
            message += " due \(formatter.string(from: dueDate))"
        }

        return .result(dialog: "\(message)")
    }
}

/// Priority parameter for `AddTaskIntent`.
enum TaskPriorityParam: String, AppEnum {
    case low, medium, high

    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "Priority")
    static var caseDisplayRepresentations: [TaskPriorityParam: DisplayRepresentation] = [
        .low: "Low",
        .medium: "Medium",
        .high: "High",
    ]

    var toPriority: Priority {
        switch self {
        case .low: return .low
        case .medium: return .medium
        case .high: return .high
        }
    }
}

// MARK: - Complete Task Intent

/// Siri: "Mark dentist done in Tock"
///
/// Completes a task by entity reference. Siri resolves the task name
/// via `TaskEntityQuery`. Also used by widget interactive checkboxes
/// (via the convenience `init(task:)` with a pre-built `TaskEntity`).
struct CompleteTaskIntent: AppIntent {
    static var title: LocalizedStringResource = "Complete Task"
    static var description: IntentDescription = "Marks a task as done in tock."
    static var openAppWhenRun = false

    @Parameter(title: "Task")
    var task: TaskEntity

    init() {}

    init(task: TaskEntity) {
        self.task = task
    }

    func perform() async throws -> some IntentResult & ProvidesDialog {
        // TODO: In production, complete via CoreActor / App Group storage
        let client = MockCoreClient.shared
        try await client.completeTask(id: task.id)
        return .result(dialog: "Completed '\(task.title)' ✓")
    }
}

// MARK: - Capture to Inbox Intent

/// Siri: "Capture think about Q4 plan to Tock inbox"
///
/// Quick-capture text as an inbox task. Minimal friction — only
/// requires the text, no project or metadata.
struct CaptureToInboxIntent: AppIntent {
    static var title: LocalizedStringResource = "Capture to Inbox"
    static var description: IntentDescription = "Adds a quick note to your tock inbox."
    static var openAppWhenRun = false

    @Parameter(title: "Text")
    var text: String

    func perform() async throws -> some IntentResult & ProvidesDialog {
        let client = MockCoreClient.shared
        let input = NewTaskInput(title: text)
        _ = try await client.addTask(input)
        return .result(dialog: "Captured to inbox: '\(text)'")
    }
}
