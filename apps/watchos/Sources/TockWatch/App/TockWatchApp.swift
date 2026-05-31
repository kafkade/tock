import SwiftUI

/// Tock watchOS companion app.
///
/// Three-tab layout: Today tasks, Habits, and Timer/Focus.
/// Gates on vault status — shows connection prompt when the
/// paired iPhone has not unlocked the vault.
@main
struct TockWatchApp: App {

    @State private var appState = WatchAppState()

    var body: some Scene {
        WindowGroup {
            Group {
                switch appState.vaultStatus {
                case .unlocked:
                    mainTabView
                case .locked:
                    lockedView
                case .unknown:
                    connectingView
                }
            }
            .environment(appState)
        }
    }

    @ViewBuilder
    private var mainTabView: some View {
        TabView(selection: Binding(
            get: { appState.selectedTab },
            set: { appState.selectedTab = $0 }
        )) {
            NavigationStack {
                TodayView()
            }
            .tag(WatchAppState.Tab.today)
            .tabItem {
                Label("Today", systemImage: "sun.max.fill")
            }

            NavigationStack {
                HabitsView()
            }
            .tag(WatchAppState.Tab.habits)
            .tabItem {
                Label("Habits", systemImage: "flame.fill")
            }

            NavigationStack {
                TimerFocusView()
            }
            .tag(WatchAppState.Tab.timer)
            .tabItem {
                Label("Timer", systemImage: "timer")
            }
        }
    }

    @ViewBuilder
    private var lockedView: some View {
        VStack(spacing: WatchTheme.Spacing.xl) {
            Image(systemName: "lock.fill")
                .font(.title)
                .foregroundStyle(.secondary)

            Text("Vault Locked")
                .font(.headline)

            Text("Unlock on iPhone")
                .font(.caption2)
                .foregroundStyle(.secondary)

            connectionBadge
        }
    }

    @ViewBuilder
    private var connectingView: some View {
        VStack(spacing: WatchTheme.Spacing.xl) {
            ProgressView()

            Text("Connecting…")
                .font(.headline)

            Text("Open tock on iPhone")
                .font(.caption2)
                .foregroundStyle(.secondary)

            connectionBadge
        }
    }

    @ViewBuilder
    private var connectionBadge: some View {
        HStack(spacing: WatchTheme.Spacing.sm) {
            Circle()
                .fill(connectionColor)
                .frame(width: 6, height: 6)
            Text(appState.connectionLabel)
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }

    private var connectionColor: Color {
        switch appState.connectionStatus {
        case .connected: .green
        case .disconnected: .red
        case .stale: .orange
        }
    }
}
