// NOTE: Shared with apps/ios — extract to shared package when apps/shared is created.

import SwiftUI

/// Small colored circle indicating task priority.
struct PriorityBadge: View {
    let priority: Priority

    private var color: Color {
        switch priority {
        case .high: TockTheme.Colors.priorityHigh
        case .medium: TockTheme.Colors.priorityMedium
        case .low: TockTheme.Colors.priorityLow
        }
    }

    var body: some View {
        Circle()
            .fill(color)
            .frame(width: 8, height: 8)
            .accessibilityLabel("\(priority.accessibilityName) priority")
    }
}

private extension Priority {
    var accessibilityName: String {
        switch self {
        case .high: "High"
        case .medium: "Medium"
        case .low: "Low"
        }
    }
}
