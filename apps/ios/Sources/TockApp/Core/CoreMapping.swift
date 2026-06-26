import Foundation
import TockSwift

// Converts the UniFFI-generated `Tock*` records/enums (from `TockSwift`) into
// the app's UI-facing `Models` types, and app inputs back into `TockNew*`
// records. Centralising the bridging keeps `TockCoreClient` small and makes
// the date/enum handling testable in one place.

// MARK: - Date bridging

enum TockDate {

    /// RFC 3339 parser tolerant of fractional seconds (the core emits both
    /// `...Z` and `...­.SSSZ`).
    private static let withFraction: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    private static let plain: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()

    /// Parse an RFC 3339 timestamp emitted by the core.
    static func parse(_ string: String) -> Date? {
        withFraction.date(from: string) ?? plain.date(from: string)
    }

    /// Parse an optional timestamp.
    static func parse(_ string: String?) -> Date? {
        guard let string else { return nil }
        return parse(string)
    }

    /// Format a `Date` as an RFC 3339 timestamp for the core.
    static func format(_ date: Date) -> String {
        plain.string(from: date)
    }

    /// Format an optional date.
    static func format(_ date: Date?) -> String? {
        guard let date else { return nil }
        return format(date)
    }
}

// MARK: - Enum bridging

extension TockTaskStatus {
    var asModel: TaskStatus {
        switch self {
        case .inbox: return .inbox
        case .pending: return .pending
        case .started: return .started
        case .done: return .done
        case .cancelled: return .cancelled
        case .someday: return .someday
        }
    }
}

extension TaskStatus {
    var asTock: TockTaskStatus {
        switch self {
        case .inbox: return .inbox
        case .pending: return .pending
        case .started: return .started
        case .done: return .done
        case .cancelled: return .cancelled
        case .someday: return .someday
        }
    }
}

extension TockPriority {
    var asModel: Priority {
        switch self {
        case .low: return .low
        case .medium: return .medium
        case .high: return .high
        }
    }
}

extension Priority {
    var asTock: TockPriority {
        switch self {
        case .low: return .low
        case .medium: return .medium
        case .high: return .high
        }
    }
}

extension TockProjectStatus {
    var asModel: ProjectStatus {
        switch self {
        case .active: return .active
        case .paused: return .paused
        case .someday: return .someday
        case .done: return .done
        case .cancelled: return .cancelled
        }
    }
}

extension TockFocusState {
    var asModel: FocusState {
        switch self {
        case .working: return .working
        case .shortBreak: return .shortBreak
        case .longBreak: return .longBreak
        case .paused: return .paused
        case .aborted: return .aborted
        case .completed: return .completed
        }
    }
}

extension TockHabitDirection {
    var asModel: HabitDirection {
        self == .build ? .build : .breakHabit
    }
}

// MARK: - Record bridging

extension TockTask {
    var asModel: TaskItem {
        TaskItem(
            id: id,
            sid: sid,
            title: title,
            notes: notes,
            status: status.asModel,
            projectId: projectId,
            deadline: TockDate.parse(deadline),
            priority: priority?.asModel,
            evening: evening,
            tags: tags,
            dependsOn: dependsOn,
            urgency: urgency,
            createdAt: TockDate.parse(createdAt) ?? Date(),
            modifiedAt: TockDate.parse(modifiedAt) ?? Date(),
            doneAt: TockDate.parse(doneAt)
        )
    }
}

extension TockProject {
    var asModel: ProjectItem {
        ProjectItem(
            id: id,
            sid: sid,
            name: name,
            notes: notes,
            status: status.asModel,
            areaId: areaId,
            deadline: TockDate.parse(deadline),
            createdAt: TockDate.parse(createdAt) ?? Date(),
            modifiedAt: TockDate.parse(modifiedAt) ?? Date()
        )
    }
}

extension TockArea {
    var asModel: AreaItem {
        AreaItem(
            id: id,
            name: name,
            color: color,
            createdAt: TockDate.parse(createdAt) ?? Date()
        )
    }
}

extension TockTag {
    var asModel: TagItem {
        TagItem(id: id, name: name, color: color)
    }
}

extension TockTimeBlock {
    var asModel: TimeBlockItem {
        TimeBlockItem(
            id: id,
            sid: sid,
            title: title,
            startedAt: TockDate.parse(startTs) ?? Date(),
            endedAt: TockDate.parse(endTs),
            taskId: taskId,
            projectId: projectId,
            notes: notes,
            billable: billable
        )
    }
}

extension TockFocusSession {
    var asModel: FocusSessionItem {
        FocusSessionItem(
            id: id,
            sid: sid,
            startedAt: TockDate.parse(startedAt) ?? Date(),
            endedAt: TockDate.parse(endedAt),
            taskId: taskId,
            projectId: projectId,
            plannedCycles: plannedCycles,
            completedCycles: completedCycles,
            state: state.asModel,
            workMinutes: config.workMinutes,
            shortBreakMinutes: config.shortBreakMinutes,
            longBreakMinutes: config.longBreakMinutes
        )
    }
}

extension TockHabit {
    var asModel: HabitItem {
        HabitItem(
            id: id,
            sid: sid,
            title: title,
            identity: identity,
            direction: direction.asModel,
            level: level,
            xp: xp,
            streakCurrent: streakCurrent,
            streakBest: streakBest,
            levelName: levelName,
            areaId: areaId,
            createdAt: TockDate.parse(createdAt) ?? Date(),
            archivedAt: TockDate.parse(archivedAt)
        )
    }
}

extension TockHabitEntry {
    var asModel: HabitEntryItem {
        HabitEntryItem(
            id: id,
            habitId: habitId,
            occurredAt: TockDate.parse(occurredAt) ?? Date(),
            notes: notes,
            slip: slip
        )
    }
}

// MARK: - Default payloads

enum TockDefaults {
    /// JSON payload for a daily habit cadence (matches the CLI default).
    static let dailyCadence = "\"daily\""
    /// JSON payload for a boolean habit minimum (matches the CLI default).
    static let booleanMinimum = "\"boolean\""
    /// Default amount for a habit log entry (matches the CLI default).
    static let booleanAmount = "true"

    /// Standard 25/5/15 Pomodoro configuration.
    static var focusConfig: TockFocusConfig {
        TockFocusConfig(
            workMinutes: 25,
            shortBreakMinutes: 5,
            longBreakMinutes: 15,
            cyclesBeforeLongBreak: 4
        )
    }
}

// MARK: - Task filtering

extension Array where Element == TaskItem {

    /// Apply a `TaskFilter` to a full task list.
    ///
    /// The UniFFI surface only exposes "list all tasks", so the view-specific
    /// filtering that the CLI performs with its query DSL is reproduced here.
    /// The rules mirror the previous mock client so the UI behaves identically.
    func applying(_ taskFilter: TaskFilter) -> [TaskItem] {
        switch taskFilter {
        case .today:
            return filter { $0.status == .started || $0.status == .pending }
                .filter { task in
                    if let deadline = task.deadline {
                        return deadline <= Calendar.current.startOfDay(
                            for: Calendar.current.date(byAdding: .day, value: 1, to: Date()) ?? Date()
                        ) || task.priority == .high
                    }
                    return task.priority == .high
                }
        case .inbox:
            return self.filter { $0.status == .inbox }
        case .upcoming:
            return self.filter { $0.deadline != nil && $0.status != .done && $0.status != .cancelled }
        case .anytime:
            return self.filter { $0.status == .pending || $0.status == .started }
        case .someday:
            return self.filter { $0.status == .someday }
        case .logbook:
            return self.filter { $0.status == .done || $0.status == .cancelled }
        case .project(let id):
            return self.filter { $0.projectId == id }
        case .all:
            return self
        }
    }
}
