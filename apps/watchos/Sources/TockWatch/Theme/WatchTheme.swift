import SwiftUI

/// Design tokens for the Tock watchOS app.
///
/// Adapted from the iOS `TockTheme` for small-screen constraints.
/// Uses slightly tighter spacing and watch-appropriate type scales.
enum WatchTheme {

    // MARK: - Colors
    //
    // Accessibility: All semantic colors use SwiftUI system adaptive colors
    // which adjust for light/dark mode and increased contrast settings.
    // Colors are never the sole indicator of meaning — all color-coded
    // elements also have text labels or accessibility labels.

    enum Colors {
        static let accent = Color.blue
        static let destructive = Color.red
        static let success = Color.green
        static let warning = Color.orange

        static let priorityHigh = Color.red
        static let priorityMedium = Color.orange
        static let priorityLow = Color.blue

        static let habitBuild = Color.green
        static let habitBreak = Color.purple

        static let timerActive = Color.green
        static let timerPaused = Color.orange

        static let focusWork = Color.red
        static let focusBreak = Color.green
    }

    // MARK: - Spacing

    enum Spacing {
        static let xxs: CGFloat = 1
        static let xs: CGFloat = 2
        static let sm: CGFloat = 4
        static let md: CGFloat = 6
        static let lg: CGFloat = 8
        static let xl: CGFloat = 12
    }
}
