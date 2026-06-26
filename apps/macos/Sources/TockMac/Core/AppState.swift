import SwiftUI
import TockSwift

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
    var client: any CoreClient = LockedCoreClient()

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
        do {
            let workspace = try await TockWorkspace.open(
                path: path, password: Data(password.utf8)
            )
            client = TockCoreClient(workspace: workspace)
            vaultStatus = .unlocked
            await refreshMenuBarState()
        } catch {
            vaultStatus = .error(Self.describe(error))
        }
    }

    func createVault(path: String, password: String) async {
        vaultStatus = .unlocking
        do {
            let workspace = try await TockWorkspace.create(
                path: path, password: Data(password.utf8)
            )
            client = TockCoreClient(workspace: workspace)
            vaultStatus = .unlocked
            await refreshMenuBarState()
        } catch {
            vaultStatus = .error(Self.describe(error))
        }
    }

    func lock() {
        if let live = client as? TockCoreClient {
            Task { try? await live.lock() }
        }
        client = LockedCoreClient()
        vaultStatus = .locked
        activeTimer = nil
        activeFocus = nil
        todayTasks = []
    }

    /// Human-readable description for a vault error, mapping the common
    /// `TockError` cases to friendly text.
    private static func describe(_ error: Error) -> String {
        if let tockError = error as? TockError {
            switch tockError {
            case .InvalidCredentials:
                return "Incorrect master password."
            case .VaultNotFound:
                return "No vault found. Create one to get started."
            default:
                return "Could not open the vault. Please try again."
            }
        }
        return error.localizedDescription
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
