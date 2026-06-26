import AppIntents
import Foundation

// MARK: - Log Habit Intent

/// Siri: "Log meditation 10 minutes"
///
/// Logs a habit completion with an optional value/notes. Resolves the habit
/// by name via `HabitEntityQuery`. Also used by widget habit chips
/// (via the convenience `init(habit:)` with a pre-built `HabitEntity`).
struct LogHabitIntent: AppIntent {
    static var title: LocalizedStringResource = "Log Habit"
    static var description: IntentDescription = "Logs a habit completion in tock."
    static var openAppWhenRun = false

    @Parameter(title: "Habit")
    var habit: HabitEntity

    @Parameter(title: "Notes")
    var notes: String?

    init() {}

    init(habit: HabitEntity) {
        self.habit = habit
    }

    func perform() async throws -> some IntentResult & ProvidesDialog {
        let client = try await VaultGateway.shared.client()
        _ = try await client.logHabit(id: habit.id, notes: notes)
        await WidgetSnapshotWriter.publish(from: client)
        return .result(dialog: "Logged '\(habit.title)' ✓")
    }
}

// MARK: - Show Habit Streak Intent

/// Siri: "What's my reading streak?"
///
/// Returns the current streak for a habit. If no habit is specified,
/// shows a summary of all active habits.
struct ShowHabitStreakIntent: AppIntent {
    static var title: LocalizedStringResource = "Show Habit Streak"
    static var description: IntentDescription = "Shows your habit streak in tock."
    static var openAppWhenRun = false

    @Parameter(title: "Habit")
    var habit: HabitEntity?

    func perform() async throws -> some IntentResult & ProvidesDialog {
        let client = try await VaultGateway.shared.client()

        if let habit {
            let habits = try await client.listHabits()
            if let found = habits.first(where: { $0.id == habit.id }) {
                return .result(dialog: "\(found.title): \(found.streakCurrent)-day streak (best: \(found.streakBest)) · \(found.levelName) 🔥")
            } else {
                return .result(dialog: "Habit '\(habit.title)' not found.")
            }
        } else {
            let habits = try await client.listHabits()
            if habits.isEmpty {
                return .result(dialog: "No habits tracked yet.")
            }
            let summaries = habits.prefix(3).map { "\($0.title): \($0.streakCurrent)🔥" }
            return .result(dialog: summaries.joined(separator: " · "))
        }
    }
}
