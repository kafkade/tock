import SwiftUI

/// Today view — agenda of tasks due or prioritized for today.
struct TodayView: View {
    @Environment(AppState.self) private var appState
    @State private var viewModel: TodayViewModel?

    var body: some View {
        Group {
            if let vm = viewModel {
                contentView(vm: vm)
            } else {
                ProgressView()
            }
        }
        .navigationTitle("Today")
        .task {
            let vm = TodayViewModel(client: appState.client)
            viewModel = vm
            await vm.load()
        }
        .refreshable {
            await viewModel?.load()
        }
    }

    @ViewBuilder
    private func contentView(vm: TodayViewModel) -> some View {
        if vm.isLoading && vm.tasks.isEmpty {
            ProgressView("Loading...")
        } else if vm.tasks.isEmpty {
            ContentUnavailableView(
                "All clear",
                systemImage: "sun.max.fill",
                description: Text("No tasks for today. Enjoy!")
            )
        } else {
            List {
                ForEach(vm.tasks) { task in
                    NavigationLink(value: task) {
                        TaskRow(task: task) {
                            Task { await vm.completeTask(task) }
                        }
                    }
                }
            }
            .navigationDestination(for: TaskItem.self) { task in
                TaskDetailView(task: task)
            }
        }
    }
}
