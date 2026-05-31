import Foundation

/// Deep-link URL helpers for widget tap actions.
///
/// Widgets use `widgetURL(_:)` or `Link(destination:)` to open the app
/// at the appropriate screen. The main app's scene handles these URLs
/// via `.onOpenURL { }`.
enum WidgetDeepLinks {

    private static let scheme = "tock"

    static let today = URL(string: "\(scheme)://today")!
    static let inbox = URL(string: "\(scheme)://inbox")!
    static let habits = URL(string: "\(scheme)://habits")!
    static let timer = URL(string: "\(scheme)://timer")!
    static let unlock = URL(string: "\(scheme)://unlock")!

    static func task(id: String) -> URL {
        URL(string: "\(scheme)://task/\(id)")!
    }

    static func habit(id: String) -> URL {
        URL(string: "\(scheme)://habit/\(id)")!
    }
}
