import SwiftUI

/// Inbox view — unprocessed tasks awaiting triage.
struct InboxView: View {
    @Environment(AppState.self) private var appState
    @State private var viewModel: InboxViewModel?

    var body: some View {
        Group {
            if let vm = viewModel {
                contentView(vm: vm)
            } else {
                ProgressView()
            }
        }
        .navigationTitle("Inbox")
        .task {
            let vm = InboxViewModel(client: appState.client)
            viewModel = vm
            await vm.load()
        }
        .refreshable {
            await viewModel?.load()
        }
    }

    @ViewBuilder
    private func contentView(vm: InboxViewModel) -> some View {
        if vm.isLoading && vm.tasks.isEmpty {
            ProgressView("Loading...")
        } else if vm.tasks.isEmpty {
            ContentUnavailableView(
                "Inbox zero",
                systemImage: "tray",
                description: Text("No unprocessed tasks.")
            )
        } else {
            List {
                ForEach(vm.tasks) { task in
                    NavigationLink(value: task) {
                        TaskRow(
                            task: task,
                            onComplete: { Task { await vm.completeTask(task) } },
                            onDelete: { Task { await vm.deleteTask(task) } }
                        )
                    }
                }
            }
            .navigationDestination(for: TaskItem.self) { task in
                TaskDetailView(task: task)
            }
        }
    }
}
