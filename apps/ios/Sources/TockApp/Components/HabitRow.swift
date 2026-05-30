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

                    Label("Lv.\(habit.level)", systemImage: "star.fill")
                        .font(.caption)
                        .foregroundStyle(.yellow)

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
        }
        .padding(.vertical, TockTheme.Spacing.xxs)
    }
}
