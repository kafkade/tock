import SwiftUI

/// Root navigation view with a tab bar.
///
/// Five tabs: Today, Inbox, Projects, Habits, Timer.
/// Settings accessible from toolbar. Quick-add sheet presented at this level.
struct ContentView: View {
    @Environment(AppState.self) private var appState
    @State private var selectedTab: Tab = .today

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

    var body: some View {
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
        .sheet(isPresented: $appState.showQuickAdd) {
            QuickAddSheet()
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
