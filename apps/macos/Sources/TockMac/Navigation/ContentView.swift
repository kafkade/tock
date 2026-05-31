import SwiftUI

/// Root navigation view — macOS three-column layout.
///
/// Always uses `NavigationSplitView` with sidebar, content list, and detail pane.
/// No compact/tab layout — macOS always has enough horizontal space.
struct ContentView: View {
    @Environment(AppSessionState.self) private var appState

    @SceneStorage("selectedSidebarItem") private var selectedSidebarRaw: String = "today"
    @State private var selectedSidebarItem: SidebarItem = .today
    @State private var selectedTaskId: String?
    @State private var searchText = ""

    var body: some View {
        NavigationSplitView {
            SidebarView(selection: $selectedSidebarItem)
        } content: {
            contentColumn
                .searchable(text: $searchText, placement: .toolbar, prompt: "Search tasks…")
                .toolbar {
                    ToolbarItemGroup(placement: .primaryAction) {
                        Button {
                            appState.showQuickAdd = true
                        } label: {
                            Image(systemName: "plus.circle.fill")
                                .font(.title3)
                        }
                        .keyboardShortcut("n", modifiers: .command)
                        .help("New Task (⌘N)")
                    }
                }
        } detail: {
            detailColumn
        }
        .navigationSplitViewStyle(.balanced)
        .sheet(isPresented: Binding(
            get: { appState.showQuickAdd },
            set: { appState.showQuickAdd = $0 }
        )) {
            QuickAddSheet()
        }
        // Sync sidebar selection with SceneStorage for restoration
        .onChange(of: selectedSidebarItem) { _, newValue in
            selectedSidebarRaw = sidebarItemToStorageKey(newValue)
            if newValue.taskFilter == nil {
                selectedTaskId = nil
            }
        }
        .onAppear {
            selectedSidebarItem = storageKeyToSidebarItem(selectedSidebarRaw)
        }
        // Publish focused values for keyboard shortcut routing
        .focusedValue(\.sidebarItem, $selectedSidebarItem)
        .focusedValue(\.selectedTask, $selectedTaskId)
        .focusedValue(\.quickAddAction) { appState.showQuickAdd = true }
        .focusedValue(\.lockVaultAction) { appState.lock() }
        .focusedValue(
            \.completeTaskAction,
            selectedTaskId == nil ? nil : {
                guard let taskId = selectedTaskId else { return }
                Task {
                    try? await appState.client.completeTask(id: taskId)
                    await appState.refreshMenuBarState()
                }
            }
        )
        .focusedValue(
            \.toggleEveningAction,
            selectedTaskId == nil ? nil : {
                // Evening toggle requires modifyTask extension; stubbed for now
            }
        )
        .focusedValue(
            \.toggleTimerAction,
            selectedTaskId == nil ? nil : {
                guard let taskId = selectedTaskId else { return }
                Task {
                    if let _ = try? await appState.client.currentTimer() {
                        _ = try? await appState.client.stopTimer()
                    } else {
                        _ = try? await appState.client.startTimer(title: "Task", taskId: taskId)
                    }
                    await appState.refreshMenuBarState()
                }
            }
        )
        .focusedValue(\.startFocusAction) {
            Task {
                _ = try? await appState.client.startFocus(taskId: selectedTaskId, cycles: 4)
                await appState.refreshMenuBarState()
            }
        }
    }

    // MARK: - Content column

    @ViewBuilder
    private var contentColumn: some View {
        switch selectedSidebarItem {
        case .habits:
            HabitsView()
        case .timer:
            TimerView()
        case .settings:
            // On macOS, settings are in the Settings scene; redirect to Today
            TodayView()
        case .area:
            ContentUnavailableView(
                "Area",
                systemImage: "rectangle.stack.fill",
                description: Text("Select a project or view to see tasks.")
            )
        default:
            if let filter = selectedSidebarItem.taskFilter {
                TaskListView(filter: filter, selectedTaskId: $selectedTaskId)
            }
        }
    }

    // MARK: - Detail column

    @ViewBuilder
    private var detailColumn: some View {
        if let taskId = selectedTaskId {
            TaskDetailLoadingView(taskId: taskId)
        } else {
            ContentUnavailableView(
                "No Selection",
                systemImage: "sidebar.right",
                description: Text("Select a task to see its details.")
            )
        }
    }

    // MARK: - SceneStorage helpers

    private func sidebarItemToStorageKey(_ item: SidebarItem) -> String {
        switch item {
        case .today: "today"
        case .inbox: "inbox"
        case .upcoming: "upcoming"
        case .anytime: "anytime"
        case .someday: "someday"
        case .logbook: "logbook"
        case .habits: "habits"
        case .timer: "timer"
        case .settings: "settings"
        case .project(let id): "project:\(id)"
        case .area(let id): "area:\(id)"
        }
    }

    private func storageKeyToSidebarItem(_ key: String) -> SidebarItem {
        switch key {
        case "today": .today
        case "inbox": .inbox
        case "upcoming": .upcoming
        case "anytime": .anytime
        case "someday": .someday
        case "logbook": .logbook
        case "habits": .habits
        case "timer": .timer
        case "settings": .settings
        default:
            if key.hasPrefix("project:") {
                .project(id: String(key.dropFirst("project:".count)))
            } else if key.hasPrefix("area:") {
                .area(id: String(key.dropFirst("area:".count)))
            } else {
                .today
            }
        }
    }
}

// MARK: - Detail loading wrapper

/// Loads and displays task detail for the detail column.
private struct TaskDetailLoadingView: View {
    @Environment(AppSessionState.self) private var appState
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
