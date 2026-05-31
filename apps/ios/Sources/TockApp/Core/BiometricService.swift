import LocalAuthentication

/// Lightweight wrapper around LAContext for checking biometric availability.
///
/// This service only checks device capabilities — actual biometric authentication
/// happens through `KeychainService.loadVaultKey(reason:)`, which uses the
/// Keychain's built-in biometric gate as the security boundary.
enum BiometricService {

    /// The type of biometric authentication available on this device.
    enum BiometricType: Sendable {
        case faceID
        case touchID
        case none

        /// User-facing label for the biometric type.
        var label: String {
            switch self {
            case .faceID: "Face ID"
            case .touchID: "Touch ID"
            case .none: "Biometrics"
            }
        }

        /// SF Symbol name for the biometric type.
        var systemImage: String {
            switch self {
            case .faceID: "faceid"
            case .touchID: "touchid"
            case .none: "lock.shield"
            }
        }
    }

    /// Whether biometric authentication is available on this device.
    ///
    /// Returns `false` if biometrics are not enrolled, hardware is unavailable,
    /// or the device passcode is not set.
    static func isAvailable() -> Bool {
        let context = LAContext()
        var error: NSError?
        return context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &error)
    }

    /// The type of biometric hardware available.
    static func currentType() -> BiometricType {
        let context = LAContext()
        var error: NSError?
        guard context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &error) else {
            return .none
        }
        switch context.biometryType {
        case .faceID:
            return .faceID
        case .touchID:
            return .touchID
        case .opticID:
            return .none
        @unknown default:
            return .none
        }
    }
}
