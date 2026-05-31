import Foundation

/// Watch-scoped dependency boundary between SwiftUI views and the Rust core.
///
/// A constrained subset of the full `CoreClient` — the watch only supports
/// today's tasks, habits, timer, and focus sessions. No projects, areas,
/// search, or full vault access.
///
/// In production, the implementation forwards actions to the iPhone via
/// `WatchConnectivity` and reads from the cached snapshot. In previews,
/// `MockWatchCoreClient` returns static data.
protocol WatchCoreClient: Sendable {

    // MARK: Tasks (today only, max 20)
    func listTodayTasks() async throws -> [TaskItem]
    func completeTask(id: String) async throws

    // MARK: Time tracking
    func currentTimer() async throws -> TimeBlockItem?
    func startTimer(title: String, taskId: String?) async throws -> TimeBlockItem
    func stopTimer() async throws -> TimeBlockItem?

    // MARK: Focus
    func focusStatus() async throws -> FocusSessionItem?
    func startFocus(taskId: String?, cycles: UInt32) async throws -> FocusSessionItem
    func completeFocusCycle() async throws -> FocusSessionItem
    func skipBreak() async throws -> FocusSessionItem
    func pauseFocus() async throws -> FocusSessionItem
    func resumeFocus() async throws -> FocusSessionItem
    func abortFocus() async throws -> FocusSessionItem

    // MARK: Habits
    func listHabits() async throws -> [HabitItem]
    func logHabit(id: String, notes: String?) async throws -> HabitEntryItem
}
