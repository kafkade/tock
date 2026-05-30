import SwiftUI

@Observable
@MainActor
final class HabitsViewModel {
    private let client: any CoreClient

    var habits: [HabitItem] = []
    var isLoading = false
    var error: String?

    init(client: any CoreClient) {
        self.client = client
    }

    func load() async {
        isLoading = true
        error = nil
        do {
            habits = try await client.listHabits()
                .filter { $0.archivedAt == nil }
        } catch {
            self.error = error.localizedDescription
        }
        isLoading = false
    }

    func logHabit(_ habit: HabitItem) async {
        do {
            _ = try await client.logHabit(id: habit.id, notes: nil)
            await load() // Reload to update streaks
        } catch {
            self.error = error.localizedDescription
        }
    }
}
