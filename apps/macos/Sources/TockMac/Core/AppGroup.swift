import Foundation

/// Shared App Group container for the macOS app and its extensions (menu bar
/// helper, future widgets) so they read and write **one** vault file.
///
/// The identifier must match the `com.apple.security.application-groups`
/// entitlement. When the entitlement is unavailable (e.g. running the SwiftPM
/// executable directly during development) the helper falls back to
/// `~/Library/Application Support/tock`.
enum AppGroup {

    /// App Group identifier. Keep in sync with each target's entitlements.
    /// macOS App Groups are conventionally prefixed with the team ID; adjust in
    /// the Xcode project as needed.
    static let identifier = "group.com.kafkade.tock"

    /// The shared container URL, or `nil` when the entitlement is unavailable.
    static func containerURL() -> URL? {
        FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: identifier
        )
    }

    /// Absolute path to the shared vault file.
    static func vaultPath() -> String {
        let directory: URL
        if let container = containerURL() {
            directory = container
        } else {
            let appSupport = FileManager.default.urls(
                for: .applicationSupportDirectory, in: .userDomainMask
            ).first!
            directory = appSupport.appendingPathComponent("tock")
        }
        try? FileManager.default.createDirectory(
            at: directory, withIntermediateDirectories: true
        )
        return directory.appendingPathComponent("vault.tock").path
    }
}
