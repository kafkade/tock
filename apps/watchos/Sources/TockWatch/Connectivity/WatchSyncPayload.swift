import Foundation

/// Wire schema shared between the iPhone (`PhoneSessionManager`) and the watch
/// (`WatchSessionManager`). Both targets keep an **identical** copy of this
/// file so the JSON snapshot encoded on the phone decodes losslessly on the
/// watch (the two app targets don't share a module).
///
/// Transport:
/// - The phone publishes a `WatchSnapshotDTO` as JSON `Data` under
///   ``WatchSync/snapshotKey`` via `updateApplicationContext` (latest-wins) and,
///   when reachable, an interactive `sendMessage` tagged
///   ``WatchSync/messageTypeKey`` = ``WatchSync/snapshotType``.
/// - The watch sends mutation intents tagged ``WatchSync/messageTypeKey`` =
///   ``WatchSync/intentType`` and the phone replies with ``WatchSync/ackKey``.
enum WatchSync {
    static let snapshotKey = "snapshot"
    static let messageTypeKey = "type"
    static let snapshotType = "snapshot"
    static let intentType = "intent"
    static let intentIdKey = "intentId"
    static let intentTypeKey = "intentType"
    static let payloadKey = "payload"
    static let ackKey = "ack"

    /// JSON coder with ISO-8601 dates, used on both sides so timestamps match.
    static let encoder: JSONEncoder = {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        return encoder
    }()

    static let decoder: JSONDecoder = {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }()
}

/// The actionable surface the watch mirrors. Enum values are stored as their
/// `rawValue` strings (e.g. priority `"high"`, direction `"build"`,
/// focus state `"working"`) which are identical across the phone and watch
/// model enums.
struct WatchSnapshotDTO: Codable, Sendable {
    var vaultLocked: Bool
    var generatedAt: Date
    var dueCount: Int
    var todayTasks: [TaskDTO]
    var habits: [HabitDTO]
    var activeTimer: TimerDTO?
    var activeFocus: FocusDTO?

    struct TaskDTO: Codable, Sendable {
        var id: String
        var sid: UInt32
        var title: String
        var status: String
        var deadline: Date?
        var priority: String?
        var evening: Bool
        var urgency: Double
    }

    struct HabitDTO: Codable, Sendable {
        var id: String
        var sid: UInt32
        var title: String
        var identity: String?
        var direction: String
        var level: UInt32
        var streakCurrent: UInt32
        var streakBest: UInt32
        var levelName: String
    }

    struct TimerDTO: Codable, Sendable {
        var id: String
        var sid: UInt32
        var title: String
        var startedAt: Date
        var taskId: String?
    }

    struct FocusDTO: Codable, Sendable {
        var id: String
        var sid: UInt32
        var startedAt: Date
        var taskId: String?
        var plannedCycles: UInt32
        var completedCycles: UInt32
        var state: String
        var workMinutes: UInt32
        var shortBreakMinutes: UInt32
        var longBreakMinutes: UInt32
    }

    /// An empty, locked snapshot — shown before the first sync or when the
    /// phone reports the vault is locked.
    static let locked = WatchSnapshotDTO(
        vaultLocked: true, generatedAt: .distantPast, dueCount: 0,
        todayTasks: [], habits: [], activeTimer: nil, activeFocus: nil
    )
}
