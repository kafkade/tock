import AppIntents

/// Provides pre-built shortcuts that appear in the Shortcuts app
/// and Siri suggestions. Users can install these with one tap.
///
/// Covers the core productivity workflows: task capture, timer control,
/// habit logging, daily review, and focus sessions.
struct TockShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        // Quick capture to inbox
        AppShortcut(
            intent: CaptureToInboxIntent(),
            phrases: [
                "Capture \(\.$text) to \(.applicationName) inbox",
                "Add \(\.$text) to \(.applicationName)",
                "Quick add \(\.$text) in \(.applicationName)",
            ],
            shortTitle: "Capture to Inbox",
            systemImageName: "tray.and.arrow.down"
        )

        // Add task with details
        AppShortcut(
            intent: AddTaskIntent(),
            phrases: [
                "Add task \(\.$taskTitle) to \(.applicationName)",
                "Create task \(\.$taskTitle) in \(.applicationName)",
                "New task \(\.$taskTitle) in \(.applicationName)",
            ],
            shortTitle: "Add Task",
            systemImageName: "plus.circle"
        )

        // Show today's agenda
        AppShortcut(
            intent: ShowTodayIntent(),
            phrases: [
                "Show today in \(.applicationName)",
                "What's on today in \(.applicationName)",
                "Open \(.applicationName) today",
            ],
            shortTitle: "Show Today",
            systemImageName: "sun.max"
        )

        // Start timer
        AppShortcut(
            intent: StartTimerIntent(),
            phrases: [
                "Start timer in \(.applicationName)",
                "Start tracking time in \(.applicationName)",
                "Track time in \(.applicationName)",
            ],
            shortTitle: "Start Timer",
            systemImageName: "timer"
        )

        // Start focus session
        AppShortcut(
            intent: StartFocusIntent(),
            phrases: [
                "Focus for \(\.$length) minutes in \(.applicationName)",
                "Start focus in \(.applicationName)",
                "Pomodoro in \(.applicationName)",
            ],
            shortTitle: "Start Focus",
            systemImageName: "brain.head.profile"
        )

        // Log habit
        AppShortcut(
            intent: LogHabitIntent(),
            phrases: [
                "Log \(\.$habit) in \(.applicationName)",
                "Record \(\.$habit) in \(.applicationName)",
                "Track \(\.$habit) in \(.applicationName)",
            ],
            shortTitle: "Log Habit",
            systemImageName: "flame"
        )
    }
}
