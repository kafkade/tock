// NOTE: Shared with apps/ios — extract to shared package when apps/shared is created.

import SwiftUI

/// Project detail — shows project info and its tasks.
struct ProjectDetailView: View {
    @Environment(AppSessionState.self) private var appState
    let project: ProjectItem

    @State private var tasks: [TaskItem] = []
    @State private var isLoading = false

    var body: some View {
        Group {
            if isLoading && tasks.isEmpty {
                ProgressView("Loading...")
            } else if tasks.isEmpty {
                ContentUnavailableView(
                    "No tasks",
                    systemImage: "checklist",
                    description: Text("This project has no tasks yet.")
                )
            } else {
                List {
                    if let notes = project.notes {
                        Section("Notes") {
                            Text(notes)
                                .font(.callout)
                                .foregroundStyle(.secondary)
                        }
                    }

                    Section("Tasks") {
                        ForEach(tasks) { task in
                            NavigationLink(value: task) {
                                TaskRow(task: task)
                            }
                        }
                    }
                }
                .navigationDestination(for: TaskItem.self) { task in
                    TaskDetailView(task: task)
                }
            }
        }
        .navigationTitle(project.name)
        .task {
            await loadTasks()
        }
    }

    private func loadTasks() async {
        isLoading = true
        tasks = (try? await appState.client.listTasks(filter: .project(id: project.id))) ?? []
        isLoading = false
    }
}
