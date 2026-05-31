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
    }
}
