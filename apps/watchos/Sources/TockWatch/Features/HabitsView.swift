import SwiftUI
import WatchKit

/// Habit tracker for the watch — view streaks and tap to log.
///
/// Groups habits by direction (build vs break). Each row has a
/// tap-to-log button with haptic confirmation.
struct HabitsView: View {
    @Environment(WatchAppState.self) private var appState
    @State private var habits: [HabitItem] = []
    @State private var isLoading = false

    var body: some View {
        Group {
            if isLoading && habits.isEmpty {
                ProgressView()
            } else if habits.isEmpty {
                emptyView
            } else {
                habitList
            }
        }
        .navigationTitle("Habits")
        .task {
            await load()
        }
    }

    @ViewBuilder
    private var habitList: some View {
        let buildHabits = habits.filter { $0.direction == .build }
        let breakHabits = habits.filter { $0.direction == .breakHabit }

        List {
            if !buildHabits.isEmpty {
                Section("Build") {
                    ForEach(buildHabits) { habit in
                        WatchHabitRow(habit: habit) {
                            Task { await logHabit(habit) }
                        }
                    }
                }
            }

            if !breakHabits.isEmpty {
                Section("Break") {
                    ForEach(breakHabits) { habit in
                        WatchHabitRow(habit: habit) {
                            Task { await logHabit(habit) }
                        }
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var emptyView: some View {
        VStack(spacing: WatchTheme.Spacing.lg) {
            Image(systemName: "flame")
                .font(.title2)
                .foregroundStyle(.orange)
                .accessibilityHidden(true)
            Text("No habits")
                .font(.headline)
            Text("Add habits on iPhone")
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }

    private func load() async {
        isLoading = true
        do {
            habits = try await appState.client.listHabits()
        } catch {
            habits = []
        }
        isLoading = false
    }

    private func logHabit(_ habit: HabitItem) async {
        do {
            _ = try await appState.client.logHabit(id: habit.id, notes: nil)
            // Refresh to show updated streak
            await load()
        } catch {
            // Silently fail — intent will be retried via queue
        }
    }
}
