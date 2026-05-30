import SwiftUI

/// Time tracking and focus session view.
///
/// Toggle between simple timer mode and Pomodoro focus mode.
struct TimerView: View {
    @Environment(AppState.self) private var appState
    @State private var viewModel: TimerViewModel?

    var body: some View {
        Group {
            if let vm = viewModel {
                contentView(vm: vm)
            } else {
                ProgressView()
            }
        }
        .navigationTitle("Timer")
        .task {
            let vm = TimerViewModel(client: appState.client)
            viewModel = vm
            await vm.load()
        }
        .refreshable {
            await viewModel?.load()
        }
    }

    @ViewBuilder
    private func contentView(vm: TimerViewModel) -> some View {
        VStack(spacing: 0) {
            // Mode picker
            Picker("Mode", selection: Binding(
                get: { vm.mode },
                set: { vm.mode = $0 }
            )) {
                ForEach(TimerViewModel.Mode.allCases, id: \.self) { mode in
                    Text(mode.rawValue).tag(mode)
                }
            }
            .pickerStyle(.segmented)
            .padding()

            switch vm.mode {
            case .timer:
                timerContent(vm: vm)
            case .focus:
                focusContent(vm: vm)
            }
        }
    }

    // MARK: - Timer mode

    @ViewBuilder
    private func timerContent(vm: TimerViewModel) -> some View {
        List {
            if let block = vm.currentBlock {
                Section("Running") {
                    TimeBlockRow(block: block)

                    Button("Stop", role: .destructive) {
                        Task { await vm.stopTimer() }
                    }
                }
            } else {
                Section("Start Timer") {
                    HStack {
                        TextField("What are you working on?", text: Binding(
                            get: { vm.newTimerTitle },
                            set: { vm.newTimerTitle = $0 }
                        ))

                        Button {
                            Task { await vm.startTimer() }
                        } label: {
                            Image(systemName: "play.circle.fill")
                                .font(.title2)
                        }
                        .disabled(vm.newTimerTitle.trimmingCharacters(in: .whitespaces).isEmpty)
                    }
                }
            }

            if !vm.recentBlocks.isEmpty {
                Section("Recent") {
                    ForEach(vm.recentBlocks.prefix(10)) { block in
                        TimeBlockRow(block: block)
                    }
                }
            }
        }
    }

    // MARK: - Focus mode

    @ViewBuilder
    private func focusContent(vm: TimerViewModel) -> some View {
        if let session = vm.activeFocus {
            activeFocusView(session: session, vm: vm)
        } else {
            startFocusView(vm: vm)
        }
    }

    @ViewBuilder
    private func startFocusView(vm: TimerViewModel) -> some View {
        VStack(spacing: TockTheme.Spacing.xl) {
            Spacer()

            Image(systemName: "brain.head.profile")
                .font(.system(size: 60))
                .foregroundStyle(TockTheme.Colors.focusWork)

            Text("Focus Session")
                .font(.title2)
                .bold()

            Stepper(
                "Cycles: \(vm.focusCycles)",
                value: Binding(
                    get: { Int(vm.focusCycles) },
                    set: { vm.focusCycles = UInt32($0) }
                ),
                in: 1...8
            )
            .padding(.horizontal, TockTheme.Spacing.xxl)

            Text("25 min work · 5 min break · 15 min long break")
                .font(.caption)
                .foregroundStyle(.secondary)

            Button {
                Task { await vm.startFocus() }
            } label: {
                Text("Start Focus")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .padding(.horizontal, TockTheme.Spacing.xxl)

            Spacer()
        }
    }

    @ViewBuilder
    private func activeFocusView(session: FocusSessionItem, vm: TimerViewModel) -> some View {
        VStack(spacing: TockTheme.Spacing.xl) {
            Spacer()

            // State indicator
            ZStack {
                Circle()
                    .stroke(Color.secondary.opacity(0.2), lineWidth: 12)
                    .frame(width: 200, height: 200)

                Circle()
                    .trim(
                        from: 0,
                        to: Double(session.completedCycles) / max(Double(session.plannedCycles), 1)
                    )
                    .stroke(
                        focusColor(for: session.state),
                        style: StrokeStyle(lineWidth: 12, lineCap: .round)
                    )
                    .frame(width: 200, height: 200)
                    .rotationEffect(.degrees(-90))

                VStack(spacing: TockTheme.Spacing.xs) {
                    Text(focusLabel(for: session.state))
                        .font(.headline)
                        .foregroundStyle(focusColor(for: session.state))

                    Text("\(session.completedCycles)/\(session.plannedCycles)")
                        .font(.title)
                        .bold()
                        .monospacedDigit()

                    Text("cycles")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            // Action buttons
            HStack(spacing: TockTheme.Spacing.lg) {
                switch session.state {
                case .working:
                    Button("Pause") { Task { await vm.pauseFocus() } }
                        .buttonStyle(.bordered)
                    Button("Complete Cycle") { Task { await vm.completeCycle() } }
                        .buttonStyle(.borderedProminent)
                case .shortBreak, .longBreak:
                    Button("Skip Break") { Task { await vm.skipBreak() } }
                        .buttonStyle(.borderedProminent)
                case .paused:
                    Button("Resume") { Task { await vm.resumeFocus() } }
                        .buttonStyle(.borderedProminent)
                case .completed, .aborted:
                    EmptyView()
                }

                Button("Abort", role: .destructive) { Task { await vm.abortFocus() } }
                    .buttonStyle(.bordered)
            }

            Spacer()
        }
        .padding()
    }

    private func focusColor(for state: FocusState) -> Color {
        switch state {
        case .working: TockTheme.Colors.focusWork
        case .shortBreak, .longBreak: TockTheme.Colors.focusBreak
        case .paused: TockTheme.Colors.timerPaused
        case .aborted: TockTheme.Colors.destructive
        case .completed: TockTheme.Colors.success
        }
    }

    private func focusLabel(for state: FocusState) -> String {
        switch state {
        case .working: "Working"
        case .shortBreak: "Short Break"
        case .longBreak: "Long Break"
        case .paused: "Paused"
        case .aborted: "Aborted"
        case .completed: "Done!"
        }
    }
}
