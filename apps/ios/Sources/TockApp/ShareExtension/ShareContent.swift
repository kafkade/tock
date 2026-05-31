import Foundation

// MARK: - Share Source Kind

/// The type of content shared from the host app.
///
/// Preserved even when the attachment isn't processed yet,
/// so future versions can handle images, files, and locations.
enum ShareSourceKind: String, Codable, Sendable {
    case url
    case text
    case image
    case file
    case mail
    case map
    case unknown
}

// MARK: - Share Content

/// Extracted metadata from shared items, ready for the capture form.
///
/// Populated by `ShareItemExtractor`. The `title` and `notes` fields
/// are pre-filled from the shared content and editable by the user.
struct ShareContent: Sendable {
    var kind: ShareSourceKind
    var title: String
    var notes: String?
    var url: URL?
    var attachmentName: String?
    var attachmentTypeIdentifier: String?
    var sourceApp: String?

    /// Placeholder for when extraction hasn't completed yet.
    static let empty = ShareContent(kind: .unknown, title: "")
}

// MARK: - Quick Capture Destination

/// Where to file the captured task.
///
/// Maps to task lifecycle states. The share extension UI presents these
/// as a split-button menu: default is Inbox, long-press reveals others.
enum QuickCaptureDestination: String, Codable, Sendable, CaseIterable, Identifiable {
    case inbox
    case today
    case evening
    case someday

    var id: String { rawValue }

    var label: String {
        switch self {
        case .inbox: "Inbox"
        case .today: "Today"
        case .evening: "Evening"
        case .someday: "Someday"
        }
    }

    var icon: String {
        switch self {
        case .inbox: "tray"
        case .today: "sun.max"
        case .evening: "moon.stars"
        case .someday: "moon.zzz"
        }
    }

    /// The `TaskStatus` this destination maps to.
    var taskStatus: TaskStatus {
        switch self {
        case .inbox: .inbox
        case .today: .pending
        case .evening: .pending
        case .someday: .someday
        }
    }
}

// MARK: - Capture Availability

/// Whether the share extension can capture tasks right now.
enum CaptureAvailability: Sendable {
    case available
    case vaultLocked
    case unavailable(reason: String)
}

// MARK: - Vault Access Checking

/// Abstraction for checking whether capture is possible.
///
/// In development: always returns `.available`.
/// In production: checks App Group vault state.
protocol ShareVaultAccessChecking: Sendable {
    func captureAvailability() async -> CaptureAvailability
}

/// Mock vault access — always available for development.
struct MockShareVaultAccess: ShareVaultAccessChecking {
    func captureAvailability() async -> CaptureAvailability {
        .available
    }
}
