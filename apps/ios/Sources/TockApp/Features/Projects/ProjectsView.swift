import SwiftUI

/// Project browser — lists active projects, drills into tasks.
struct ProjectsView: View {
    @Environment(AppState.self) private var appState
    @State private var viewModel: ProjectsViewModel?

    var body: some View {
        Group {
            if let vm = viewModel {
                contentView(vm: vm)
            } else {
                ProgressView()
            }
        }
        .navigationTitle("Projects")
        .task {
            let vm = ProjectsViewModel(client: appState.client)
            viewModel = vm
            await vm.load()
        }
        .refreshable {
            await viewModel?.load()
        }
    }

    @ViewBuilder
    private func contentView(vm: ProjectsViewModel) -> some View {
        if vm.isLoading && vm.projects.isEmpty {
            ProgressView("Loading...")
        } else if vm.projects.isEmpty {
            ContentUnavailableView(
                "No projects",
                systemImage: "folder",
                description: Text("Create a project to organize your tasks.")
            )
        } else {
            List {
                ForEach(vm.projects) { project in
                    NavigationLink(value: project) {
                        VStack(alignment: .leading, spacing: TockTheme.Spacing.xxs) {
                            Text(project.name)
                                .font(.body)

                            if let notes = project.notes {
                                Text(notes)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                        }
                    }
                }
            }
            .navigationDestination(for: ProjectItem.self) { project in
                ProjectDetailView(project: project)
            }
        }
    }
}
