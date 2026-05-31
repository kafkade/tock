import SwiftUI

/// Sheet for quickly adding a new task.
///
/// macOS-adapted version: uses native macOS form style, no iOS-specific
/// modifiers like `.textInputAutocapitalization`.
struct QuickAddSheet: View {
    @Environment(AppSessionState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    @State private var title = ""
    @State private var notes = ""
    @State private var priority: Priority?
    @State private var deadline: Date?
    @State private var hasDeadline = false
    @State private var isEvening = false
    @State private var tagText = ""
    @State private var isSubmitting = false

    var body: some View {
        VStack(spacing: 0) {
            Form {
                Section {
                    TextField("Task title", text: $title)
                        .font(.headline)

                    TextField("Notes (optional)", text: $notes, axis: .vertical)
                        .lineLimit(3...6)
                }

                Section("Details") {
                    Picker("Priority", selection: $priority) {
                        Text("None").tag(nil as Priority?)
                        Text("Low").tag(Priority.low as Priority?)
                        Text("Medium").tag(Priority.medium as Priority?)
                        Text("High").tag(Priority.high as Priority?)
                    }

                    Toggle("Evening", isOn: $isEvening)

                    Toggle("Deadline", isOn: $hasDeadline)
                    if hasDeadline {
                        DatePicker(
                            "Due date",
                            selection: Binding(
                                get: { deadline ?? Date() },
                                set: { deadline = $0 }
                            ),
                            displayedComponents: .date
                        )
                    }
                }

                Section("Tags") {
                    TextField("Add tags (comma-separated)", text: $tagText)
                }
            }
            .formStyle(.grouped)
            .frame(minWidth: 400, minHeight: 300)

            // Bottom button bar
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Add") {
                    Task { await submit() }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(title.trimmingCharacters(in: .whitespaces).isEmpty || isSubmitting)
                .buttonStyle(.borderedProminent)
                .accessibilityHint(
                    title.trimmingCharacters(in: .whitespaces).isEmpty
                        ? "Enter a task title to add" : "Adds the task"
                )
            }
            .padding()
        }
    }

    private func submit() async {
        isSubmitting = true
        let tags = tagText.split(separator: ",")
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }

        let input = NewTaskInput(
            title: title.trimmingCharacters(in: .whitespaces),
            notes: notes.isEmpty ? nil : notes,
            deadline: hasDeadline ? deadline : nil,
            priority: priority,
            evening: isEvening,
            tags: tags
        )
        _ = try? await appState.client.addTask(input)
        await appState.refreshMenuBarState()
        dismiss()
    }
}
