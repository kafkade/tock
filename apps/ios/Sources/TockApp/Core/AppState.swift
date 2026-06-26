import SwiftUI
import TockSwift

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
    /// vault is unlocked; locked sentinel until then.
    var client: any CoreClient = LockedCoreClient()

    /// Master password held transiently in memory while the vault is unlocked,
    /// so the user can opt in to biometric unlock (which caches it in the
    /// Keychain) after unlocking. Cleared on `lock()`.
    private var sessionPassword: String?

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
        do {
            let workspace = try await TockWorkspace.open(
                path: path, password: Data(password.utf8)
            )
            client = TockCoreClient(workspace: workspace)
            sessionPassword = password
            vaultStatus = .unlocked
            didExplicitlyLock = false
            didAttemptAutoUnlock = false
            await WidgetSnapshotWriter.publish(from: client)
            PhoneSessionManager.shared.setClient(client)
            PhoneSessionManager.shared.pushSnapshot()
        } catch {
            vaultStatus = .error(Self.describe(error))
        }
    }

    func createVault(path: String, password: String) async {
        vaultStatus = .unlocking
        do {
            let workspace = try await TockWorkspace.create(
                path: path, password: Data(password.utf8)
            )
            client = TockCoreClient(workspace: workspace)
            sessionPassword = password
            vaultStatus = .unlocked
            didExplicitlyLock = false
            didAttemptAutoUnlock = false
            await WidgetSnapshotWriter.publish(from: client)
            PhoneSessionManager.shared.setClient(client)
            PhoneSessionManager.shared.pushSnapshot()
        } catch {
            vaultStatus = .error(Self.describe(error))
        }
    }

    func lock() {
        if let live = client as? TockCoreClient {
            Task { try? await live.lock() }
        }
        client = LockedCoreClient()
        sessionPassword = nil
        vaultStatus = .locked
        selectedSidebarItem = .today
        selectedTaskId = nil
        selectedTaskIds = []
        didExplicitlyLock = true
        didAttemptAutoUnlock = false
        WidgetSnapshotWriter.publishLocked()
        PhoneSessionManager.shared.setClient(nil)
        PhoneSessionManager.shared.pushSnapshot()
    }

    /// Human-readable description for a vault error, mapping the common
    /// `TockError` cases to friendly text.
    private static func describe(_ error: Error) -> String {
        if let tockError = error as? TockError {
            switch tockError {
            case .InvalidCredentials:
                return "Incorrect master password."
            case .VaultNotFound:
                return "No vault found. Create one to get started."
            default:
                return "Could not open the vault. Please try again."
            }
        }
        return error.localizedDescription
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
            let password = try KeychainService.loadMasterPassword(reason: reason)
            let workspace = try await TockWorkspace.open(
                path: AppGroup.vaultPath(), password: Data(password.utf8)
            )
            client = TockCoreClient(workspace: workspace)
            sessionPassword = password
            vaultStatus = .unlocked
            didExplicitlyLock = false
            didAttemptAutoUnlock = false
            await WidgetSnapshotWriter.publish(from: client)
            PhoneSessionManager.shared.setClient(client)
            PhoneSessionManager.shared.pushSnapshot()
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
            vaultStatus = .error("Biometric unlock failed: \(Self.describe(error))")
        }
    }

    // MARK: - Biometric enable / disable

    /// Enable biometric unlock by caching the master password in the Keychain.
    ///
    /// Must be called while the vault is unlocked — the password captured at
    /// unlock time is stored under biometric protection so it can be passed to
    /// `open_workspace` on future biometric unlocks.
    func enableBiometrics() throws {
        guard case .unlocked = vaultStatus, let password = sessionPassword else { return }
        try KeychainService.saveMasterPassword(password)
        biometricEnabled = true
    }

    /// Disable biometric unlock and remove the cached password.
    func disableBiometrics() {
        KeychainService.deleteVaultKey()
        biometricEnabled = false
    }
}
