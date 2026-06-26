import Foundation

/// Sentinel `CoreClient` used while the vault is locked.
///
/// Every operation throws ``CoreClientError/vaultLocked`` so that no UI code
/// path silently falls back to mock data when the vault isn't open. Reads that
/// the locked UI performs (none, in practice — locked state shows
/// `VaultSetupView`) fail loudly instead of returning fixtures.
struct LockedCoreClient: CoreClient {

    func listTasks(filter: TaskFilter) async throws -> [TaskItem] { throw CoreClientError.vaultLocked }
    func getTask(id: String) async throws -> TaskItem? { throw CoreClientError.vaultLocked }
    func addTask(_ input: NewTaskInput) async throws -> TaskItem { throw CoreClientError.vaultLocked }
    func completeTask(id: String) async throws { throw CoreClientError.vaultLocked }
    func deleteTask(id: String) async throws { throw CoreClientError.vaultLocked }
    func modifyTask(id: String, projectId: String?) async throws { throw CoreClientError.vaultLocked }

    func listProjects() async throws -> [ProjectItem] { throw CoreClientError.vaultLocked }
    func addProject(_ input: NewProjectInput) async throws -> ProjectItem { throw CoreClientError.vaultLocked }

    func listAreas() async throws -> [AreaItem] { throw CoreClientError.vaultLocked }
    func listTags() async throws -> [TagItem] { throw CoreClientError.vaultLocked }

    func startTimer(title: String, taskId: String?) async throws -> TimeBlockItem { throw CoreClientError.vaultLocked }
    func stopTimer() async throws -> TimeBlockItem? { throw CoreClientError.vaultLocked }
    func currentTimer() async throws -> TimeBlockItem? { throw CoreClientError.vaultLocked }
    func listTimeBlocks() async throws -> [TimeBlockItem] { throw CoreClientError.vaultLocked }

    func startFocus(taskId: String?, cycles: UInt32) async throws -> FocusSessionItem { throw CoreClientError.vaultLocked }
    func focusStatus() async throws -> FocusSessionItem? { throw CoreClientError.vaultLocked }
    func completeFocusCycle() async throws -> FocusSessionItem { throw CoreClientError.vaultLocked }
    func skipBreak() async throws -> FocusSessionItem { throw CoreClientError.vaultLocked }
    func pauseFocus() async throws -> FocusSessionItem { throw CoreClientError.vaultLocked }
    func resumeFocus() async throws -> FocusSessionItem { throw CoreClientError.vaultLocked }
    func abortFocus() async throws -> FocusSessionItem { throw CoreClientError.vaultLocked }

    func listHabits() async throws -> [HabitItem] { throw CoreClientError.vaultLocked }
    func addHabit(title: String, identity: String?) async throws -> HabitItem { throw CoreClientError.vaultLocked }
    func logHabit(id: String, notes: String?) async throws -> HabitEntryItem { throw CoreClientError.vaultLocked }

    func vaultPath() -> String { "" }
}

/// Errors surfaced by the app-level `CoreClient` layer.
enum CoreClientError: LocalizedError {
    case vaultLocked

    var errorDescription: String? {
        switch self {
        case .vaultLocked:
            return "The vault is locked. Unlock it to continue."
        }
    }
}
