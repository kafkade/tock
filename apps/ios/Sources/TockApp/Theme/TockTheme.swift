import SwiftUI

/// Design tokens for the Tock iOS app.
///
/// Centralizes colors, spacing, and typography so views stay consistent.
enum TockTheme {

    // MARK: - Colors

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
        static let xxs: CGFloat = 2
        static let xs: CGFloat = 4
        static let sm: CGFloat = 8
        static let md: CGFloat = 12
        static let lg: CGFloat = 16
        static let xl: CGFloat = 24
        static let xxl: CGFloat = 32
    }

    // MARK: - Corner radius

    enum Radius {
        static let sm: CGFloat = 6
        static let md: CGFloat = 10
        static let lg: CGFloat = 16
        static let full: CGFloat = 999
    }
}
