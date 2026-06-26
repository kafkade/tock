import Foundation
import Security
import LocalAuthentication

/// Keychain wrapper for storing the vault unlock key with biometric protection.
///
/// The Keychain item is protected with `.biometryCurrentSet` access control,
/// meaning it is automatically invalidated when:
/// - Face ID / Touch ID enrollment changes (new face/fingerprint added/removed)
/// - All biometrics are removed
/// - Device passcode is removed
///
/// The item uses `kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly` so it:
/// - Requires a device passcode to exist
/// - Cannot be restored to another device via backup
/// - Cannot survive device migration
///
/// Biometric authentication happens implicitly when reading the item —
/// `SecItemCopyMatching` triggers the system biometric prompt. This makes
/// the Keychain access itself the security boundary, not a separate LAContext.
enum KeychainService {

    // MARK: - Constants

    private static let service = "com.kafkade.tock.vault"
    private static let account = "vault-master-key"
    private static let installIdKey = "com.kafkade.tock.installId"

    // MARK: - Vault key operations

    /// Store the vault key in the Keychain with biometric access control.
    ///
    /// - Parameter key: The vault key data to cache. In production, this is
    ///   the derived key from CoreActor. Currently stores a mock placeholder.
    /// - Throws: `KeychainError` if the operation fails.
    static func saveVaultKey(_ key: Data) throws {
        // Delete any existing item first
        deleteVaultKey()

        var error: Unmanaged<CFError>?
        guard let accessControl = SecAccessControlCreateWithFlags(
            nil,
            kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly,
            .biometryCurrentSet,
            &error
        ) else {
            throw KeychainError.accessControlCreationFailed(
                error?.takeRetainedValue().localizedDescription ?? "Unknown error"
            )
        }

        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecValueData as String: key,
            kSecAttrAccessControl as String: accessControl,
        ]

        let status = SecItemAdd(query as CFDictionary, nil)
        guard status == errSecSuccess else {
            throw KeychainError.saveFailed(status)
        }
    }

    /// Load the vault key from the Keychain, triggering biometric authentication.
    ///
    /// The system presents a Face ID or Touch ID prompt with the given reason.
    /// This method blocks on biometric auth — call from an async context.
    ///
    /// - Parameter reason: Localized string shown in the biometric prompt.
    /// - Returns: The cached vault key data.
    /// - Throws: `KeychainError` describing why the load failed.
    static func loadVaultKey(reason: String) throws -> Data {
        let context = LAContext()
        context.localizedReason = reason

        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
            kSecUseAuthenticationContext as String: context,
            kSecUseOperationPrompt as String: reason,
        ]

        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)

        switch status {
        case errSecSuccess:
            guard let data = result as? Data else {
                throw KeychainError.unexpectedData
            }
            return data
        case errSecItemNotFound:
            throw KeychainError.itemNotFound
        case errSecUserCanceled:
            throw KeychainError.userCancelled
        case errSecAuthFailed:
            throw KeychainError.authenticationFailed
        case errSecInteractionNotAllowed:
            throw KeychainError.interactionNotAllowed
        default:
            throw KeychainError.loadFailed(status)
        }
    }

    /// Delete the cached vault key from the Keychain.
    @discardableResult
    static func deleteVaultKey() -> Bool {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]

        let status = SecItemDelete(query as CFDictionary)
        return status == errSecSuccess || status == errSecItemNotFound
    }

    /// Check whether a cached vault key exists without triggering biometric auth.
    static func hasCachedKey() -> Bool {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecUseAuthenticationUI as String: kSecUseAuthenticationUIFail,
        ]

        let status = SecItemCopyMatching(query as CFDictionary, nil)
        // errSecInteractionNotAllowed means the item exists but needs biometric
        return status == errSecSuccess || status == errSecInteractionNotAllowed
    }

    // MARK: - Master password convenience

    /// Cache the vault **master password** (UTF-8 bytes) under biometric
    /// protection. The core opens the vault from this password, so caching it
    /// is what lets Face ID / Touch ID open the real encrypted vault.
    static func saveMasterPassword(_ password: String) throws {
        try saveVaultKey(Data(password.utf8))
    }

    /// Load the cached master password, triggering biometric authentication.
    static func loadMasterPassword(reason: String) throws -> String {
        let data = try loadVaultKey(reason: reason)
        return String(decoding: data, as: UTF8.self)
    }

    // MARK: - Install ID (reinstall detection)

    /// Verify the install ID matches. If UserDefaults was cleared (reinstall),
    /// any stale Keychain items are deleted to force password re-entry.
    static func validateInstallId() {
        let defaults = UserDefaults.standard
        if defaults.string(forKey: installIdKey) == nil {
            // First launch or reinstall — clear any stale biometric keys
            deleteVaultKey()
            defaults.set(UUID().uuidString, forKey: installIdKey)
        }
    }
}

// MARK: - Errors

/// Keychain operation errors with distinct cases for clear user messaging.
enum KeychainError: LocalizedError {
    case accessControlCreationFailed(String)
    case saveFailed(OSStatus)
    case loadFailed(OSStatus)
    case itemNotFound
    case userCancelled
    case authenticationFailed
    case interactionNotAllowed
    case unexpectedData

    var errorDescription: String? {
        switch self {
        case .accessControlCreationFailed(let detail):
            "Could not create biometric access control: \(detail)"
        case .saveFailed(let status):
            "Could not save to Keychain (error \(status))"
        case .loadFailed(let status):
            "Could not read from Keychain (error \(status))"
        case .itemNotFound:
            "No cached vault key found. Please unlock with your master password."
        case .userCancelled:
            "Biometric authentication was cancelled."
        case .authenticationFailed:
            "Biometric authentication failed."
        case .interactionNotAllowed:
            "Biometric authentication is not available right now."
        case .unexpectedData:
            "Keychain returned unexpected data."
        }
    }
}
