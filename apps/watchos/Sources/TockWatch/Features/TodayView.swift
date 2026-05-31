import SwiftUI

/// Today task list for the watch — up to 20 items, sorted by urgency.
///
/// Digital Crown naturally scrolls the list. Tap the checkmark on any
/// task to complete it with haptic feedback.
struct TodayView: View {
    @Environment(WatchAppState.self) private var appState
    @State private var tasks: [TaskItem] = []
    @State private var isLoading = false

    var body: some View {
        Group {
            if isLoading && tasks.isEmpty {
                ProgressView()
            } else if tasks.isEmpty {
                emptyView
            } else {
                taskList
            }
        }
        .navigationTitle("Today")
        .task {
            await load()
        }
    }

    @ViewBuilder
    private var taskList: some View {
        List {
            ForEach(tasks) { task in
                WatchTaskRow(task: task) {
                    Task { await completeTask(task) }
                }
            }
        }
    }

    @ViewBuilder
    private var emptyView: some View {
        VStack(spacing: WatchTheme.Spacing.lg) {
            Image(systemName: "sun.max.fill")
                .font(.title2)
                .foregroundStyle(.yellow)
            Text("All clear")
                .font(.headline)
            Text("No tasks for today")
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }

    private func load() async {
        isLoading = true
        do {
            tasks = try await appState.client.listTodayTasks()
                .sorted { $0.urgency > $1.urgency }
                .prefix(20)
                .map { $0 }
        } catch {
            tasks = []
        }
        isLoading = false
    }

    private func completeTask(_ task: TaskItem) async {
        do {
            try await appState.client.completeTask(id: task.id)
            tasks.removeAll { $0.id == task.id }
        } catch {
            // Silently fail — intent will be retried via queue
        }
    }
}
