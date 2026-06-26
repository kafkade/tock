// TockSwift — idiomatic async/await wrappers around UniFFI bindings
//
// The UniFFI-generated API (in TockFFI) is synchronous and blocking. This
// module wraps it into Swift-native patterns:
//
// * All vault and mutation calls dispatch to a background queue and
//   return via `async throws`.
// * The generated `Tock*` record/enum types are re-exported so consumers
//   only need `import TockSwift`.
//
// ## Usage
//
// ```swift
// import TockSwift
//
// let workspace = try await TockWorkspace.create(
//     path: vaultURL.path,
//     password: Data("secret".utf8)
// )
// let task = try await workspace.addTask(
//     TockNewTask(title: "Buy groceries", tags: ["errands"])
// )
// ```
//
// ## Architecture
//
// `TockWorkspace` wraps the UniFFI `Workspace` object and dispatches
// every call onto a serial `DispatchQueue` via
// `withCheckedThrowingContinuation`. This keeps the main thread free
// for UI work while the Rust/SQLite layer processes the request.
//
// A future version (per ADR-005) will use UniFFI's native async
// support backed by a Tokio current-thread runtime, eliminating the
// Swift-side queue.

import Foundation

// Re-export the generated types (TockTask, TockNewTask, TockError, …) so
// consumers only need `import TockSwift`.
@_exported import TockFFI

/// Serialised, `async`-friendly wrapper around the UniFFI `Workspace`.
///
/// All methods are `async throws` — they dispatch the synchronous FFI
/// call onto a background queue and return the result. Errors surface as
/// the generated ``TockError``.
///
/// Create via ``create(path:password:)`` (new vault) or
/// ``open(path:password:)`` (existing vault).
public final class TockWorkspace: @unchecked Sendable {
    // The queue serialises calls to the underlying SQLite connection.
    private let queue = DispatchQueue(
        label: "com.tock.workspace",
        qos: .userInitiated
    )

    private let handle: Workspace
    private let vaultPath: String

    private init(handle: Workspace, path: String) {
        self.handle = handle
        self.vaultPath = path
    }

    /// The filesystem path of the vault.
    public var path: String { vaultPath }

    // MARK: - Internal dispatch

    /// Run a synchronous FFI call on the serial queue and bridge it to
    /// `async throws`.
    private func perform<T>(
        _ body: @escaping (Workspace) throws -> T
    ) async throws -> T {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<T, Error>) in
            queue.async {
                do {
                    continuation.resume(returning: try body(self.handle))
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    /// Run a synchronous constructor off the caller's thread.
    private static func performStatic<T>(
        _ body: @escaping () throws -> T
    ) async throws -> T {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<T, Error>) in
            DispatchQueue.global(qos: .userInitiated).async {
                do {
                    continuation.resume(returning: try body())
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    // MARK: - Lifecycle

    /// Create a new vault at `path` protected by `password`.
    ///
    /// - Parameters:
    ///   - path: Filesystem path for the new vault file.
    ///   - password: Raw password bytes (zeroed after use by the Rust layer).
    /// - Returns: An open `TockWorkspace` handle.
    public static func create(path: String, password: Data) async throws -> TockWorkspace {
        let handle = try await performStatic {
            try initWorkspace(path: path, password: password)
        }
        return TockWorkspace(handle: handle, path: path)
    }

    /// Open an existing vault at `path`.
    ///
    /// - Parameters:
    ///   - path: Filesystem path to the vault file.
    ///   - password: Raw password bytes.
    /// - Returns: An open `TockWorkspace` handle.
    public static func open(path: String, password: Data) async throws -> TockWorkspace {
        let handle = try await performStatic {
            try openWorkspace(path: path, password: password)
        }
        return TockWorkspace(handle: handle, path: path)
    }

    /// Lock the workspace, zeroing key material.
    ///
    /// After this call, all other methods will throw ``TockError/locked``.
    public func lock() async throws {
        try await perform { try $0.lock() }
    }

    // MARK: - Tasks

    /// Add a new task.
    public func addTask(_ input: TockNewTask) async throws -> TockTask {
        try await perform { try $0.addTask(input: input) }
    }

    /// Fetch a task by its short ID, or `nil` if none exists.
    public func getTask(sid: UInt32) async throws -> TockTask? {
        try await perform { try $0.getTask(sid: sid) }
    }

    /// List all non-deleted tasks, ordered by urgency.
    public func listTasks() async throws -> [TockTask] {
        try await perform { try $0.listTasks() }
    }

    /// Apply a patch to the task with the given short ID.
    public func modifyTask(sid: UInt32, patch: TockTaskPatch) async throws -> TockTask {
        try await perform { try $0.modifyTask(sid: sid, patch: patch) }
    }

    /// Mark a task as done.
    public func completeTask(sid: UInt32) async throws -> TockTask {
        try await perform { try $0.completeTask(sid: sid) }
    }

    /// Mark a task as cancelled.
    public func cancelTask(sid: UInt32) async throws -> TockTask {
        try await perform { try $0.cancelTask(sid: sid) }
    }

    /// Soft-delete a task.
    public func deleteTask(sid: UInt32) async throws {
        try await perform { try $0.deleteTask(sid: sid) }
    }

    // MARK: - Projects

    /// Add a new project.
    public func addProject(_ input: TockNewProject) async throws -> TockProject {
        try await perform { try $0.addProject(input: input) }
    }

    /// Fetch a project by its short ID, or `nil` if none exists.
    public func getProject(sid: UInt32) async throws -> TockProject? {
        try await perform { try $0.getProject(sid: sid) }
    }

    /// List all projects.
    public func listProjects() async throws -> [TockProject] {
        try await perform { try $0.listProjects() }
    }

    // MARK: - Areas

    /// Add a new area.
    public func addArea(_ input: TockNewArea) async throws -> TockArea {
        try await perform { try $0.addArea(input: input) }
    }

    /// List all areas.
    public func listAreas() async throws -> [TockArea] {
        try await perform { try $0.listAreas() }
    }

    // MARK: - Tags

    /// List all tags.
    public func listTags() async throws -> [TockTag] {
        try await perform { try $0.listTags() }
    }

    // MARK: - Time blocks

    /// Start a new time-tracking block.
    public func startTimer(_ input: TockNewTimeBlock) async throws -> TockTimeBlock {
        try await perform { try $0.startTimer(input: input) }
    }

    /// Stop the time block with the given short ID.
    public func stopTimer(sid: UInt32) async throws -> TockTimeBlock {
        try await perform { try $0.stopTimer(sid: sid) }
    }

    /// The currently running time block, if any.
    public func currentTimer() async throws -> TockTimeBlock? {
        try await perform { try $0.currentTimer() }
    }

    /// Resume tracking by starting a block from the most recent one.
    public func resumeTimer() async throws -> TockTimeBlock {
        try await perform { try $0.resumeTimer() }
    }

    /// List all time blocks.
    public func listTimeBlocks() async throws -> [TockTimeBlock] {
        try await perform { try $0.listTimeBlocks() }
    }

    // MARK: - Focus sessions

    /// Start a new Pomodoro focus session.
    public func startFocus(_ input: TockNewFocusSession) async throws -> TockFocusSession {
        try await perform { try $0.startFocus(input: input) }
    }

    /// The currently active focus session, if any.
    public func focusStatus() async throws -> TockFocusSession? {
        try await perform { try $0.focusStatus() }
    }

    /// Complete the current work cycle of a focus session.
    public func completeFocusCycle(sid: UInt32) async throws -> TockFocusSession {
        try await perform { try $0.completeFocusCycle(sid: sid) }
    }

    /// Skip the current break of a focus session.
    public func skipFocusBreak(sid: UInt32) async throws -> TockFocusSession {
        try await perform { try $0.skipFocusBreak(sid: sid) }
    }

    /// Pause a focus session.
    public func pauseFocus(sid: UInt32) async throws -> TockFocusSession {
        try await perform { try $0.pauseFocus(sid: sid) }
    }

    /// Resume a paused focus session.
    public func resumeFocus(sid: UInt32) async throws -> TockFocusSession {
        try await perform { try $0.resumeFocus(sid: sid) }
    }

    /// Abort a focus session before its planned cycles complete.
    public func abortFocus(sid: UInt32) async throws -> TockFocusSession {
        try await perform { try $0.abortFocus(sid: sid) }
    }

    /// Finish a focus session, logging completed cycles.
    public func finishFocus(sid: UInt32) async throws -> TockFocusSession {
        try await perform { try $0.finishFocus(sid: sid) }
    }

    // MARK: - Habits

    /// Add a new habit.
    public func addHabit(_ input: TockNewHabit) async throws -> TockHabit {
        try await perform { try $0.addHabit(input: input) }
    }

    /// Fetch a habit by its short ID, or `nil` if none exists.
    public func getHabit(sid: UInt32) async throws -> TockHabit? {
        try await perform { try $0.getHabit(sid: sid) }
    }

    /// List all active (non-archived) habits.
    public func listHabits() async throws -> [TockHabit] {
        try await perform { try $0.listHabits() }
    }

    /// Log a habit entry (completion or slip).
    public func logHabit(
        habitSid: UInt32,
        amount: String,
        notes: String?,
        slip: Bool
    ) async throws -> TockHabitEntry {
        try await perform {
            try $0.logHabit(habitSid: habitSid, amount: amount, notes: notes, slip: slip)
        }
    }

    /// Archive a habit by its short ID.
    public func archiveHabit(sid: UInt32) async throws {
        try await perform { try $0.archiveHabit(sid: sid) }
    }
}
