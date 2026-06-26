import Foundation
import WidgetKit

// MARK: - App Group snapshot file location

enum WidgetSnapshotFile {
    static let name = "widget-snapshot.json"

    static var url: URL? {
        AppGroup.containerURL()?.appendingPathComponent(name)
    }
}

// MARK: - Reader (used by widget timeline providers)

/// Reads the widget snapshot the main app writes into the App Group container.
///
/// Widgets run frequently in a sandboxed, no-UI process and therefore must
/// **never** open the encrypted vault directly (it would require a biometric
/// prompt). Instead the main app publishes a plaintext-safe projection — only
/// the fields the widgets render — into the shared container after every data
/// change, and the widgets read that file here.
struct AppGroupWidgetSnapshotStore: WidgetSnapshotStore {
    static let shared = AppGroupWidgetSnapshotStore()

    func loadSnapshot() async -> WidgetSnapshot {
        guard
            let url = WidgetSnapshotFile.url,
            let data = try? Data(contentsOf: url),
            let snapshot = try? JSONDecoder.tockWidget.decode(WidgetSnapshot.self, from: data)
        else {
            return .lockedPlaceholder
        }
        return snapshot
    }
}

extension WidgetSnapshot {
    /// Shown when no snapshot has been written yet or the vault is locked.
    static let lockedPlaceholder = WidgetSnapshot(
        todayTasks: [], inboxTasks: [], habits: [],
        activeTimer: nil, activeFocus: nil,
        isVaultLocked: true, dueCount: 0
    )
}

// MARK: - Writer (used by the main app)

/// Publishes the current vault state into the App Group container so widgets
/// can render real data without opening the vault.
enum WidgetSnapshotWriter {

    /// Build a snapshot from the live client and persist it, then ask
    /// WidgetKit to refresh its timelines.
    static func publish(from client: any CoreClient) async {
        let snapshot: WidgetSnapshot
        do {
            snapshot = try await buildSnapshot(from: client)
        } catch {
            return
        }
        write(snapshot)
    }

    /// Publish an explicitly locked snapshot (e.g. when the app locks).
    static func publishLocked() {
        write(.lockedPlaceholder)
    }

    private static func write(_ snapshot: WidgetSnapshot) {
        guard
            let url = WidgetSnapshotFile.url,
            let data = try? JSONEncoder.tockWidget.encode(snapshot)
        else { return }
        try? data.write(to: url, options: .atomic)
        WidgetCenter.shared.reloadAllTimelines()
    }

    private static func buildSnapshot(from client: any CoreClient) async throws -> WidgetSnapshot {
        let today = try await client.listTasks(filter: .today)
        let inbox = try await client.listTasks(filter: .inbox)
        let habits = try await client.listHabits()
        let timer = try await client.currentTimer()
        let focus = try await client.focusStatus()

        let dueCount = today.filter { task in
            guard let deadline = task.deadline else { return false }
            return deadline <= Calendar.current.startOfDay(for: Date()).addingTimeInterval(86_400)
        }.count

        return WidgetSnapshot(
            todayTasks: today.map(WidgetTask.init(from:)),
            inboxTasks: inbox.map(WidgetTask.init(from:)),
            habits: habits.map(WidgetHabit.init(from:)),
            activeTimer: timer.map(WidgetTimer.init(from:)),
            activeFocus: focus.map(WidgetFocus.init(from:)),
            isVaultLocked: false,
            dueCount: dueCount
        )
    }
}

// MARK: - Model → widget projections

private extension WidgetTask {
    init(from task: TaskItem) {
        self.init(
            id: task.id, sid: task.sid, title: task.title,
            priority: task.priority, deadline: task.deadline,
            evening: task.evening, urgency: task.urgency
        )
    }
}

private extension WidgetHabit {
    init(from habit: HabitItem) {
        self.init(
            id: habit.id, title: habit.title, direction: habit.direction,
            streakCurrent: habit.streakCurrent, streakBest: habit.streakBest,
            level: habit.level
        )
    }
}

private extension WidgetTimer {
    init(from block: TimeBlockItem) {
        self.init(title: block.title, startedAt: block.startedAt, taskId: block.taskId)
    }
}

private extension WidgetFocus {
    init(from session: FocusSessionItem) {
        self.init(
            completedCycles: session.completedCycles,
            plannedCycles: session.plannedCycles,
            state: session.state,
            workMinutes: session.workMinutes,
            taskId: session.taskId
        )
    }
}

// MARK: - JSON coders

private extension JSONEncoder {
    static let tockWidget: JSONEncoder = {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        return encoder
    }()
}

private extension JSONDecoder {
    static let tockWidget: JSONDecoder = {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }()
}
