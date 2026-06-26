import AppIntents
import Foundation

// MARK: - Start Timer Intent

/// Siri: "Start timer on review PR"
///
/// Starts a time-tracking timer with an optional task reference and note.
/// If a task is specified, the timer is linked to that task.
struct StartTimerIntent: AppIntent {
    static var title: LocalizedStringResource = "Start Timer"
    static var description: IntentDescription = "Starts a time-tracking timer in tock."
    static var openAppWhenRun = false

    @Parameter(title: "Task")
    var task: TaskEntity?

    @Parameter(title: "Note")
    var note: String?

    func perform() async throws -> some IntentResult & ProvidesDialog {
        let client = try await VaultGateway.shared.client()
        let title = note ?? task?.title ?? "Untitled"
        let block = try await client.startTimer(title: title, taskId: task?.id)
        await WidgetSnapshotWriter.publish(from: client)

        var message = "Timer started"
        if let taskTitle = task?.title {
            message += " on '\(taskTitle)'"
        } else if let note {
            message += ": \(note)"
        }
        _ = block
        return .result(dialog: "\(message) ⏱")
    }
}

// MARK: - Stop Timer Intent

/// Siri: "Stop the timer"
///
/// Stops the currently running time-tracking timer.
struct StopTimerIntent: AppIntent {
    static var title: LocalizedStringResource = "Stop Timer"
    static var description: IntentDescription = "Stops the running timer in tock."
    static var openAppWhenRun = false

    func perform() async throws -> some IntentResult & ProvidesDialog {
        let client = try await VaultGateway.shared.client()
        if let block = try await client.stopTimer() {
            await WidgetSnapshotWriter.publish(from: client)
            let duration = block.duration
            let minutes = Int(duration / 60)
            let seconds = Int(duration.truncatingRemainder(dividingBy: 60))
            return .result(dialog: "Timer stopped — \(minutes)m \(seconds)s logged ⏱")
        } else {
            return .result(dialog: "No timer is running.")
        }
    }
}

// MARK: - Start Focus Intent

/// Siri: "Focus for 25 minutes on writing"
///
/// Starts a Pomodoro focus session. The `length` parameter is in minutes
/// and is converted to cycles (`cycles = max(1, length / workMinutes)`).
struct StartFocusIntent: AppIntent {
    static var title: LocalizedStringResource = "Start Focus Session"
    static var description: IntentDescription = "Starts a Pomodoro focus session in tock."
    static var openAppWhenRun = false

    @Parameter(title: "Task")
    var task: TaskEntity?

    @Parameter(title: "Length (minutes)", default: 25)
    var length: Int

    func perform() async throws -> some IntentResult & ProvidesDialog {
        let client = try await VaultGateway.shared.client()
        let workMinutes: UInt32 = 25
        let cycles = max(1, UInt32(length) / workMinutes)
        let session = try await client.startFocus(taskId: task?.id, cycles: cycles)
        await WidgetSnapshotWriter.publish(from: client)

        var message = "Focus session started — \(session.plannedCycles) cycle"
        if session.plannedCycles != 1 { message += "s" }
        message += " (\(length) min)"
        if let taskTitle = task?.title {
            message += " on '\(taskTitle)'"
        }
        return .result(dialog: "\(message) 🍅")
    }
}
