// Integration tests for the TockSwift wrapper against the real Rust core
// (via the TockFFI XCFramework). These run on macOS through `swift test`.
//
// Build the FFI first:
//
//     cargo xtask xcframework
//
// then:
//
//     cd bindings/swift && swift test

import Foundation
import XCTest

@testable import TockSwift

final class TockSwiftTests: XCTestCase {
    /// A throwaway vault path under a unique temp directory.
    private func makeVaultPath() throws -> String {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("tock-tests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("test.tockvault").path
    }

    private static let password = Data("test-password".utf8)

    // MARK: - Lifecycle

    func testCreateOpenLockRoundTrip() async throws {
        let path = try makeVaultPath()

        let ws = try await TockWorkspace.create(path: path, password: Self.password)
        XCTAssertEqual(ws.path, path)
        try await ws.lock()

        // Locked workspace rejects further calls.
        do {
            _ = try await ws.listTasks()
            XCTFail("expected listTasks to throw after lock")
        } catch let error as TockError {
            guard case .Locked = error else {
                return XCTFail("expected .Locked, got \(error)")
            }
        }

        // Reopen with the right password succeeds.
        let reopened = try await TockWorkspace.open(path: path, password: Self.password)
        XCTAssertEqual(reopened.path, path)
    }

    func testOpenWrongPasswordThrows() async throws {
        let path = try makeVaultPath()
        try await TockWorkspace.create(path: path, password: Self.password).lock()

        do {
            _ = try await TockWorkspace.open(path: path, password: Data("wrong".utf8))
            XCTFail("expected open to throw on wrong password")
        } catch let error as TockError {
            guard case .InvalidCredentials = error else {
                return XCTFail("expected .InvalidCredentials, got \(error)")
            }
        }
    }

    // MARK: - Tasks (the acceptance round-trip)

    func testTaskRoundTrip() async throws {
        let ws = try await TockWorkspace.create(path: makeVaultPath(), password: Self.password)

        let task = try await ws.addTask(
            TockNewTask(
                title: "Buy groceries",
                notes: "Milk, eggs, bread",
                status: nil,
                projectId: nil,
                areaId: nil,
                headingId: nil,
                startDate: nil,
                deadline: nil,
                recurrence: nil,
                priority: .high,
                evening: false,
                udas: "{}",
                tags: ["errands"]
            )
        )
        XCTAssertEqual(task.title, "Buy groceries")
        XCTAssertEqual(task.status, .inbox)
        XCTAssertEqual(task.priority, .high)
        XCTAssertEqual(task.tags, ["errands"])

        let fetched = try await ws.getTask(sid: task.sid)
        XCTAssertNotNil(fetched)
        XCTAssertEqual(fetched?.title, "Buy groceries")

        let all = try await ws.listTasks()
        XCTAssertEqual(all.count, 1)

        let patch = TockTaskPatch(
            title: "Buy more groceries",
            notes: nil,
            clearNotes: false,
            status: nil,
            projectId: nil,
            clearProject: false,
            areaId: nil,
            clearArea: false,
            headingId: nil,
            clearHeading: false,
            startDate: nil,
            clearStartDate: false,
            deadline: nil,
            clearDeadline: false,
            priority: .low,
            clearPriority: false,
            evening: nil,
            setUdas: "{}",
            removeUdaKeys: [],
            addTags: ["shopping"],
            removeTags: [],
            addDeps: [],
            removeDeps: []
        )
        let modified = try await ws.modifyTask(sid: task.sid, patch: patch)
        XCTAssertEqual(modified.title, "Buy more groceries")
        XCTAssertEqual(modified.priority, .low)
        XCTAssertTrue(modified.tags.contains("shopping"))

        let done = try await ws.completeTask(sid: task.sid)
        XCTAssertEqual(done.status, .done)
        XCTAssertNotNil(done.doneAt)
    }

    // MARK: - Projects & areas

    func testProjectAndAreaCrud() async throws {
        let ws = try await TockWorkspace.create(path: makeVaultPath(), password: Self.password)

        let area = try await ws.addArea(TockNewArea(name: "Health", color: "#00FF00"))
        XCTAssertEqual(area.name, "Health")
        let areas = try await ws.listAreas()
        XCTAssertEqual(areas.count, 1)

        let project = try await ws.addProject(
            TockNewProject(name: "Website redesign", notes: nil, areaId: nil, deadline: nil)
        )
        XCTAssertEqual(project.name, "Website redesign")
        let projects = try await ws.listProjects()
        XCTAssertEqual(projects.count, 1)

        let fetched = try await ws.getProject(sid: project.sid)
        XCTAssertEqual(fetched?.name, "Website redesign")
    }

    // MARK: - Tags

    func testTagsList() async throws {
        let ws = try await TockWorkspace.create(path: makeVaultPath(), password: Self.password)
        _ = try await ws.addTask(
            TockNewTask(
                title: "Tagged",
                notes: nil,
                status: nil,
                projectId: nil,
                areaId: nil,
                headingId: nil,
                startDate: nil,
                deadline: nil,
                recurrence: nil,
                priority: nil,
                evening: false,
                udas: "{}",
                tags: ["alpha", "beta"]
            )
        )
        let names = try await ws.listTags().map(\.name)
        XCTAssertTrue(names.contains("alpha"))
        XCTAssertTrue(names.contains("beta"))
    }

    // MARK: - Time tracking

    func testTimeTracking() async throws {
        let ws = try await TockWorkspace.create(path: makeVaultPath(), password: Self.password)

        let block = try await ws.startTimer(
            TockNewTimeBlock(title: "Coding", taskSid: nil, projectId: nil, notes: nil)
        )
        XCTAssertNil(block.endTs)
        let current = try await ws.currentTimer()
        XCTAssertNotNil(current)

        let stopped = try await ws.stopTimer(sid: block.sid)
        XCTAssertNotNil(stopped.endTs)

        let resumed = try await ws.resumeTimer()
        XCTAssertNil(resumed.endTs)
        let blocks = try await ws.listTimeBlocks()
        XCTAssertFalse(blocks.isEmpty)
    }

    // MARK: - Focus sessions

    func testFocusLifecycle() async throws {
        let ws = try await TockWorkspace.create(path: makeVaultPath(), password: Self.password)

        let session = try await ws.startFocus(
            TockNewFocusSession(
                taskSid: nil,
                projectId: nil,
                plannedCycles: 2,
                config: TockFocusConfig(
                    workMinutes: 25,
                    shortBreakMinutes: 5,
                    longBreakMinutes: 15,
                    cyclesBeforeLongBreak: 4
                )
            )
        )
        XCTAssertEqual(session.state, .working)
        let status = try await ws.focusStatus()
        XCTAssertNotNil(status)

        let paused = try await ws.pauseFocus(sid: session.sid)
        XCTAssertEqual(paused.state, .paused)
        let resumed = try await ws.resumeFocus(sid: session.sid)
        XCTAssertEqual(resumed.state, .working)

        let cycle1 = try await ws.completeFocusCycle(sid: session.sid)
        XCTAssertEqual(cycle1.completedCycles, 1)
        XCTAssertEqual(cycle1.state, .shortBreak)

        let skipped = try await ws.skipFocusBreak(sid: session.sid)
        XCTAssertEqual(skipped.state, .working)

        let cycle2 = try await ws.completeFocusCycle(sid: session.sid)
        XCTAssertEqual(cycle2.completedCycles, 2)
        XCTAssertEqual(cycle2.state, .completed)
    }

    // MARK: - Habits

    func testHabitLifecycle() async throws {
        let ws = try await TockWorkspace.create(path: makeVaultPath(), password: Self.password)

        let habit = try await ws.addHabit(
            TockNewHabit(
                title: "Read 10 pages",
                identity: "I am a reader",
                cue: nil,
                craving: nil,
                response: nil,
                reward: nil,
                direction: .build,
                cadence: "\"daily\"",
                minimum: "\"boolean\"",
                stackAfter: nil,
                stackDelayS: 0,
                areaId: nil,
                projectId: nil
            )
        )
        XCTAssertEqual(habit.title, "Read 10 pages")
        let habits = try await ws.listHabits()
        XCTAssertEqual(habits.count, 1)
        let fetchedHabit = try await ws.getHabit(sid: habit.sid)
        XCTAssertNotNil(fetchedHabit)

        let entry = try await ws.logHabit(
            habitSid: habit.sid,
            amount: "1",
            notes: "Great session",
            slip: false
        )
        XCTAssertFalse(entry.slip)
        XCTAssertEqual(entry.notes, "Great session")

        try await ws.archiveHabit(sid: habit.sid)
        let remaining = try await ws.listHabits()
        XCTAssertTrue(remaining.isEmpty)
    }
}
