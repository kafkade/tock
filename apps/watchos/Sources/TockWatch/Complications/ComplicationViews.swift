import SwiftUI
import WidgetKit

// MARK: - Habit Ring (accessoryCircular)

/// Circular complication showing the top habit's streak progress ring.
///
/// Maps to architecture §8.4: `.circularSmall`, `.graphicCircular`,
/// and `.graphicCorner` (habit ring content).
struct HabitRingComplicationView: View {
    let entry: ComplicationEntry

    var body: some View {
        if entry.snapshot.isVaultLocked {
            lockedCircular
        } else if let habit = entry.snapshot.habits.first {
            habitRing(habit)
        } else {
            taskCountBadge
        }
    }

    @ViewBuilder
    private func habitRing(_ habit: ComplicationHabit) -> some View {
        ZStack {
            AccessoryWidgetBackground()

            let progress = Double(habit.streakCurrent) / max(Double(habit.streakBest), 1.0)
            Circle()
                .trim(from: 0, to: min(progress, 1.0))
                .stroke(style: StrokeStyle(lineWidth: 3, lineCap: .round))
                .rotationEffect(.degrees(-90))
                .padding(4)

            VStack(spacing: 0) {
                Text("\(habit.streakCurrent)")
                    .font(.system(size: 16, weight: .bold))
                    .monospacedDigit()
                Text("🔥")
                    .font(.system(size: 8))
            }
        }
    }

    @ViewBuilder
    private var taskCountBadge: some View {
        ZStack {
            AccessoryWidgetBackground()
            VStack(spacing: 0) {
                Text("\(entry.snapshot.todayTasks.count)")
                    .font(.system(size: 20, weight: .bold))
                    .monospacedDigit()
                Text("tasks")
                    .font(.system(size: 8))
            }
        }
    }

    @ViewBuilder
    private var lockedCircular: some View {
        ZStack {
            AccessoryWidgetBackground()
            Image(systemName: "lock.fill")
                .font(.title3)
        }
    }
}

// MARK: - Task List / Timer (accessoryRectangular)

/// Rectangular complication showing next 3 tasks or active timer.
///
/// Maps to architecture §8.4: `.modularLarge`, `.graphicRectangular`,
/// and `.graphicExtraLarge` (Apple Watch Ultra).
struct TaskListComplicationView: View {
    let entry: ComplicationEntry

    var body: some View {
        if entry.snapshot.isVaultLocked {
            lockedRectangular
        } else if let timer = entry.snapshot.activeTimer {
            timerView(timer)
        } else if let focus = entry.snapshot.activeFocus {
            focusView(focus)
        } else {
            taskListView
        }
    }

    @ViewBuilder
    private func timerView(_ timer: ComplicationTimer) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Label("Timer", systemImage: "timer")
                .font(.caption2)
                .foregroundStyle(.secondary)

            Text(timer.title)
                .font(.caption)
                .bold()
                .lineLimit(1)

            Text(timer.startedAt, style: .relative)
                .font(.caption2)
                .monospacedDigit()
                .foregroundStyle(WatchTheme.Colors.timerActive)
        }
    }

    @ViewBuilder
    private func focusView(_ focus: ComplicationFocus) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Label("Focus", systemImage: "brain.head.profile")
                .font(.caption2)
                .foregroundStyle(WatchTheme.Colors.focusWork)

            Text("\(focus.completedCycles)/\(focus.plannedCycles) cycles")
                .font(.caption)
                .bold()

            Text(focusStateLabel(focus.state))
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private var taskListView: some View {
        VStack(alignment: .leading, spacing: 2) {
            Label("tock", systemImage: "checkmark.seal.fill")
                .font(.caption2)
                .foregroundStyle(.secondary)

            let tasks = entry.snapshot.todayTasks.prefix(3)
            if tasks.isEmpty {
                Text("All clear ✓")
                    .font(.caption)
                    .bold()
            } else {
                ForEach(Array(tasks)) { task in
                    HStack(spacing: 3) {
                        if let priority = task.priority {
                            Circle()
                                .fill(priorityColor(priority))
                                .frame(width: 4, height: 4)
                        }
                        Text(task.title)
                            .font(.system(size: 10))
                            .lineLimit(1)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var lockedRectangular: some View {
        VStack(alignment: .leading) {
            Label("tock", systemImage: "lock.fill")
                .font(.caption)
                .bold()
            Text("Tap to unlock vault")
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }

    private func priorityColor(_ priority: Priority) -> Color {
        switch priority {
        case .high: WatchTheme.Colors.priorityHigh
        case .medium: WatchTheme.Colors.priorityMedium
        case .low: WatchTheme.Colors.priorityLow
        }
    }

    private func focusStateLabel(_ state: FocusState) -> String {
        switch state {
        case .working: "Working"
        case .shortBreak: "Short break"
        case .longBreak: "Long break"
        case .paused: "Paused"
        case .aborted: "Aborted"
        case .completed: "Complete"
        }
    }
}

// MARK: - Status Line (accessoryInline)

/// Inline complication showing due count and timer status.
///
/// Maps to architecture §8.4: `.utilitarianSmall`, `.utilitarianLarge`,
/// and `.graphicBezel`.
struct StatusInlineComplicationView: View {
    let entry: ComplicationEntry

    var body: some View {
        if entry.snapshot.isVaultLocked {
            Text("🔒 Tap to unlock")
        } else {
            statusText
        }
    }

    @ViewBuilder
    private var statusText: some View {
        let dueCount = entry.snapshot.dueCount
        if let timer = entry.snapshot.activeTimer {
            Text("\(dueCount) due · 🍅 \(timer.startedAt, style: .timer)")
        } else if let focus = entry.snapshot.activeFocus {
            Text("\(dueCount) due · 🧠 \(focus.completedCycles)/\(focus.plannedCycles)")
        } else {
            Text("\(dueCount) due today")
        }
    }
}

// MARK: - Corner Gauge (accessoryCorner)

/// Corner complication showing a habit gauge along the bezel.
///
/// Maps to architecture §8.4: `.graphicCorner` (habit ring).
struct CornerGaugeComplicationView: View {
    let entry: ComplicationEntry

    var body: some View {
        if entry.snapshot.isVaultLocked {
            lockedCorner
        } else if let habit = entry.snapshot.habits.first {
            habitCorner(habit)
        } else {
            taskCorner
        }
    }

    @ViewBuilder
    private func habitCorner(_ habit: ComplicationHabit) -> some View {
        let progress = Double(habit.streakCurrent) / max(Double(habit.streakBest), 1.0)
        Text("\(habit.streakCurrent)")
            .font(.system(size: 16, weight: .bold))
            .monospacedDigit()
            .widgetCurvesContent()
            .widgetLabel {
                Gauge(value: min(progress, 1.0)) {
                    Text("🔥")
                }
                .gaugeStyle(.accessoryLinear)
            }
    }

    @ViewBuilder
    private var taskCorner: some View {
        Text("\(entry.snapshot.dueCount)")
            .font(.system(size: 16, weight: .bold))
            .monospacedDigit()
            .widgetCurvesContent()
            .widgetLabel {
                Text("due today")
            }
    }

    @ViewBuilder
    private var lockedCorner: some View {
        Image(systemName: "lock.fill")
            .widgetCurvesContent()
            .widgetLabel {
                Text("Locked")
            }
    }
}
