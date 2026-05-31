import SwiftUI
import WatchKit

/// Habit row for the watch Habits list.
///
/// Shows direction indicator, title, streak, and a tap-to-log button.
/// Plays haptic feedback on log.
struct WatchHabitRow: View {
    let habit: HabitItem
    var onLog: (() -> Void)?

    var body: some View {
        HStack(spacing: WatchTheme.Spacing.md) {
            // Direction indicator
            Circle()
                .fill(habit.direction == .build
                    ? WatchTheme.Colors.habitBuild
                    : WatchTheme.Colors.habitBreak)
                .frame(width: 8, height: 8)
                .accessibilityLabel(habit.direction == .build ? "Build habit" : "Break habit")

            VStack(alignment: .leading, spacing: WatchTheme.Spacing.xs) {
                Text(habit.title)
                    .font(.caption)
                    .lineLimit(2)

                HStack(spacing: WatchTheme.Spacing.sm) {
                    Label("\(habit.streakCurrent)d", systemImage: "flame.fill")
                        .font(.caption2)
                        .foregroundStyle(habit.streakCurrent > 0 ? .orange : .secondary)
                        .accessibilityLabel("Streak: \(habit.streakCurrent) days")

                    Text("Lv.\(habit.level)")
                        .font(.caption2)
                        .foregroundStyle(.yellow)
                }
            }

            Spacer()

            // Log button
            Button {
                WKInterfaceDevice.current().play(.success)
                onLog?()
            } label: {
                Image(systemName: habit.direction == .build
                    ? "checkmark.circle.fill"
                    : "xmark.circle.fill")
                    .font(.title3)
                    .foregroundStyle(WatchTheme.Colors.accent)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Log habit")
            .accessibilityHint("Logs progress for this habit")
        }
    }
}
