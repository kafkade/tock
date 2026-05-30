import SwiftUI

/// A single task row for list views.
///
/// Shows priority indicator, title, tags, deadline, and urgency.
/// Swipe actions for complete and delete.
struct TaskRow: View {
    let task: TaskItem
    var onComplete: (() -> Void)?
    var onDelete: (() -> Void)?

    var body: some View {
        HStack(spacing: TockTheme.Spacing.sm) {
            // Priority indicator
            if let priority = task.priority {
                PriorityBadge(priority: priority)
            }

            VStack(alignment: .leading, spacing: TockTheme.Spacing.xxs) {
                HStack {
                    Text(task.title)
                        .font(.body)
                        .strikethrough(task.status == .done || task.status == .cancelled)
                        .foregroundStyle(
                            task.status == .done || task.status == .cancelled
                                ? .secondary : .primary
                        )

                    if task.evening {
                        Image(systemName: "moon.fill")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }

                HStack(spacing: TockTheme.Spacing.xs) {
                    if let deadline = task.deadline {
                        DeadlineLabel(date: deadline)
                    }

                    if !task.tags.isEmpty {
                        HStack(spacing: TockTheme.Spacing.xxs) {
                            ForEach(task.tags.prefix(3), id: \.self) { tag in
                                TagChip(name: tag)
                            }
                            if task.tags.count > 3 {
                                Text("+\(task.tags.count - 3)")
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                }
            }

            Spacer()

            Text(String(format: "%.1f", task.urgency))
                .font(.caption)
                .foregroundStyle(.secondary)
                .monospacedDigit()
        }
        .padding(.vertical, TockTheme.Spacing.xxs)
        .swipeActions(edge: .leading) {
            if task.status != .done {
                Button {
                    onComplete?()
                } label: {
                    Label("Done", systemImage: "checkmark")
                }
                .tint(TockTheme.Colors.success)
            }
        }
        .swipeActions(edge: .trailing, allowsFullSwipe: false) {
            Button(role: .destructive) {
                onDelete?()
            } label: {
                Label("Delete", systemImage: "trash")
            }
        }
    }
}

/// Deadline label with color coding.
private struct DeadlineLabel: View {
    let date: Date

    private var isOverdue: Bool {
        date < Date()
    }

    private var isToday: Bool {
        Calendar.current.isDateInToday(date)
    }

    var body: some View {
        Label {
            Text(date, style: .date)
                .font(.caption)
        } icon: {
            Image(systemName: "calendar")
                .font(.caption2)
        }
        .foregroundStyle(isOverdue ? .red : isToday ? .orange : .secondary)
    }
}
