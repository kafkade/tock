import SwiftUI

/// Daily habit tracker with streak display.
struct HabitsView: View {
    @Environment(AppSessionState.self) private var appState
    @State private var viewModel: HabitsViewModel?

    var body: some View {
        Group {
            if let vm = viewModel {
                contentView(vm: vm)
            } else {
                ProgressView()
            }
        }
        .navigationTitle("Habits")
        .task {
            let vm = HabitsViewModel(client: appState.client)
            viewModel = vm
            await vm.load()
        }
    }

    @ViewBuilder
    private func contentView(vm: HabitsViewModel) -> some View {
        if vm.isLoading && vm.habits.isEmpty {
            ProgressView("Loading...")
        } else if vm.habits.isEmpty {
            ContentUnavailableView(
                "No habits",
                systemImage: "flame",
                description: Text("Start building positive habits.")
            )
        } else {
            List {
                let buildHabits = vm.habits.filter { $0.direction == .build }
                let breakHabits = vm.habits.filter { $0.direction == .breakHabit }

                if !buildHabits.isEmpty {
                    Section("Build") {
                        ForEach(buildHabits) { habit in
                            NavigationLink(value: habit) {
                                HabitRow(habit: habit) {
                                    Task { await vm.logHabit(habit) }
                                }
                            }
                        }
                    }
                }

                if !breakHabits.isEmpty {
                    Section("Break") {
                        ForEach(breakHabits) { habit in
                            NavigationLink(value: habit) {
                                HabitRow(habit: habit) {
                                    Task { await vm.logHabit(habit) }
                                }
                            }
                        }
                    }
                }
            }
            .navigationDestination(for: HabitItem.self) { habit in
                HabitDetailView(habit: habit)
            }
        }
    }
}
