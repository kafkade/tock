import Foundation
import TockSwift

/// Production `CoreClient` backed by the real Rust core via
/// `TockSwift.TockWorkspace`.
///
/// Reads and writes go through the encrypted on-device SQLite vault. The
/// protocol addresses entities by their UUID `id`, while the core mutation
/// API keys off the short numeric `sid`; this actor maintains an id→sid map
/// (populated whenever it materialises records) and refreshes it on a miss.
///
/// It is an `actor` so the id→sid caches stay race-free; the `TockWorkspace`
/// wrapper already serialises the underlying SQLite connection.
actor TockCoreClient: CoreClient {

    private let workspace: TockWorkspace
    private let pathValue: String

    private var taskSidById: [String: UInt32] = [:]
    private var habitSidById: [String: UInt32] = [:]

    init(workspace: TockWorkspace) {
        self.workspace = workspace
        self.pathValue = workspace.path
    }

    /// Errors raised while bridging UUID ids to core SIDs.
    enum ClientError: Error {
        case taskNotFound(String)
        case habitNotFound(String)
    }

    // MARK: - Tasks

    func listTasks(filter: TaskFilter) async throws -> [TaskItem] {
        let items = try await workspace.listTasks().map(\.asModel)
        cache(tasks: items)
        return items.applying(filter)
    }

    func getTask(id: String) async throws -> TaskItem? {
        let sid = try await resolveTaskSid(id)
        guard let task = try await workspace.getTask(sid: sid) else { return nil }
        let item = task.asModel
        taskSidById[item.id] = item.sid
        return item
    }

    func addTask(_ input: NewTaskInput) async throws -> TaskItem {
        let new = TockNewTask(
            title: input.title,
            notes: input.notes,
            status: nil,
            projectId: input.projectId,
            areaId: nil,
            headingId: nil,
            startDate: nil,
            deadline: TockDate.format(input.deadline),
            scheduledFor: nil,
            recurrence: nil,
            priority: input.priority?.asTock,
            evening: input.evening,
            udas: "{}",
            tags: input.tags
        )
        let item = try await workspace.addTask(new).asModel
        taskSidById[item.id] = item.sid
        return item
    }

    func completeTask(id: String) async throws {
        let sid = try await resolveTaskSid(id)
        _ = try await workspace.completeTask(sid: sid)
    }

    func deleteTask(id: String) async throws {
        let sid = try await resolveTaskSid(id)
        try await workspace.deleteTask(sid: sid)
    }

    func modifyTask(id: String, projectId: String?) async throws {
        let sid = try await resolveTaskSid(id)
        let patch = TockTaskPatch(
            title: nil,
            notes: nil,
            clearNotes: false,
            status: nil,
            projectId: projectId,
            clearProject: projectId == nil,
            areaId: nil,
            clearArea: false,
            headingId: nil,
            clearHeading: false,
            startDate: nil,
            clearStartDate: false,
            deadline: nil,
            clearDeadline: false,
            scheduledFor: nil,
            clearScheduled: false,
            priority: nil,
            clearPriority: false,
            evening: nil,
            setUdas: "{}",
            removeUdaKeys: [],
            addTags: [],
            removeTags: [],
            addDeps: [],
            removeDeps: []
        )
        _ = try await workspace.modifyTask(sid: sid, patch: patch)
    }

    // MARK: - Projects

    func listProjects() async throws -> [ProjectItem] {
        try await workspace.listProjects().map(\.asModel)
    }

    func addProject(_ input: NewProjectInput) async throws -> ProjectItem {
        let new = TockNewProject(
            name: input.name,
            notes: input.notes,
            areaId: input.areaId,
            deadline: nil
        )
        return try await workspace.addProject(new).asModel
    }

    // MARK: - Areas

    func listAreas() async throws -> [AreaItem] {
        try await workspace.listAreas().map(\.asModel)
    }

    // MARK: - Tags

    func listTags() async throws -> [TagItem] {
        try await workspace.listTags().map(\.asModel)
    }

    // MARK: - Time tracking

    func startTimer(title: String, taskId: String?) async throws -> TimeBlockItem {
        let taskSid = try await resolveOptionalTaskSid(taskId)
        let new = TockNewTimeBlock(
            title: title,
            taskSid: taskSid,
            projectId: nil,
            notes: nil
        )
        return try await workspace.startTimer(new).asModel
    }

    func stopTimer() async throws -> TimeBlockItem? {
        guard let current = try await workspace.currentTimer() else { return nil }
        return try await workspace.stopTimer(sid: current.sid).asModel
    }

    func currentTimer() async throws -> TimeBlockItem? {
        try await workspace.currentTimer()?.asModel
    }

    func listTimeBlocks() async throws -> [TimeBlockItem] {
        try await workspace.listTimeBlocks().map(\.asModel)
    }

    // MARK: - Focus

    func startFocus(taskId: String?, cycles: UInt32) async throws -> FocusSessionItem {
        let taskSid = try await resolveOptionalTaskSid(taskId)
        let new = TockNewFocusSession(
            taskSid: taskSid,
            projectId: nil,
            plannedCycles: cycles,
            config: TockDefaults.focusConfig
        )
        return try await workspace.startFocus(new).asModel
    }

    func focusStatus() async throws -> FocusSessionItem? {
        try await workspace.focusStatus()?.asModel
    }

    func completeFocusCycle() async throws -> FocusSessionItem {
        let sid = try await activeFocusSid()
        return try await workspace.completeFocusCycle(sid: sid).asModel
    }

    func skipBreak() async throws -> FocusSessionItem {
        let sid = try await activeFocusSid()
        return try await workspace.skipFocusBreak(sid: sid).asModel
    }

    func pauseFocus() async throws -> FocusSessionItem {
        let sid = try await activeFocusSid()
        return try await workspace.pauseFocus(sid: sid).asModel
    }

    func resumeFocus() async throws -> FocusSessionItem {
        let sid = try await activeFocusSid()
        return try await workspace.resumeFocus(sid: sid).asModel
    }

    func abortFocus() async throws -> FocusSessionItem {
        let sid = try await activeFocusSid()
        return try await workspace.abortFocus(sid: sid).asModel
    }

    // MARK: - Habits

    func listHabits() async throws -> [HabitItem] {
        let items = try await workspace.listHabits().map(\.asModel)
        cache(habits: items)
        return items
    }

    func addHabit(title: String, identity: String?) async throws -> HabitItem {
        let new = TockNewHabit(
            title: title,
            identity: identity,
            cue: nil,
            craving: nil,
            response: nil,
            reward: nil,
            direction: .build,
            cadence: TockDefaults.dailyCadence,
            minimum: TockDefaults.booleanMinimum,
            stackAfter: nil,
            stackDelayS: 0,
            areaId: nil,
            projectId: nil
        )
        let item = try await workspace.addHabit(new).asModel
        habitSidById[item.id] = item.sid
        return item
    }

    func logHabit(id: String, notes: String?) async throws -> HabitEntryItem {
        let sid = try await resolveHabitSid(id)
        return try await workspace.logHabit(
            habitSid: sid,
            amount: TockDefaults.booleanAmount,
            notes: notes,
            slip: false
        ).asModel
    }

    // MARK: - Vault

    nonisolated func vaultPath() -> String { pathValue }

    /// Lock the underlying vault, zeroing key material.
    func lock() async throws {
        try await workspace.lock()
    }

    // MARK: - id → sid resolution

    private func cache(tasks: [TaskItem]) {
        for task in tasks { taskSidById[task.id] = task.sid }
    }

    private func cache(habits: [HabitItem]) {
        for habit in habits { habitSidById[habit.id] = habit.sid }
    }

    private func resolveTaskSid(_ id: String) async throws -> UInt32 {
        if let sid = taskSidById[id] { return sid }
        cache(tasks: try await workspace.listTasks().map(\.asModel))
        guard let sid = taskSidById[id] else { throw ClientError.taskNotFound(id) }
        return sid
    }

    private func resolveOptionalTaskSid(_ id: String?) async throws -> UInt32? {
        guard let id else { return nil }
        return try await resolveTaskSid(id)
    }

    private func resolveHabitSid(_ id: String) async throws -> UInt32 {
        if let sid = habitSidById[id] { return sid }
        cache(habits: try await workspace.listHabits().map(\.asModel))
        guard let sid = habitSidById[id] else { throw ClientError.habitNotFound(id) }
        return sid
    }

    private func activeFocusSid() async throws -> UInt32 {
        guard let session = try await workspace.focusStatus() else {
            throw ClientError.taskNotFound("no active focus session")
        }
        return session.sid
    }
}
