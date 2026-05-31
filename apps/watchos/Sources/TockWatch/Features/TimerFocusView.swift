import SwiftUI
import WatchKit

/// Combined timer and focus session view for the watch.
///
/// Supports:
/// - **Quick Timer**: One-tap start with recent label or custom title.
/// - **Focus session**: Configurable cycle count with progress ring.
/// - Active timer/focus display with controls.
struct TimerFocusView: View {
    @Environment(WatchAppState.self) private var appState

    @State private var currentTimer: TimeBlockItem?
    @State private var activeFocus: FocusSessionItem?
    @State private var isLoading = false
    @State private var showingQuickTimer = false
    @State private var focusCycles: Int = 4

    var body: some View {
        Group {
            if isLoading {
                ProgressView()
            } else if let focus = activeFocus {
                activeFocusView(focus)
            } else if let timer = currentTimer {
                activeTimerView(timer)
            } else {
                idleView
            }
        }
        .navigationTitle("Timer")
        .task {
            await load()
        }
    }

    // MARK: - Idle (no active timer or focus)

    @ViewBuilder
    private var idleView: some View {
        List {
            // Quick Timer
            Section {
                Button {
                    Task { await startQuickTimer() }
                } label: {
                    Label("Quick Timer", systemImage: "timer")
                }
            }

            // Focus session
            Section {
                Stepper("Cycles: \(focusCycles)", value: $focusCycles, in: 1...8)
                    .font(.caption)

                Button {
                    Task { await startFocus() }
                } label: {
                    Label("Start Focus", systemImage: "brain.head.profile")
                        .foregroundStyle(WatchTheme.Colors.focusWork)
                }
            } header: {
                Text("Focus")
            } footer: {
                Text("25 min work · 5 min break")
                    .font(.caption2)
            }
        }
    }

    // MARK: - Active timer

    @ViewBuilder
    private func activeTimerView(_ timer: TimeBlockItem) -> some View {
        VStack(spacing: WatchTheme.Spacing.xl) {
            Text(timer.title)
                .font(.headline)
                .lineLimit(2)
                .multilineTextAlignment(.center)

            Text(timer.startedAt, style: .relative)
                .font(.title2)
                .monospacedDigit()
                .foregroundStyle(WatchTheme.Colors.timerActive)

            Button(role: .destructive) {
                Task { await stopTimer() }
            } label: {
                Label("Stop", systemImage: "stop.fill")
            }
            .buttonStyle(.bordered)
            .tint(WatchTheme.Colors.destructive)
        }
        .padding()
    }

    // MARK: - Active focus

    @ViewBuilder
    private func activeFocusView(_ session: FocusSessionItem) -> some View {
        VStack(spacing: WatchTheme.Spacing.lg) {
            // Progress ring
            ZStack {
                Circle()
                    .stroke(Color.secondary.opacity(0.2), lineWidth: 6)

                Circle()
                    .trim(
                        from: 0,
                        to: Double(session.completedCycles) / max(Double(session.plannedCycles), 1)
                    )
                    .stroke(
                        focusColor(session.state),
                        style: StrokeStyle(lineWidth: 6, lineCap: .round)
                    )
                    .rotationEffect(.degrees(-90))

                VStack(spacing: 0) {
                    Text(focusLabel(session.state))
                        .font(.caption2)
                        .foregroundStyle(focusColor(session.state))

                    Text("\(session.completedCycles)/\(session.plannedCycles)")
                        .font(.title3)
                        .bold()
                        .monospacedDigit()
                }
            }
            .frame(width: 100, height: 100)

            // Action buttons
            focusActions(session)
        }
        .padding()
    }

    @ViewBuilder
    private func focusActions(_ session: FocusSessionItem) -> some View {
        switch session.state {
        case .working:
            HStack(spacing: WatchTheme.Spacing.lg) {
                Button {
                    Task { await pauseFocus() }
                } label: {
                    Image(systemName: "pause.fill")
                }
                .buttonStyle(.bordered)

                Button {
                    Task { await completeCycle() }
                } label: {
                    Image(systemName: "checkmark")
                }
                .buttonStyle(.bordered)
                .tint(WatchTheme.Colors.success)
            }

        case .shortBreak, .longBreak:
            Button {
                Task { await skipBreak() }
            } label: {
                Label("Skip Break", systemImage: "forward.fill")
            }
            .buttonStyle(.bordered)
            .tint(WatchTheme.Colors.focusBreak)

        case .paused:
            Button {
                Task { await resumeFocus() }
            } label: {
                Label("Resume", systemImage: "play.fill")
            }
            .buttonStyle(.bordered)
            .tint(WatchTheme.Colors.focusWork)

        case .completed, .aborted:
            EmptyView()
        }

        Button(role: .destructive) {
            Task { await abortFocus() }
        } label: {
            Text("Abort")
                .font(.caption2)
        }
        .buttonStyle(.plain)
        .foregroundStyle(.secondary)
    }

    // MARK: - Actions

    private func load() async {
        isLoading = true
        do {
            currentTimer = try await appState.client.currentTimer()
            activeFocus = try await appState.client.focusStatus()
        } catch {
            // Fall through to idle state
        }
        isLoading = false
    }

    private func startQuickTimer() async {
        WKInterfaceDevice.current().play(.start)
        do {
            currentTimer = try await appState.client.startTimer(
                title: "Quick Timer", taskId: nil
            )
        } catch {}
    }

    private func stopTimer() async {
        WKInterfaceDevice.current().play(.stop)
        do {
            _ = try await appState.client.stopTimer()
            currentTimer = nil
        } catch {}
    }

    private func startFocus() async {
        WKInterfaceDevice.current().play(.start)
        do {
            activeFocus = try await appState.client.startFocus(
                taskId: nil, cycles: UInt32(focusCycles)
            )
        } catch {}
    }

    private func completeCycle() async {
        WKInterfaceDevice.current().play(.success)
        do { activeFocus = try await appState.client.completeFocusCycle() } catch {}
    }

    private func skipBreak() async {
        do { activeFocus = try await appState.client.skipBreak() } catch {}
    }

    private func pauseFocus() async {
        do { activeFocus = try await appState.client.pauseFocus() } catch {}
    }

    private func resumeFocus() async {
        do { activeFocus = try await appState.client.resumeFocus() } catch {}
    }

    private func abortFocus() async {
        WKInterfaceDevice.current().play(.stop)
        do {
            _ = try await appState.client.abortFocus()
            activeFocus = nil
        } catch {}
    }

    // MARK: - Helpers

    private func focusColor(_ state: FocusState) -> Color {
        switch state {
        case .working: WatchTheme.Colors.focusWork
        case .shortBreak, .longBreak: WatchTheme.Colors.focusBreak
        case .paused: WatchTheme.Colors.timerPaused
        case .aborted: WatchTheme.Colors.destructive
        case .completed: WatchTheme.Colors.success
        }
    }

    private func focusLabel(_ state: FocusState) -> String {
        switch state {
        case .working: "Working"
        case .shortBreak: "Break"
        case .longBreak: "Long Break"
        case .paused: "Paused"
        case .aborted: "Aborted"
        case .completed: "Done!"
        }
    }
}
