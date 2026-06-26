import SwiftUI

// MARK: - Share Extension View

/// Share extension capture form — the main UI for quick task capture.
///
/// Matches the architecture.md §8.7 wireframe: editable title/notes,
/// project picker, tags, priority, and a split-button destination chooser.
/// Pre-filled from `ShareContent` extracted from the shared items.
///
/// **Note:** This view lives in the main app target for development.
/// When moved to a real share extension target, it will be the principal
/// view presented by the extension's view controller.
struct ShareExtensionView: View {
    let content: ShareContent
    let onDismiss: () -> Void

    @State private var title: String
    @State private var notes: String
    @State private var destination: QuickCaptureDestination = .inbox
    @State private var selectedProjectId: String?
    @State private var tagText = ""
    @State private var priority: Priority?
    @State private var isSubmitting = false
    @State private var projects: [ProjectItem] = []
    @State private var availability: CaptureAvailability = .available

    private let store = SharePendingStore.shared
    private let vaultAccess: any ShareVaultAccessChecking

    init(
        content: ShareContent,
        vaultAccess: any ShareVaultAccessChecking = AppGroupShareVaultAccess(),
        onDismiss: @escaping () -> Void
    ) {
        self.content = content
        self.vaultAccess = vaultAccess
        self.onDismiss = onDismiss
        self._title = State(initialValue: content.title)
        self._notes = State(initialValue: content.notes ?? "")
    }

    var body: some View {
        NavigationStack {
            Group {
                switch availability {
                case .available:
                    captureForm
                case .vaultLocked:
                    vaultLockedView
                case .unavailable(let reason):
                    unavailableView(reason: reason)
                }
            }
            .navigationTitle("Add to Tock")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { onDismiss() }
                }

                if case .available = availability {
                    ToolbarItem(placement: .confirmationAction) {
                        addButton
                    }
                }
            }
        }
        .task {
            availability = await vaultAccess.captureAvailability()
            if let client = try? await VaultGateway.shared.client() {
                projects = (try? await client.listProjects()) ?? []
            }
        }
    }

    // MARK: - Capture Form

    @ViewBuilder
    private var captureForm: some View {
        Form {
            // Title and notes
            Section {
                TextField("Task title", text: $title)
                    .font(.headline)

                TextField("Notes (optional)", text: $notes, axis: .vertical)
                    .lineLimit(3...8)
            }

            // Shared URL display
            if let url = content.url {
                Section("Source") {
                    Link(destination: url) {
                        Label(url.absoluteString, systemImage: "link")
                            .font(.caption)
                            .lineLimit(2)
                            .foregroundStyle(.secondary)
                    }
                }
            }

            // Attachment indicator
            if content.kind == .image || content.kind == .file {
                Section("Attachment") {
                    Label {
                        Text(content.attachmentName ?? "Attached \(content.kind.rawValue)")
                            .foregroundStyle(.secondary)
                    } icon: {
                        Image(systemName: content.kind == .image ? "photo" : "doc")
                    }
                    .font(.caption)

                    Text("Attachments will be saved when vault support is available.")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
            }

            // Project and metadata
            Section("Details") {
                Picker("Project", selection: $selectedProjectId) {
                    Text("Inbox (no project)").tag(nil as String?)
                    ForEach(projects) { project in
                        Text(project.name).tag(project.id as String?)
                    }
                }

                Picker("Priority", selection: $priority) {
                    Text("None").tag(nil as Priority?)
                    Text("Low").tag(Priority.low as Priority?)
                    Text("Medium").tag(Priority.medium as Priority?)
                    Text("High").tag(Priority.high as Priority?)
                }

                TextField("Tags (comma-separated)", text: $tagText)
                    .textInputAutocapitalization(.never)
            }

            // Destination
            Section("Destination") {
                Picker("Add to", selection: $destination) {
                    ForEach(QuickCaptureDestination.allCases) { dest in
                        Label(dest.label, systemImage: dest.icon).tag(dest)
                    }
                }
                .pickerStyle(.menu)
            }
        }
    }

    // MARK: - Add Button (Split Button)

    @ViewBuilder
    private var addButton: some View {
        Menu {
            ForEach(QuickCaptureDestination.allCases) { dest in
                Button {
                    destination = dest
                    Task { await submit() }
                } label: {
                    Label("Add to \(dest.label)", systemImage: dest.icon)
                }
            }
        } label: {
            Text("Add to \(destination.label)")
                .bold()
        } primaryAction: {
            Task { await submit() }
        }
        .disabled(title.trimmingCharacters(in: .whitespaces).isEmpty || isSubmitting)
    }

    // MARK: - Vault Locked

    @ViewBuilder
    private var vaultLockedView: some View {
        ContentUnavailableView {
            Label("Vault Locked", systemImage: "lock.fill")
        } description: {
            Text("Open tock to unlock your vault before capturing tasks.")
        } actions: {
            Button("Open Tock") {
                #if os(iOS)
                if let url = URL(string: "tock://unlock") {
                    UIApplication.shared.open(url)
                }
                #endif
                onDismiss()
            }
            .buttonStyle(.borderedProminent)
        }
    }

    // MARK: - Unavailable

    @ViewBuilder
    private func unavailableView(reason: String) -> some View {
        ContentUnavailableView {
            Label("Capture Unavailable", systemImage: "exclamationmark.triangle")
        } description: {
            Text(reason)
        }
    }

    // MARK: - Submit

    private func submit() async {
        let trimmedTitle = title.trimmingCharacters(in: .whitespaces)
        guard !trimmedTitle.isEmpty else { return }
        isSubmitting = true

        let tags = tagText.split(separator: ",")
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }

        store.saveCapture(
            content: content,
            destination: destination,
            projectId: selectedProjectId,
            tags: tags,
            priority: priority,
            editedTitle: trimmedTitle,
            editedNotes: notes.isEmpty ? nil : notes
        )

        onDismiss()
    }
}

// MARK: - Preview Helper

/// Simulates the share extension experience within the main app.
///
/// Wraps `ShareExtensionView` in a sheet for development/preview testing.
/// In production, this would be replaced by the extension's view controller.
struct ShareExtensionPreview: View {
    @State private var isPresented = true

    var body: some View {
        Color.clear
            .sheet(isPresented: $isPresented) {
                ShareExtensionView(
                    content: ShareContent(
                        kind: .url,
                        title: "How to design CRDTs",
                        notes: "https://example.com/crdts",
                        url: URL(string: "https://example.com/crdts")
                    ),
                    onDismiss: { isPresented = false }
                )
            }
    }
}

#Preview("Share Extension — URL") {
    ShareExtensionPreview()
}

#Preview("Share Extension — Text") {
    ShareExtensionView(
        content: ShareContent(
            kind: .text,
            title: "Review the Q4 planning document",
            notes: "Make sure to check budget allocations and team assignments before the Friday meeting."
        ),
        onDismiss: {}
    )
}
