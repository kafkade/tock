import SwiftUI

/// Menu bar popover content — compact view for quick access.
///
/// Shows current timer/focus status, today's top tasks, quick-add field,
/// and focus controls. Always available regardless of main window state.
struct MenuBarView: View {
    @Environment(AppSessionState.self) private var appState

    @State private var quickAddTitle = ""
    @State private var isSubmitting = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Header with status
            statusSection
                .padding(.horizontal, TockTheme.Spacing.md)
                .padding(.top, TockTheme.Spacing.md)
                .padding(.bottom, TockTheme.Spacing.sm)

            Divider()

            switch appState.vaultStatus {
            case .unlocked:
                unlockedContent
            case .locked, .unlocking, .error:
                lockedContent
            }

            Divider()

            // Footer actions
            footerSection
                .padding(.horizontal, TockTheme.Spacing.md)
                .padding(.vertical, TockTheme.Spacing.sm)
        }
        .frame(width: 320)
        .task {
            await appState.refreshMenuBarState()
        }
    }

    // MARK: - Status section

    @ViewBuilder
    private var statusSection: some View {
        HStack {
            Image(systemName: "checkmark.seal.fill")
                .foregroundStyle(TockTheme.Colors.accent)
            Text("tock")
                .font(.headline)
            Spacer()
            statusBadge
        }
    }

    @ViewBuilder
    private var statusBadge: some View {
        if let focus = appState.activeFocus {
            HStack(spacing: TockTheme.Spacing.xxs) {
                Circle()
                    .fill(focus.state == .paused ? TockTheme.Colors.timerPaused : TockTheme.Colors.focusWork)
                    .frame(width: 6, height: 6)
                Text("\(focus.completedCycles)/\(focus.plannedCycles) cycles")
                    .font(.caption)
                    .monospacedDigit()
            }
        } else if let timer = appState.activeTimer {
            HStack(spacing: TockTheme.Spacing.xxs) {
                Circle()
                    .fill(TockTheme.Colors.timerActive)
                    .frame(width: 6, height: 6)
                Text(TimeBlockRow.formatDuration(timer.duration))
                    .font(.caption)
                    .monospacedDigit()
            }
        } else {
            Text("No timer")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    // MARK: - Unlocked content

    @ViewBuilder
    private var unlockedContent: some View {
        VStack(spacing: 0) {
            // Quick add field
            HStack(spacing: TockTheme.Spacing.sm) {
                TextField("Quick add task…", text: $quickAddTitle)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit {
                        Task { await submitQuickAdd() }
                    }

                if isSubmitting {
                    ProgressView()
                        .controlSize(.small)
                } else {
                    Button {
                        Task { await submitQuickAdd() }
                    } label: {
                        Image(systemName: "plus.circle.fill")
                    }
                    .buttonStyle(.plain)
                    .disabled(quickAddTitle.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .padding(.horizontal, TockTheme.Spacing.md)
            .padding(.vertical, TockTheme.Spacing.sm)

            Divider()

            // Today tasks (top 5)
            if appState.todayTasks.isEmpty {
                Text("No tasks for today")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .center)
                    .padding(.vertical, TockTheme.Spacing.xl)
            } else {
                VStack(spacing: 0) {
                    ForEach(appState.todayTasks.prefix(5)) { task in
                        menuBarTaskRow(task)
                        if task.id != appState.todayTasks.prefix(5).last?.id {
                            Divider()
                                .padding(.leading, TockTheme.Spacing.lg)
                        }
                    }

                    if appState.todayTasks.count > 5 {
                        Text("+\(appState.todayTasks.count - 5) more")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .padding(.vertical, TockTheme.Spacing.xs)
                    }
                }
            }

            // Focus controls
            if let focus = appState.activeFocus {
                Divider()
                focusControls(session: focus)
                    .padding(.horizontal, TockTheme.Spacing.md)
                    .padding(.vertical, TockTheme.Spacing.sm)
            }
        }
    }

    // MARK: - Locked content

    @ViewBuilder
    private var lockedContent: some View {
        VStack(spacing: TockTheme.Spacing.md) {
            Image(systemName: "lock.fill")
                .font(.title2)
                .foregroundStyle(.secondary)
            Text("Vault is locked")
                .font(.caption)
                .foregroundStyle(.secondary)
            Text("Open the main window to unlock.")
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, TockTheme.Spacing.xl)
    }

    // MARK: - Task row for menu bar

    @ViewBuilder
    private func menuBarTaskRow(_ task: TaskItem) -> some View {
        HStack(spacing: TockTheme.Spacing.sm) {
            if let priority = task.priority {
                PriorityBadge(priority: priority)
            }

            Text(task.title)
                .font(.callout)
                .lineLimit(1)

            Spacer()

            Button {
                Task {
                    try? await appState.client.completeTask(id: task.id)
                    await appState.refreshMenuBarState()
                }
            } label: {
                Image(systemName: "checkmark.circle")
                    .font(.caption)
            }
            .buttonStyle(.plain)
            .foregroundStyle(.secondary)
        }
        .padding(.horizontal, TockTheme.Spacing.md)
        .padding(.vertical, TockTheme.Spacing.xs)
    }

    // MARK: - Focus controls

    @ViewBuilder
    private func focusControls(session: FocusSessionItem) -> some View {
        HStack {
            Label("Focus", systemImage: "brain.head.profile")
                .font(.caption)
                .foregroundStyle(TockTheme.Colors.focusWork)

            Spacer()

            switch session.state {
            case .working:
                Button("Pause") {
                    Task {
                        _ = try? await appState.client.pauseFocus()
                        await appState.refreshMenuBarState()
                    }
                }
                .controlSize(.small)
            case .paused:
                Button("Resume") {
                    Task {
                        _ = try? await appState.client.resumeFocus()
                        await appState.refreshMenuBarState()
                    }
                }
                .controlSize(.small)
            case .shortBreak, .longBreak:
                Button("Skip Break") {
                    Task {
                        _ = try? await appState.client.skipBreak()
                        await appState.refreshMenuBarState()
                    }
                }
                .controlSize(.small)
            default:
                EmptyView()
            }

            Button("Stop") {
                Task {
                    _ = try? await appState.client.abortFocus()
                    await appState.refreshMenuBarState()
                }
            }
            .controlSize(.small)
            .foregroundStyle(TockTheme.Colors.destructive)
        }
    }

    // MARK: - Footer

    @ViewBuilder
    private var footerSection: some View {
        HStack {
            if case .unlocked = appState.vaultStatus {
                Button("Lock") {
                    appState.lock()
                }
                .font(.caption)
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
            }

            Spacer()

            Button("Quit tock") {
                NSApplication.shared.terminate(nil)
            }
            .font(.caption)
            .buttonStyle(.plain)
            .foregroundStyle(.secondary)
        }
    }

    // MARK: - Quick add

    private func submitQuickAdd() async {
        let trimmed = quickAddTitle.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return }

        isSubmitting = true
        let input = NewTaskInput(title: trimmed)
        _ = try? await appState.client.addTask(input)
        quickAddTitle = ""
        isSubmitting = false
        await appState.refreshMenuBarState()
    }
}
