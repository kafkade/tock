import SwiftUI

/// Tock — macOS native app.
///
/// Three scenes per architecture §8.3:
/// 1. **WindowGroup**: Full window with NavigationSplitView (same 3-column as iPad).
/// 2. **MenuBarExtra**: Always-on menu bar item with compact popover.
/// 3. **Settings**: macOS native settings window (⌘,).
///
/// Global hotkey (⌃⌥Space) is registered via `QuickEntryPanelController`
/// using the Carbon `RegisterEventHotKey` API.
@main
struct TockMacApp: App {
    @State private var appState = AppSessionState()
    @State private var quickEntryController: QuickEntryPanelController?

    var body: some Scene {
        // MARK: - Main Window

        WindowGroup {
            RootSceneView()
                .environment(appState)
                .focusedValue(\.quickEntryAction) { [weak quickEntryController] in
                    quickEntryController?.togglePanel()
                }
                .onAppear {
                    setupQuickEntry()
                }
        }
        .defaultSize(width: 1100, height: 720)
        .commands {
            TockCommands()
        }

        // MARK: - Menu Bar

        MenuBarExtra {
            MenuBarView()
                .environment(appState)
        } label: {
            Label(appState.menuBarTitle, systemImage: appState.menuBarIcon)
        }
        .menuBarExtraStyle(.window)

        // MARK: - Settings

        Settings {
            SettingsView()
                .environment(appState)
        }
    }

    // MARK: - Quick Entry Setup

    private func setupQuickEntry() {
        guard quickEntryController == nil else { return }
        let controller = QuickEntryPanelController(appState: appState)
        controller.registerHotKey()
        quickEntryController = controller
    }
}

// MARK: - Root Scene View

/// Per-window root view. Gates on vault status: locked → `VaultSetupView`,
/// unlocked → `ContentView`.
///
/// Uses `@SceneStorage` for per-window state restoration. Each window in
/// the WindowGroup creates its own independent navigation state.
private struct RootSceneView: View {
    @Environment(AppSessionState.self) private var appState

    var body: some View {
        Group {
            switch appState.vaultStatus {
            case .locked, .unlocking, .error:
                VaultSetupView()
            case .unlocked:
                ContentView()
            }
        }
        .onOpenURL { url in
            handleDeepLink(url)
        }
    }

    private func handleDeepLink(_ url: URL) {
        guard url.scheme == "tock" else { return }
        // Deep link handling — navigate to the appropriate view.
        // Handled at the ContentView level via sidebar selection.
    }
}
