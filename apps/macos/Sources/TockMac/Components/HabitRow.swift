// NOTE: Shared with apps/ios — extract to shared package when apps/shared is created.

import SwiftUI

/// A habit row showing title, identity, streak, and level.
struct HabitRow: View {
    let habit: HabitItem
    var onLog: (() -> Void)?

    var body: some View {
        HStack(spacing: TockTheme.Spacing.md) {
            // Direction indicator
            Circle()
                .fill(habit.direction == .build
                    ? TockTheme.Colors.habitBuild
                    : TockTheme.Colors.habitBreak)
                .frame(width: 10, height: 10)
                .accessibilityLabel(habit.direction == .build ? "Build habit" : "Break habit")

            VStack(alignment: .leading, spacing: TockTheme.Spacing.xxs) {
                Text(habit.title)
                    .font(.body)

                if let identity = habit.identity {
                    Text(identity)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .italic()
                }

                HStack(spacing: TockTheme.Spacing.sm) {
                    Label("\(habit.streakCurrent)d", systemImage: "flame.fill")
                        .font(.caption)
                        .foregroundStyle(habit.streakCurrent > 0 ? .orange : .secondary)
                        .accessibilityLabel("Streak: \(habit.streakCurrent) days")

                    Label("Lv.\(habit.level)", systemImage: "star.fill")
                        .font(.caption)
                        .foregroundStyle(.yellow)
                        .accessibilityLabel("Level \(habit.level)")

                    Text(habit.levelName)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }

            Spacer()

            Button {
                onLog?()
            } label: {
                Image(systemName: habit.direction == .build
                    ? "checkmark.circle"
                    : "xmark.circle")
                    .font(.title2)
                    .foregroundStyle(TockTheme.Colors.accent)
            }
            .buttonStyle(.plain)
            .accessibilityLabel(habit.direction == .build ? "Log habit" : "Log slip")
            .accessibilityHint(habit.direction == .build
                ? "Records a completion for this habit"
                : "Records a slip for this habit")
        }
        .padding(.vertical, TockTheme.Spacing.xxs)
    }
}
