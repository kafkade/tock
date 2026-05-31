import SwiftUI

/// iPad sidebar — three-column NavigationSplitView sidebar.
///
/// Shows smart views (Today, Inbox, Upcoming, Anytime, Someday, Logbook),
/// projects, areas, and utility sections (Habits, Timer, Settings).
struct SidebarView: View {
    @Environment(AppState.self) private var appState
    @Binding var selection: SidebarItem

    @State private var projects: [ProjectItem] = []
    @State private var areas: [AreaItem] = []

    // Wrap non-optional binding for List(selection:) which expects Optional
    private var optionalSelection: Binding<SidebarItem?> {
        Binding<SidebarItem?>(
            get: { selection },
            set: { if let newValue = $0 { selection = newValue } }
        )
    }

    var body: some View {
        List(selection: optionalSelection) {
            Section("Views") {
                ForEach(smartViews, id: \.self) { item in
                    sidebarLabel(item)
                        .tag(item)
                }
            }

            if !projects.isEmpty {
                Section("Projects") {
                    ForEach(projects) { project in
                        let item = SidebarItem.project(id: project.id)
                        Label(project.name, systemImage: "folder.fill")
                            .tag(item)
                            .dropDestination(for: TaskTransferable.self) { items, _ in
                                handleTaskDrop(items, onto: item)
                            }
                    }
                }
            }

            if !areas.isEmpty {
                Section("Areas") {
                    ForEach(areas) { area in
                        let item = SidebarItem.area(id: area.id)
                        Label(area.name, systemImage: "rectangle.stack.fill")
                            .tag(item)
                    }
                }
            }

            Section {
                sidebarLabel(.habits).tag(SidebarItem.habits)
                sidebarLabel(.timer).tag(SidebarItem.timer)
            }

            Section {
                sidebarLabel(.settings).tag(SidebarItem.settings)
            }
        }
        .navigationTitle("tock")
        .task {
            await loadSidebarData()
        }
    }

    // MARK: - Helpers

    private var smartViews: [SidebarItem] {
        [.today, .inbox, .upcoming, .anytime, .someday, .logbook]
    }

    @ViewBuilder
    private func sidebarLabel(_ item: SidebarItem) -> some View {
        Label(item.title, systemImage: item.icon)
    }

    private func loadSidebarData() async {
        projects = (try? await appState.client.listProjects()
            .filter { $0.status == .active || $0.status == .paused }) ?? []
        areas = (try? await appState.client.listAreas()) ?? []
    }

    private func handleTaskDrop(_ items: [TaskTransferable], onto target: SidebarItem) -> Bool {
        guard !items.isEmpty, case .project(let projectId) = target else {
            return false
        }
        for item in items {
            Task {
                try? await appState.client.modifyTask(
                    id: item.taskId,
                    projectId: projectId
                )
            }
        }
        return true
    }
}
