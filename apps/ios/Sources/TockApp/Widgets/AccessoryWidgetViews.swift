#if !os(macOS)
import SwiftUI
import WidgetKit

// MARK: - Habit Accessory View

/// Accessory circular widget showing a habit completion ring.
struct HabitAccessoryView: View {
    let entry: AccessoryWidgetEntry

    var body: some View {
        if entry.snapshot.isVaultLocked {
            LockedWidgetView()
        } else if let habit = entry.snapshot.habits.first {
            habitRing(habit)
                .widgetURL(WidgetDeepLinks.habit(id: habit.id))
        } else {
            taskCountBadge
                .widgetURL(WidgetDeepLinks.today)
        }
    }

    @ViewBuilder
    private func habitRing(_ habit: WidgetHabit) -> some View {
        ZStack {
            AccessoryWidgetBackground()

            // Progress ring
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
}

// MARK: - Status Accessory Views

/// Accessory rectangular widget: next task + due time.
struct StatusRectangularView: View {
    let entry: AccessoryWidgetEntry

    var body: some View {
        if entry.snapshot.isVaultLocked {
            LockedWidgetView()
        } else if let task = entry.snapshot.todayTasks.first {
            VStack(alignment: .leading, spacing: 2) {
                Label("tock", systemImage: "checkmark.seal.fill")
                    .font(.caption2)
                    .foregroundStyle(.secondary)

                Text(task.title)
                    .font(.caption)
                    .bold()
                    .lineLimit(1)

                if let deadline = task.deadline {
                    Text(deadline, style: .relative)
                        .font(.caption2)
                        .foregroundStyle(deadline < Date() ? .red : .secondary)
                } else {
                    Text("No deadline")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
            .widgetURL(WidgetDeepLinks.task(id: task.id))
        } else {
            VStack(alignment: .leading, spacing: 2) {
                Label("tock", systemImage: "checkmark.seal.fill")
                    .font(.caption2)
                Text("All clear ✓")
                    .font(.caption)
                    .bold()
            }
            .widgetURL(WidgetDeepLinks.today)
        }
    }
}

/// Accessory inline widget: "3 due · 🍅 12:34" status line.
struct StatusInlineView: View {
    let entry: AccessoryWidgetEntry

    var body: some View {
        if entry.snapshot.isVaultLocked {
            Text("🔒 Tap to unlock")
        } else {
            statusText
                .widgetURL(WidgetDeepLinks.today)
        }
    }

    @ViewBuilder
    private var statusText: some View {
        let dueCount = entry.snapshot.dueCount
        if let timer = entry.snapshot.activeTimer {
            Text("\(dueCount) due · 🍅 \(timer.startedAt, style: .timer)")
        } else {
            Text("\(dueCount) due today")
        }
    }
}

// MARK: - Accessory Widget Dispatch

/// Routes accessory widget families to the appropriate view.
struct AccessoryWidgetView: View {
    @Environment(\.widgetFamily) var family
    let entry: AccessoryWidgetEntry

    var body: some View {
        switch family {
        case .accessoryCircular:
            HabitAccessoryView(entry: entry)
        case .accessoryRectangular:
            StatusRectangularView(entry: entry)
        case .accessoryInline:
            StatusInlineView(entry: entry)
        default:
            StatusInlineView(entry: entry)
        }
    }
}
#endif
