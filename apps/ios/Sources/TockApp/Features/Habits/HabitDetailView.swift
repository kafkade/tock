import SwiftUI

/// Habit detail view — shows identity, streak, level, and stats.
struct HabitDetailView: View {
    let habit: HabitItem

    var body: some View {
        List {
            Section {
                VStack(spacing: TockTheme.Spacing.lg) {
                    // Streak ring
                    ZStack {
                        Circle()
                            .stroke(Color.secondary.opacity(0.2), lineWidth: 8)
                            .frame(width: 120, height: 120)

                        Circle()
                            .trim(from: 0, to: min(Double(habit.streakCurrent) / max(Double(habit.streakBest), 1), 1.0))
                            .stroke(
                                habit.direction == .build
                                    ? TockTheme.Colors.habitBuild
                                    : TockTheme.Colors.habitBreak,
                                style: StrokeStyle(lineWidth: 8, lineCap: .round)
                            )
                            .frame(width: 120, height: 120)
                            .rotationEffect(.degrees(-90))

                        VStack(spacing: 2) {
                            Text("\(habit.streakCurrent)")
                                .font(.title)
                                .bold()
                                .monospacedDigit()
                            Text("day streak")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                    }

                    if let identity = habit.identity {
                        Text(identity)
                            .font(.subheadline)
                            .italic()
                            .foregroundStyle(.secondary)
                            .multilineTextAlignment(.center)
                    }
                }
                .frame(maxWidth: .infinity)
                .listRowBackground(Color.clear)
            }

            Section("Stats") {
                LabeledContent("Level", value: "Lv.\(habit.level) — \(habit.levelName)")
                LabeledContent("XP", value: "\(habit.xp)")
                LabeledContent("Current streak", value: "\(habit.streakCurrent) days")
                LabeledContent("Best streak", value: "\(habit.streakBest) days")
                LabeledContent("Direction", value: habit.direction == .build ? "Build" : "Break")
            }
        }
        .navigationTitle(habit.title)
    }
}
