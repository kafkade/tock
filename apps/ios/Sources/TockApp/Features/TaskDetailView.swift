import SwiftUI

/// Task detail view — full task information and actions.
struct TaskDetailView: View {
    let task: TaskItem

    var body: some View {
        List {
            Section {
                VStack(alignment: .leading, spacing: TockTheme.Spacing.sm) {
                    HStack {
                        if let priority = task.priority {
                            PriorityBadge(priority: priority)
                        }
                        Text(task.title)
                            .font(.title3)
                            .bold()
                    }

                    HStack(spacing: TockTheme.Spacing.sm) {
                        Label(task.status.rawValue.capitalized, systemImage: statusIcon)
                            .font(.caption)
                            .foregroundStyle(.secondary)

                        Text("SID \(task.sid)")
                            .font(.caption)
                            .foregroundStyle(.tertiary)
                            .monospacedDigit()
                            .accessibilityLabel("Task number \(task.sid)")
                    }
                    .accessibilityElement(children: .combine)
                }
            }

            if let notes = task.notes {
                Section("Notes") {
                    Text(notes)
                        .font(.callout)
                }
            }

            Section("Details") {
                if let deadline = task.deadline {
                    LabeledContent("Deadline") {
                        Text(deadline, style: .date)
                    }
                }

                if let priority = task.priority {
                    LabeledContent("Priority", value: priority.rawValue.capitalized)
                }

                if task.evening {
                    LabeledContent("Evening", value: "Yes")
                }

                LabeledContent("Urgency") {
                    Text(String(format: "%.2f", task.urgency))
                        .monospacedDigit()
                }
            }

            if !task.tags.isEmpty {
                Section("Tags") {
                    FlowLayout(spacing: TockTheme.Spacing.xs) {
                        ForEach(task.tags, id: \.self) { tag in
                            TagChip(name: tag)
                        }
                    }
                }
            }

            Section("Timestamps") {
                LabeledContent("Created", value: task.createdAt, format: .dateTime)

                if let doneAt = task.doneAt {
                    LabeledContent("Completed", value: doneAt, format: .dateTime)
                }
            }
        }
        .navigationTitle("Task \(task.sid)")
        .platformInlineNavigationBarTitle()
    }

    private var statusIcon: String {
        switch task.status {
        case .inbox: "tray"
        case .pending: "circle"
        case .started: "play.circle"
        case .done: "checkmark.circle.fill"
        case .cancelled: "xmark.circle"
        case .someday: "moon.zzz"
        }
    }
}

/// Simple flow layout for tags.
struct FlowLayout: Layout {
    var spacing: CGFloat = 4

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let result = arrange(proposal: proposal, subviews: subviews)
        return result.size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let result = arrange(proposal: proposal, subviews: subviews)
        for (index, position) in result.positions.enumerated() {
            subviews[index].place(
                at: CGPoint(x: bounds.minX + position.x, y: bounds.minY + position.y),
                proposal: .unspecified
            )
        }
    }

    private func arrange(proposal: ProposedViewSize, subviews: Subviews) -> (size: CGSize, positions: [CGPoint]) {
        let maxWidth = proposal.width ?? .infinity
        var positions: [CGPoint] = []
        var x: CGFloat = 0
        var y: CGFloat = 0
        var rowHeight: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x + size.width > maxWidth, x > 0 {
                x = 0
                y += rowHeight + spacing
                rowHeight = 0
            }
            positions.append(CGPoint(x: x, y: y))
            rowHeight = max(rowHeight, size.height)
            x += size.width + spacing
        }

        return (CGSize(width: maxWidth, height: y + rowHeight), positions)
    }
}
