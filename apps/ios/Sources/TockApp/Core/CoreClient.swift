import Foundation

/// Dependency boundary between SwiftUI views and the Rust core.
///
/// In production, `TockCoreClient` wraps `CoreActor` → UniFFI.
/// In previews/tests, `MockCoreClient` returns static data.
protocol CoreClient: Sendable {

    // MARK: Tasks
    func listTasks(filter: TaskFilter) async throws -> [TaskItem]
    func addTask(_ input: NewTaskInput) async throws -> TaskItem
    func completeTask(id: String) async throws
    func deleteTask(id: String) async throws

    // MARK: Projects
    func listProjects() async throws -> [ProjectItem]
    func addProject(_ input: NewProjectInput) async throws -> ProjectItem

    // MARK: Areas
    func listAreas() async throws -> [AreaItem]

    // MARK: Tags
    func listTags() async throws -> [TagItem]

    // MARK: Time tracking
    func startTimer(title: String, taskId: String?) async throws -> TimeBlockItem
    func stopTimer() async throws -> TimeBlockItem?
    func currentTimer() async throws -> TimeBlockItem?
    func listTimeBlocks() async throws -> [TimeBlockItem]

    // MARK: Focus
    func startFocus(taskId: String?, cycles: UInt32) async throws -> FocusSessionItem
    func focusStatus() async throws -> FocusSessionItem?
    func completeFocusCycle() async throws -> FocusSessionItem
    func skipBreak() async throws -> FocusSessionItem
    func pauseFocus() async throws -> FocusSessionItem
    func resumeFocus() async throws -> FocusSessionItem
    func abortFocus() async throws -> FocusSessionItem

    // MARK: Habits
    func listHabits() async throws -> [HabitItem]
    func addHabit(title: String, identity: String?) async throws -> HabitItem
    func logHabit(id: String, notes: String?) async throws -> HabitEntryItem

    // MARK: Vault
    func vaultPath() -> String
}
