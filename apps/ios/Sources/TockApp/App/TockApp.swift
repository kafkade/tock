import SwiftUI

/// Tock — unified productivity engine for iOS and iPadOS.
///
/// The app entry point. Each `WindowGroup` window gets its own `AppState`
/// for proper Stage Manager multi-window support. Gates on vault status:
/// locked → `VaultSetupView`, unlocked → `ContentView`.
@main
struct TockApp: App {

    var body: some Scene {
        WindowGroup {
            RootSceneView()
        }
        .commands {
            TockCommands()
        }
    }
}

/// Per-window root view that owns its own `AppState`.
///
/// Each Stage Manager window creates an independent `AppState` instance,
/// ensuring sidebar selection and task state don't bleed across windows.
/// Handles `tock://` deep-link URLs from widgets and App Intents.
private struct RootSceneView: View {
    @State private var appState = AppState()

    var body: some View {
        Group {
            switch appState.vaultStatus {
            case .locked, .unlocking, .error:
                VaultSetupView()
            case .unlocked:
                ContentView()
            }
        }
        .environment(appState)
        .onOpenURL { url in
            handleDeepLink(url)
        }
        .task {
            await drainPendingCaptures()
        }
    }

    private func handleDeepLink(_ url: URL) {
        guard url.scheme == "tock" else { return }
        let host = url.host() ?? ""

        switch host {
        case "today":
            appState.selectedSidebarItem = .today
        case "inbox":
            appState.selectedSidebarItem = .inbox
        case "upcoming":
            appState.selectedSidebarItem = .upcoming
        case "anytime":
            appState.selectedSidebarItem = .anytime
        case "someday":
            appState.selectedSidebarItem = .someday
        case "logbook":
            appState.selectedSidebarItem = .logbook
        case "habits":
            appState.selectedSidebarItem = .habits
        case "timer":
            appState.selectedSidebarItem = .timer
        case "task":
            let taskId = url.pathComponents.dropFirst().first
            if let taskId {
                appState.selectedSidebarItem = .today
                appState.selectedTaskId = taskId
            }
        case "habit":
            appState.selectedSidebarItem = .habits
        default:
            break
        }
    }

    /// Drain captures saved by the share extension and create tasks.
    private func drainPendingCaptures() async {
        guard case .unlocked = appState.vaultStatus else { return }
        let captures = SharePendingStore.shared.drainAll()
        for capture in captures {
            let input = capture.toNewTaskInput()
            _ = try? await appState.client.addTask(input)
        }
    }
}
