import SwiftUI

/// Tock — unified productivity engine for iOS.
///
/// The app entry point. Sets up the `AppState` environment and gates
/// on vault status: locked → `VaultSetupView`, unlocked → `ContentView`.
@main
struct TockApp: App {

    @State private var appState = AppState()

    var body: some Scene {
        WindowGroup {
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
}
