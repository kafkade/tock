import SwiftUI

/// A time block row showing title, duration, and running indicator.
struct TimeBlockRow: View {
    let block: TimeBlockItem

    var body: some View {
        HStack(spacing: TockTheme.Spacing.sm) {
            if block.isRunning {
                Circle()
                    .fill(TockTheme.Colors.timerActive)
                    .frame(width: 8, height: 8)
            }

            VStack(alignment: .leading, spacing: TockTheme.Spacing.xxs) {
                Text(block.title)
                    .font(.body)

                Text(block.startedAt, style: .time)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            Text(Self.formatDuration(block.duration))
                .font(.subheadline)
                .monospacedDigit()
                .foregroundStyle(block.isRunning ? TockTheme.Colors.timerActive : .secondary)
        }
        .padding(.vertical, TockTheme.Spacing.xxs)
    }

    static func formatDuration(_ interval: TimeInterval) -> String {
        let hours = Int(interval) / 3600
        let minutes = (Int(interval) % 3600) / 60
        let seconds = Int(interval) % 60
        if hours > 0 {
            return String(format: "%d:%02d:%02d", hours, minutes, seconds)
        }
        return String(format: "%d:%02d", minutes, seconds)
    }
}
