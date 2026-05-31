import AppIntents

/// App-level enum for tock navigation views, used by `OpenViewIntent`.
///
/// Maps to `TaskFilter` cases and special destinations. Available in
/// Siri and Shortcuts as a selectable parameter: "Open inbox in Tock".
enum TockView: String, AppEnum {
    case today
    case inbox
    case upcoming
    case anytime
    case someday
    case logbook
    case habits
    case timer
    case settings

    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "View")

    static var caseDisplayRepresentations: [TockView: DisplayRepresentation] = [
        .today: "Today",
        .inbox: "Inbox",
        .upcoming: "Upcoming",
        .anytime: "Anytime",
        .someday: "Someday",
        .logbook: "Logbook",
        .habits: "Habits",
        .timer: "Timer",
        .settings: "Settings",
    ]

    /// The deep-link URL for this view.
    var deepLinkURL: URL {
        switch self {
        case .today: return WidgetDeepLinks.today
        case .inbox: return WidgetDeepLinks.inbox
        case .habits: return WidgetDeepLinks.habits
        case .timer: return WidgetDeepLinks.timer
        default: return URL(string: "tock://\(rawValue)")!
        }
    }
}
