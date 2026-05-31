import SwiftUI

/// App-wide observable session state shared across all scenes.
///
/// Owned by the `TockMacApp` entry point and injected into `WindowGroup`,
/// `MenuBarExtra`, and `Settings` scenes. Holds vault lifecycle, active
/// `CoreClient`, timer/focus summaries for the menu bar, and quick-entry state.
///
/// Per-window navigation state (selected sidebar item, selected task) lives
/// in individual views using `@SceneStorage` or `@State`.
@Observable
@MainActor
final class AppSessionState {

    enum VaultStatus: Sendable {
        case locked
        case unlocking
        case unlocked
        case error(String)
    }

    var vaultStatus: VaultStatus = .locked
    var showQuickAdd = false

    /// The active core client. Replaced with a live client when the
    /// vault is unlocked; defaults to mock for development.
    var client: any CoreClient = MockCoreClient.shared

    // MARK: - Menu bar display state

    /// Current timer block for menu bar display (nil = no active timer).
    var activeTimer: TimeBlockItem?

    /// Active focus session for menu bar display (nil = no active session).
    var activeFocus: FocusSessionItem?

    /// Today's tasks for menu bar popover.
    var todayTasks: [TaskItem] = []

    /// Short label for the menu bar icon.
    var menuBarTitle: String {
        if let focus = activeFocus, focus.state == .working || focus.state == .paused {
            let state = focus.state == .paused ? "⏸" : "🍅"
            return "\(state) \(focus.completedCycles)/\(focus.plannedCycles)"
        }
        if let timer = activeTimer {
            let elapsed = TimeBlockRow.formatDuration(timer.duration)
            return "⏱ \(elapsed)"
        }
        return "✓"
    }

    /// SF Symbol for the menu bar.
    var menuBarIcon: String {
        if activeFocus != nil { return "brain.head.profile" }
        if activeTimer != nil { return "timer" }
        return "checkmark.seal.fill"
    }

    // MARK: - Vault lifecycle

    func unlock(path: String, password: String) async {
        vaultStatus = .unlocking
        // TODO: Replace with actual vault open via CoreActor when
        // UniFFI bindings are connected.
        try? await Task.sleep(for: .milliseconds(500))
        vaultStatus = .unlocked
        await refreshMenuBarState()
    }

    func createVault(path: String, password: String) async {
        vaultStatus = .unlocking
        try? await Task.sleep(for: .milliseconds(500))
        vaultStatus = .unlocked
        await refreshMenuBarState()
    }

    func lock() {
        vaultStatus = .locked
        activeTimer = nil
        activeFocus = nil
        todayTasks = []
    }

    // MARK: - Menu bar data refresh

    func refreshMenuBarState() async {
        guard case .unlocked = vaultStatus else { return }
        activeTimer = try? await client.currentTimer()
        activeFocus = try? await client.focusStatus()
        todayTasks = (try? await client.listTasks(filter: .today)
            .sorted { $0.urgency > $1.urgency }) ?? []
    }
}
