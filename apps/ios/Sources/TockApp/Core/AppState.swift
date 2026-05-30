import SwiftUI

/// App-wide observable state.
///
/// Holds vault lifecycle status and the active `CoreClient` reference.
/// Does NOT hold secrets — the vault key lives only inside `CoreActor`.
@Observable
@MainActor
final class AppState {

    enum VaultStatus: Sendable {
        case locked
        case unlocking
        case unlocked
        case error(String)
    }

    var vaultStatus: VaultStatus = .locked
    var showQuickAdd = false

    /// The active core client. Replaced with a live client when the
    /// vault is unlocked; defaults to mock for development.
    var client: any CoreClient = MockCoreClient.shared

    func unlock(path: String, password: String) async {
        vaultStatus = .unlocking
        // TODO: Replace with actual vault open via CoreActor when
        // UniFFI bindings are connected.
        // For now, simulate a brief unlock delay.
        try? await Task.sleep(for: .milliseconds(500))
        vaultStatus = .unlocked
    }

    func createVault(path: String, password: String) async {
        vaultStatus = .unlocking
        try? await Task.sleep(for: .milliseconds(500))
        vaultStatus = .unlocked
    }

    func lock() {
        vaultStatus = .locked
    }
}
