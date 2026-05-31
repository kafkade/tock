import SwiftUI

/// Represents a sidebar destination in the iPad NavigationSplitView.
///
/// Uses stable IDs (not full model objects) to avoid stale equality after reloads.
enum SidebarItem: Hashable, Sendable {

    // Smart views
    case today
    case inbox
    case upcoming
    case anytime
    case someday
    case logbook

    // Entity views
    case project(id: String)
    case area(id: String)

    // Other sections
    case habits
    case timer
    case settings

    /// The corresponding `TaskFilter` for list-based sidebar items.
    var taskFilter: TaskFilter? {
        switch self {
        case .today: .today
        case .inbox: .inbox
        case .upcoming: .upcoming
        case .anytime: .anytime
        case .someday: .someday
        case .logbook: .logbook
        case .project(let id): .project(id: id)
        default: nil
        }
    }

    var title: String {
        switch self {
        case .today: "Today"
        case .inbox: "Inbox"
        case .upcoming: "Upcoming"
        case .anytime: "Anytime"
        case .someday: "Someday"
        case .logbook: "Logbook"
        case .project: "Project"
        case .area: "Area"
        case .habits: "Habits"
        case .timer: "Timer"
        case .settings: "Settings"
        }
    }

    var icon: String {
        switch self {
        case .today: "sun.max.fill"
        case .inbox: "tray.fill"
        case .upcoming: "calendar"
        case .anytime: "tray.2.fill"
        case .someday: "moon.zzz.fill"
        case .logbook: "book.closed.fill"
        case .project: "folder.fill"
        case .area: "rectangle.stack.fill"
        case .habits: "flame.fill"
        case .timer: "timer"
        case .settings: "gear"
        }
    }

    /// Items reachable via ⌘1–⌘5 keyboard shortcuts.
    static let keyboardAccessible: [SidebarItem] = [
        .today, .inbox, .upcoming, .anytime, .someday,
    ]
}
