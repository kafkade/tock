import SwiftUI
import WidgetKit

// MARK: - Today Widget Views

/// Today widget view — adapts to all system widget sizes.
///
/// - **Small**: Active timer countdown or next task with deadline.
/// - **Medium**: Today list (3–4 tasks) with interactive checkboxes.
/// - **Large**: Today list (6 tasks) + habit ring strip + timer footer.
/// - **Extra Large**: Two-column (Today + Inbox), habits row, timer summary.
struct TodayWidgetView: View {
    @Environment(\.widgetFamily) var family
    let entry: TodayWidgetEntry

    var body: some View {
        if entry.snapshot.isVaultLocked {
            LockedWidgetView()
        } else {
            switch family {
            case .systemSmall:
                TodaySmallView(snapshot: entry.snapshot)
            case .systemMedium:
                TodayMediumView(snapshot: entry.snapshot)
            case .systemLarge:
                TodayLargeView(snapshot: entry.snapshot)
            case .systemExtraLarge:
                TodayExtraLargeView(snapshot: entry.snapshot)
            default:
                TodaySmallView(snapshot: entry.snapshot)
            }
        }
    }
}

// MARK: - Small

/// Small widget: active timer or next task.
struct TodaySmallView: View {
    let snapshot: WidgetSnapshot

    var body: some View {
        if let timer = snapshot.activeTimer {
            timerView(timer)
        } else if let task = snapshot.todayTasks.first {
            nextTaskView(task)
        } else {
            allClearView
        }
    }

    @ViewBuilder
    private func timerView(_ timer: WidgetTimer) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Label("Timer", systemImage: "timer")
                .font(.caption2)
                .foregroundStyle(.secondary)

            Text(timer.title)
                .font(.headline)
                .lineLimit(2)

            Spacer()

            Text(timer.startedAt, style: .relative)
                .font(.title2)
                .monospacedDigit()
                .foregroundStyle(TockTheme.Colors.timerActive)
        }
        .padding()
        .widgetURL(WidgetDeepLinks.timer)
    }

    @ViewBuilder
    private func nextTaskView(_ task: WidgetTask) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Label("Next", systemImage: "sun.max.fill")
                .font(.caption2)
                .foregroundStyle(.secondary)

            Text(task.title)
                .font(.headline)
                .lineLimit(2)

            Spacer()

            if let deadline = task.deadline {
                Label {
                    Text(deadline, style: .date)
                        .font(.caption)
                } icon: {
                    Image(systemName: "calendar")
                        .font(.caption2)
                }
                .foregroundStyle(deadline < Date() ? .red : .secondary)
            }

            if let priority = task.priority {
                WidgetPriorityBadge(priority: priority)
            }
        }
        .padding()
        .widgetURL(WidgetDeepLinks.task(id: task.id))
    }

    @ViewBuilder
    private var allClearView: some View {
        VStack(spacing: 8) {
            Image(systemName: "sun.max.fill")
                .font(.largeTitle)
                .foregroundStyle(.yellow)
            Text("All clear")
                .font(.headline)
            Text("No tasks for today")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding()
        .widgetURL(WidgetDeepLinks.today)
    }
}

// MARK: - Medium

/// Medium widget: 3–4 tasks with interactive checkboxes.
struct TodayMediumView: View {
    let snapshot: WidgetSnapshot

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Header
            HStack {
                Label("Today", systemImage: "sun.max.fill")
                    .font(.caption)
                    .bold()
                Spacer()
                Text("\(snapshot.todayTasks.count) tasks")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal)
            .padding(.top, 12)
            .padding(.bottom, 6)

            // Task rows
            let visibleTasks = Array(snapshot.todayTasks.prefix(4))
            ForEach(visibleTasks) { task in
                WidgetTaskRow(task: task)
            }

            if snapshot.todayTasks.count > 4 {
                Text("+\(snapshot.todayTasks.count - 4) more")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .padding(.horizontal)
                    .padding(.vertical, 2)
            }

            Spacer(minLength: 0)
        }
        .widgetURL(WidgetDeepLinks.today)
    }
}

// MARK: - Large

/// Large widget: 6 tasks + habit strip + timer footer.
struct TodayLargeView: View {
    let snapshot: WidgetSnapshot

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Header
            HStack {
                Label("Today", systemImage: "sun.max.fill")
                    .font(.caption)
                    .bold()
                Spacer()
                if let timer = snapshot.activeTimer {
                    Label {
                        Text(timer.startedAt, style: .relative)
                            .monospacedDigit()
                    } icon: {
                        Image(systemName: "timer")
                    }
                    .font(.caption2)
                    .foregroundStyle(TockTheme.Colors.timerActive)
                }
            }
            .padding(.horizontal)
            .padding(.top, 12)
            .padding(.bottom, 4)

            // Tasks
            let visibleTasks = Array(snapshot.todayTasks.prefix(6))
            ForEach(visibleTasks) { task in
                WidgetTaskRow(task: task)
            }

            Spacer(minLength: 4)

            // Habit strip
            if !snapshot.habits.isEmpty {
                Divider().padding(.horizontal)
                HStack(spacing: 12) {
                    ForEach(snapshot.habits.prefix(4)) { habit in
                        WidgetHabitChip(habit: habit)
                    }
                    Spacer()
                }
                .padding(.horizontal)
                .padding(.vertical, 6)
            }

            // Timer footer
            if let timer = snapshot.activeTimer {
                Divider().padding(.horizontal)
                HStack(spacing: 6) {
                    Circle()
                        .fill(TockTheme.Colors.timerActive)
                        .frame(width: 6, height: 6)
                    Text(timer.title)
                        .font(.caption2)
                        .lineLimit(1)
                    Spacer()
                    Text(timer.startedAt, style: .relative)
                        .font(.caption2)
                        .monospacedDigit()
                        .foregroundStyle(TockTheme.Colors.timerActive)
                }
                .padding(.horizontal)
                .padding(.vertical, 6)
            }
        }
        .widgetURL(WidgetDeepLinks.today)
    }
}

// MARK: - Extra Large (iPadOS)

/// Extra-large widget (iPadOS): two-column Today + Inbox, habits, timer.
struct TodayExtraLargeView: View {
    let snapshot: WidgetSnapshot

    var body: some View {
        VStack(spacing: 0) {
            // Two-column task lists
            HStack(alignment: .top, spacing: 16) {
                // Today column
                VStack(alignment: .leading, spacing: 0) {
                    Label("Today", systemImage: "sun.max.fill")
                        .font(.caption)
                        .bold()
                        .padding(.bottom, 4)

                    ForEach(snapshot.todayTasks.prefix(5)) { task in
                        WidgetTaskRow(task: task)
                    }
                }

                Divider()

                // Inbox column
                VStack(alignment: .leading, spacing: 0) {
                    Label("Inbox", systemImage: "tray.fill")
                        .font(.caption)
                        .bold()
                        .padding(.bottom, 4)

                    if snapshot.inboxTasks.isEmpty {
                        Text("Inbox zero ✓")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .padding(.top, 8)
                    } else {
                        ForEach(snapshot.inboxTasks.prefix(5)) { task in
                            WidgetTaskRow(task: task)
                        }
                    }
                }
            }
            .padding(.horizontal)
            .padding(.top, 12)

            Spacer(minLength: 4)

            // Habits row
            if !snapshot.habits.isEmpty {
                Divider().padding(.horizontal)
                HStack(spacing: 16) {
                    ForEach(snapshot.habits) { habit in
                        WidgetHabitChip(habit: habit)
                    }
                    Spacer()
                }
                .padding(.horizontal)
                .padding(.vertical, 6)
            }

            // Timer footer
            if let timer = snapshot.activeTimer {
                Divider().padding(.horizontal)
                HStack(spacing: 6) {
                    Circle()
                        .fill(TockTheme.Colors.timerActive)
                        .frame(width: 6, height: 6)
                    Text(timer.title)
                        .font(.caption)
                        .lineLimit(1)
                    Spacer()
                    Text(timer.startedAt, style: .relative)
                        .font(.caption)
                        .monospacedDigit()
                        .foregroundStyle(TockTheme.Colors.timerActive)
                }
                .padding(.horizontal)
                .padding(.vertical, 8)
            }
        }
        .widgetURL(WidgetDeepLinks.today)
    }
}

// MARK: - Shared widget components

/// Single task row for medium/large widgets with interactive checkbox.
struct WidgetTaskRow: View {
    let task: WidgetTask

    var body: some View {
        Link(destination: WidgetDeepLinks.task(id: task.id)) {
            HStack(spacing: 6) {
                // Interactive checkbox (iOS 17+)
                if #available(iOS 17.0, *) {
                    Button(intent: CompleteTaskIntent(task: TaskEntity(id: task.id, title: task.title))) {
                        Image(systemName: "circle")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                } else {
                    Image(systemName: "circle")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                if let priority = task.priority {
                    WidgetPriorityBadge(priority: priority)
                }

                Text(task.title)
                    .font(.caption)
                    .lineLimit(1)

                if task.evening {
                    Image(systemName: "moon.fill")
                        .font(.system(size: 8))
                        .foregroundStyle(.secondary)
                }

                Spacer()

                if let deadline = task.deadline {
                    Text(deadline, style: .date)
                        .font(.system(size: 9))
                        .foregroundStyle(deadline < Date() ? .red : .secondary)
                }
            }
            .padding(.horizontal)
            .padding(.vertical, 3)
        }
    }
}

/// Priority indicator dot for widgets.
struct WidgetPriorityBadge: View {
    let priority: Priority

    var body: some View {
        Circle()
            .fill(color)
            .frame(width: 6, height: 6)
    }

    private var color: Color {
        switch priority {
        case .high: TockTheme.Colors.priorityHigh
        case .medium: TockTheme.Colors.priorityMedium
        case .low: TockTheme.Colors.priorityLow
        }
    }
}

/// Habit chip for large/extra-large widget habit strip.
struct WidgetHabitChip: View {
    let habit: WidgetHabit

    var body: some View {
        if #available(iOS 17.0, *) {
            Button(intent: LogHabitIntent(habit: HabitEntity(id: habit.id, title: habit.title))) {
                chipContent
            }
            .buttonStyle(.plain)
        } else {
            chipContent
        }
    }

    @ViewBuilder
    private var chipContent: some View {
        HStack(spacing: 3) {
            Image(systemName: habit.direction == .build
                ? "checkmark.circle" : "xmark.circle")
                .font(.system(size: 10))
            Text(habit.title)
                .font(.system(size: 10))
                .lineLimit(1)
            Text("🔥\(habit.streakCurrent)")
                .font(.system(size: 9))
        }
        .foregroundStyle(
            habit.direction == .build
                ? TockTheme.Colors.habitBuild
                : TockTheme.Colors.habitBreak
        )
    }
}

// MARK: - Locked widget

/// Displayed when the vault is locked. Tapping opens the app to unlock.
struct LockedWidgetView: View {
    @Environment(\.widgetFamily) var family

    var body: some View {
        switch family {
        #if !os(macOS)
        case .accessoryInline:
            Text("🔒 Tap to unlock")
        case .accessoryCircular:
            ZStack {
                AccessoryWidgetBackground()
                Image(systemName: "lock.fill")
                    .font(.title3)
            }
        case .accessoryRectangular:
            VStack(alignment: .leading) {
                Label("tock", systemImage: "lock.fill")
                    .font(.caption)
                    .bold()
                Text("Tap to unlock vault")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
        #endif
        default:
            VStack(spacing: 8) {
                Image(systemName: "lock.fill")
                    .font(.title)
                    .foregroundStyle(.secondary)
                Text("Tap to unlock")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}
