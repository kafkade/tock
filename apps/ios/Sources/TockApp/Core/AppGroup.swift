import Foundation

/// Shared App Group container used by the main app, widgets, the Share
/// Extension, and App Intents so they all read and write **one** vault file.
///
/// The identifier must match the `com.apple.security.application-groups`
/// entitlement configured on every target in the Xcode project. When the
/// entitlement is missing (e.g. running the SwiftPM package directly during
/// development) the helpers fall back to the app's Documents directory so the
/// app still works as a single-process vault.
enum AppGroup {

    /// App Group identifier. Keep in sync with each target's entitlements.
    static let identifier = "group.com.kafkade.tock"

    /// The shared container URL, or `nil` when the entitlement is unavailable.
    static func containerURL() -> URL? {
        FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: identifier
        )
    }

    /// `UserDefaults` suite shared across the App Group, or `.standard`
    /// when the entitlement is unavailable.
    static var defaults: UserDefaults {
        UserDefaults(suiteName: identifier) ?? .standard
    }

    /// Absolute path to the shared vault file.
    ///
    /// Prefers the App Group container so widgets and the Share Extension
    /// open the same vault as the main app. Falls back to Documents.
    static func vaultPath() -> String {
        let directory = containerURL()
            ?? FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first!
        return directory.appendingPathComponent("tock.vault").path
    }
}
