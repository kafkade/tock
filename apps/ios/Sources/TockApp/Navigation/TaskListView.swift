import SwiftUI

/// Reusable task list used in both iPhone tabs and the iPad content column.
///
/// Shows tasks for a given filter with selection support for detail navigation.
struct TaskListView: View {
    @Environment(AppState.self) private var appState

    let filter: TaskFilter
    @Binding var selectedTaskId: String?

    @State private var tasks: [TaskItem] = []
    @State private var isLoading = false
    @State private var error: String?
    @State private var selectedTaskIds: Set<String> = []

    var body: some View {
        Group {
            if isLoading && tasks.isEmpty {
                ProgressView("Loading…")
            } else if tasks.isEmpty {
                emptyState
            } else {
                taskList
            }
        }
        .navigationTitle(filter.title)
        .task(id: filter.stableKey) {
            await load()
        }
        .refreshable {
            await load()
        }
    }

    // MARK: - Task list

    @ViewBuilder
    private var taskList: some View {
        List(selection: $selectedTaskId) {
            ForEach(tasks) { task in
                TaskRow(task: task) {
                    Task { await completeTask(task) }
                } onDelete: {
                    Task { await deleteTask(task) }
                }
                .draggable(TaskTransferable(taskId: task.id))
                .tag(task.id)
            }
        }
    }

    // MARK: - Empty states

    @ViewBuilder
    private var emptyState: some View {
        switch filter {
        case .today:
            ContentUnavailableView(
                "All clear",
                systemImage: "sun.max.fill",
                description: Text("No tasks for today. Enjoy!")
            )
        case .inbox:
            ContentUnavailableView(
                "Inbox zero",
                systemImage: "tray",
                description: Text("No unprocessed tasks.")
            )
        case .logbook:
            ContentUnavailableView(
                "No completed tasks",
                systemImage: "book.closed",
                description: Text("Completed tasks will appear here.")
            )
        default:
            ContentUnavailableView(
                "No tasks",
                systemImage: "checklist",
                description: Text("No tasks match this filter.")
            )
        }
    }

    // MARK: - Actions

    private func load() async {
        isLoading = true
        error = nil
        do {
            tasks = try await appState.client.listTasks(filter: filter)
                .sorted { $0.urgency > $1.urgency }
        } catch {
            self.error = error.localizedDescription
        }
        isLoading = false
    }

    private func completeTask(_ task: TaskItem) async {
        do {
            try await appState.client.completeTask(id: task.id)
            tasks.removeAll { $0.id == task.id }
            if selectedTaskId == task.id {
                selectedTaskId = nil
            }
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func deleteTask(_ task: TaskItem) async {
        do {
            try await appState.client.deleteTask(id: task.id)
            tasks.removeAll { $0.id == task.id }
            if selectedTaskId == task.id {
                selectedTaskId = nil
            }
        } catch {
            self.error = error.localizedDescription
        }
    }
}

// MARK: - TaskFilter helpers

extension TaskFilter {
    /// Display title for the filter.
    var title: String {
        switch self {
        case .today: "Today"
        case .inbox: "Inbox"
        case .upcoming: "Upcoming"
        case .anytime: "Anytime"
        case .someday: "Someday"
        case .logbook: "Logbook"
        case .project: "Project"
        case .all: "All Tasks"
        }
    }

    /// Stable string key for `.task(id:)` identity.
    var stableKey: String {
        switch self {
        case .today: "today"
        case .inbox: "inbox"
        case .upcoming: "upcoming"
        case .anytime: "anytime"
        case .someday: "someday"
        case .logbook: "logbook"
        case .project(let id): "project-\(id)"
        case .all: "all"
        }
    }
}

extension TaskFilter: Equatable {
    static func == (lhs: TaskFilter, rhs: TaskFilter) -> Bool {
        switch (lhs, rhs) {
        case (.today, .today), (.inbox, .inbox), (.upcoming, .upcoming),
             (.anytime, .anytime), (.someday, .someday), (.logbook, .logbook),
             (.all, .all):
            return true
        case (.project(let a), .project(let b)):
            return a == b
        default:
            return false
        }
    }
}

extension TaskFilter: Hashable {
    func hash(into hasher: inout Hasher) {
        hasher.combine(stableKey)
    }
}
