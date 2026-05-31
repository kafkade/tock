import SwiftUI

/// Root navigation view — adaptive layout.
///
/// **Compact** (iPhone / Slide Over): Tab bar with five tabs.
/// **Regular** (iPad / Stage Manager): Three-column `NavigationSplitView`
/// with sidebar, content list, and detail pane per architecture §8.2.
struct ContentView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.horizontalSizeClass) private var sizeClass

    // iPhone-only tab selection (independent from iPad sidebar)
    @State private var selectedTab: Tab = .today

    var body: some View {
        @Bindable var appState = appState

        Group {
            if sizeClass == .regular {
                iPadLayout
            } else {
                iPhoneLayout
            }
        }
        .sheet(isPresented: $appState.showQuickAdd) {
            QuickAddSheet()
        }
        // Clear task selection when switching to non-task sidebar items
        .onChange(of: appState.selectedSidebarItem) { _, newValue in
            if newValue.taskFilter == nil {
                appState.selectedTaskId = nil
            }
        }
        // Publish focused values for keyboard shortcut routing
        .focusedValue(\.sidebarItem, $appState.selectedSidebarItem)
        .focusedValue(\.selectedTask, $appState.selectedTaskId)
        .focusedValue(\.quickAddAction) { appState.showQuickAdd = true }
        .focusedValue(
            \.completeTaskAction,
            appState.selectedTaskId == nil ? nil : {
                guard let taskId = appState.selectedTaskId else { return }
                Task { try? await appState.client.completeTask(id: taskId) }
            }
        )
        .focusedValue(
            \.toggleEveningAction,
            appState.selectedTaskId == nil ? nil : {
                // Evening toggle requires modifyTask extension; stubbed for now
            }
        )
        .focusedValue(
            \.toggleTimerAction,
            appState.selectedTaskId == nil ? nil : {
                guard let taskId = appState.selectedTaskId else { return }
                Task {
                    if let _ = try? await appState.client.currentTimer() {
                        _ = try? await appState.client.stopTimer()
                    } else {
                        _ = try? await appState.client.startTimer(title: "Task", taskId: taskId)
                    }
                }
            }
        )
        .focusedValue(\.startFocusAction) {
            Task { _ = try? await appState.client.startFocus(taskId: appState.selectedTaskId, cycles: 4) }
        }
    }

    // MARK: - iPad three-column layout

    @ViewBuilder
    private var iPadLayout: some View {
        @Bindable var appState = appState

        NavigationSplitView {
            SidebarView(selection: $appState.selectedSidebarItem)
        } content: {
            iPadContentColumn
                .toolbar {
                    ToolbarItem(placement: .primaryAction) {
                        Button {
                            appState.showQuickAdd = true
                        } label: {
                            Image(systemName: "plus.circle.fill")
                                .font(.title3)
                        }
                        .keyboardShortcut("n", modifiers: .command)
                    }
                }
        } detail: {
            iPadDetailColumn
        }
    }

    @ViewBuilder
    private var iPadContentColumn: some View {
        @Bindable var appState = appState

        switch appState.selectedSidebarItem {
        case .habits:
            HabitsView()
        case .timer:
            TimerView()
        case .settings:
            SettingsView()
        case .area:
            ContentUnavailableView(
                "Area",
                systemImage: "rectangle.stack.fill",
                description: Text("Select a project or view to see tasks.")
            )
        default:
            if let filter = appState.selectedSidebarItem.taskFilter {
                TaskListView(filter: filter, selectedTaskId: $appState.selectedTaskId)
            }
        }
    }

    @ViewBuilder
    private var iPadDetailColumn: some View {
        if let taskId = appState.selectedTaskId {
            TaskDetailLoadingView(taskId: taskId)
        } else {
            ContentUnavailableView(
                "No Selection",
                systemImage: "sidebar.right",
                description: Text("Select a task to see its details.")
            )
        }
    }

    // MARK: - iPhone tab layout

    @ViewBuilder
    private var iPhoneLayout: some View {
        @Bindable var appState = appState

        TabView(selection: $selectedTab) {
            ForEach(Tab.allCases, id: \.self) { tab in
                NavigationStack {
                    tabContent(for: tab)
                        .toolbar {
                            ToolbarItem(placement: .primaryAction) {
                                Button {
                                    appState.showQuickAdd = true
                                } label: {
                                    Image(systemName: "plus.circle.fill")
                                        .font(.title3)
                                }
                            }
                            ToolbarItem(placement: .navigationBarLeading) {
                                NavigationLink {
                                    SettingsView()
                                } label: {
                                    Image(systemName: "gear")
                                }
                            }
                        }
                }
                .tabItem {
                    Label(tab.rawValue, systemImage: tab.icon)
                }
                .tag(tab)
            }
        }
    }

    @ViewBuilder
    private func tabContent(for tab: Tab) -> some View {
        switch tab {
        case .today: TodayView()
        case .inbox: InboxView()
        case .projects: ProjectsView()
        case .habits: HabitsView()
        case .timer: TimerView()
        }
    }
}

// MARK: - iPhone tab definition

enum Tab: String, CaseIterable {
    case today = "Today"
    case inbox = "Inbox"
    case projects = "Projects"
    case habits = "Habits"
    case timer = "Timer"

    var icon: String {
        switch self {
        case .today: "sun.max.fill"
        case .inbox: "tray.fill"
        case .projects: "folder.fill"
        case .habits: "flame.fill"
        case .timer: "timer"
        }
    }
}

// MARK: - Detail loading wrapper

/// Loads and displays task detail for the iPad detail column.
///
/// Uses `getTask(id:)` from CoreClient for lookup. Displays inline in
/// the NavigationSplitView detail column (no NavigationStack push).
private struct TaskDetailLoadingView: View {
    @Environment(AppState.self) private var appState
    let taskId: String

    @State private var task: TaskItem?

    var body: some View {
        Group {
            if let task {
                TaskDetailView(task: task)
            } else {
                ProgressView()
            }
        }
        .task(id: taskId) {
            task = nil
            task = try? await appState.client.getTask(id: taskId)
        }
    }
}
