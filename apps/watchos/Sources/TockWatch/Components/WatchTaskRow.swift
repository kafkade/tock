import SwiftUI
import WatchKit

/// Compact task row for the watch Today list.
///
/// Shows priority dot, title, and deadline. Tap the checkmark
/// button to complete with haptic confirmation.
struct WatchTaskRow: View {
    let task: TaskItem
    var onComplete: (() -> Void)?

    var body: some View {
        HStack(spacing: WatchTheme.Spacing.md) {
            // Completion button
            Button {
                WKInterfaceDevice.current().play(.success)
                onComplete?()
            } label: {
                Image(systemName: "circle")
                    .font(.body)
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
            .frame(width: 24, height: 24)
            .accessibilityLabel("Complete task")
            .accessibilityHint("Marks this task as done")

            VStack(alignment: .leading, spacing: WatchTheme.Spacing.xs) {
                HStack(spacing: WatchTheme.Spacing.sm) {
                    if let priority = task.priority {
                        Circle()
                            .fill(priorityColor(priority))
                            .frame(width: 6, height: 6)
                            .accessibilityLabel(priorityAccessibilityLabel(priority))
                    }

                    Text(task.title)
                        .font(.caption)
                        .lineLimit(2)

                    if task.evening {
                        Image(systemName: "moon.fill")
                            .font(.system(size: 8))
                            .foregroundStyle(.secondary)
                            .accessibilityLabel("Evening task")
                    }
                }

                if let deadline = task.deadline {
                    Text(deadline, style: .date)
                        .font(.caption2)
                        .foregroundStyle(deadline < Date() ? .red : .secondary)
                }
            }
            .accessibilityElement(children: .combine)
        }
    }

    private func priorityAccessibilityLabel(_ priority: Priority) -> String {
        switch priority {
        case .high: "High priority"
        case .medium: "Medium priority"
        case .low: "Low priority"
        }
    }

    private func priorityColor(_ priority: Priority) -> Color {
        switch priority {
        case .high: WatchTheme.Colors.priorityHigh
        case .medium: WatchTheme.Colors.priorityMedium
        case .low: WatchTheme.Colors.priorityLow
        }
    }
}
