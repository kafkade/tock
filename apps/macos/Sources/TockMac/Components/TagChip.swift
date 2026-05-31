// NOTE: Shared with apps/ios — extract to shared package when apps/shared is created.

import SwiftUI

/// Small pill-shaped tag label.
struct TagChip: View {
    let name: String
    var color: Color = .secondary

    var body: some View {
        Text(name)
            .font(.caption2)
            .padding(.horizontal, TockTheme.Spacing.xs)
            .padding(.vertical, TockTheme.Spacing.xxs)
            .background(color.opacity(0.15), in: Capsule())
            .foregroundStyle(color)
    }
}
