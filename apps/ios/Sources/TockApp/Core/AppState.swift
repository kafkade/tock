import SwiftUI

/// App-wide observable state.
///
/// Holds vault lifecycle status, navigation state, biometric preferences,
/// and the active `CoreClient` reference. Each window in Stage Manager gets
/// its own `AppState` instance (created inside `WindowGroup` content).
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

    // MARK: - iPad navigation state

    /// Currently selected sidebar item (iPad NavigationSplitView).
    var selectedSidebarItem: SidebarItem = .today

    /// Currently selected task ID in the content column (drives detail column).
    var selectedTaskId: String?

    /// Set of selected task IDs for multi-select operations (iPad).
    var selectedTaskIds: Set<String> = []

    // MARK: - Biometric state

    /// Whether the user has opted in to biometric unlock. Persisted in UserDefaults.
    var biometricEnabled: Bool {
        get { UserDefaults.standard.bool(forKey: "biometricEnabled") }
        set { UserDefaults.standard.set(newValue, forKey: "biometricEnabled") }
    }

    /// Whether biometric authentication is available on this device.
    var biometricAvailable: Bool { BiometricService.isAvailable() }

    /// The type of biometric hardware (Face ID, Touch ID, or none).
    var biometricType: BiometricService.BiometricType { BiometricService.currentType() }

    /// Whether a biometric-cached vault key exists in the Keychain.
    var hasCachedBiometricKey: Bool { KeychainService.hasCachedKey() }

    /// Whether biometric unlock should be offered on the lock screen.
    var canOfferBiometricUnlock: Bool {
        biometricEnabled && biometricAvailable && hasCachedBiometricKey && !didExplicitlyLock
    }

    /// Set when the user manually locks the vault. Prevents auto-triggering
    /// biometric unlock until the next app launch or password unlock.
    var didExplicitlyLock = false

    /// Tracks whether biometric auto-trigger has already been attempted
    /// for this lock-screen appearance. Prevents repeated prompts.
    var didAttemptAutoUnlock = false

    // MARK: - Initializer

    init() {
        // Validate install ID — clears stale Keychain items after reinstall
        KeychainService.validateInstallId()

        // Reconcile: if Keychain item was invalidated by OS (biometric change),
        // disable the preference to avoid showing a broken biometric button.
        if biometricEnabled && !KeychainService.hasCachedKey() {
            biometricEnabled = false
        }
    }

    // MARK: - Vault lifecycle

    func unlock(path: String, password: String) async {
        vaultStatus = .unlocking
        // TODO: Replace with actual vault open via CoreActor when
        // UniFFI bindings are connected.
        // For now, simulate a brief unlock delay.
        try? await Task.sleep(for: .milliseconds(500))
        vaultStatus = .unlocked
        didExplicitlyLock = false
        didAttemptAutoUnlock = false
    }

    func createVault(path: String, password: String) async {
        vaultStatus = .unlocking
        try? await Task.sleep(for: .milliseconds(500))
        vaultStatus = .unlocked
        didExplicitlyLock = false
        didAttemptAutoUnlock = false
    }

    func lock() {
        vaultStatus = .locked
        selectedSidebarItem = .today
        selectedTaskId = nil
        selectedTaskIds = []
        didExplicitlyLock = true
        didAttemptAutoUnlock = false
    }

    // MARK: - Biometric unlock

    /// Unlock the vault using biometric authentication.
    ///
    /// The biometric prompt is triggered by the Keychain access itself —
    /// `SecItemCopyMatching` presents Face ID / Touch ID when the item
    /// has `.biometryCurrentSet` access control.
    func unlockWithBiometrics() async {
        vaultStatus = .unlocking
        do {
            let reason = "Unlock your tock vault"
            let _keyData = try KeychainService.loadVaultKey(reason: reason)
            // TODO: Pass keyData to CoreActor.unlockWithCachedKey(_:) when
            // UniFFI bindings are connected. For now, mock unlock succeeds
            // if the Keychain read succeeded (biometric auth passed).
            _ = _keyData
            try? await Task.sleep(for: .milliseconds(200))
            vaultStatus = .unlocked
            didExplicitlyLock = false
            didAttemptAutoUnlock = false
        } catch let error as KeychainError {
            switch error {
            case .userCancelled:
                // Don't show error — user intentionally cancelled
                vaultStatus = .locked
            case .itemNotFound:
                // Keychain item was invalidated (biometric change / reinstall)
                biometricEnabled = false
                vaultStatus = .error(
                    "\(biometricType.label) unlock was reset. "
                    + "Enter your master password to re-enable it."
                )
            case .authenticationFailed:
                vaultStatus = .error("\(biometricType.label) authentication failed. Try again or use your password.")
            default:
                vaultStatus = .error(error.localizedDescription ?? "Biometric unlock failed.")
            }
        } catch {
            vaultStatus = .error("Biometric unlock failed: \(error.localizedDescription)")
        }
    }

    // MARK: - Biometric enable / disable

    /// Enable biometric unlock by caching the vault key in the Keychain.
    ///
    /// Must be called while the vault is unlocked. In production, the key
    /// would come from CoreActor. Currently stores a mock placeholder.
    func enableBiometrics() throws {
        guard vaultStatus == .unlocked else { return }

        // TODO: In production, get the real vault key from CoreActor:
        //   let key = await CoreActor.shared.exportCachedKey()
        // For development, store a deterministic mock key.
        let mockKey = Data("tock-mock-vault-key-v1".utf8)
        try KeychainService.saveVaultKey(mockKey)
        biometricEnabled = true
    }

    /// Disable biometric unlock and remove the cached key.
    func disableBiometrics() {
        KeychainService.deleteVaultKey()
        biometricEnabled = false
    }
}
