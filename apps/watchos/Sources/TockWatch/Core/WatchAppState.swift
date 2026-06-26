import SwiftUI

/// Watch app-wide observable state.
///
/// Simpler than the iOS `AppState` — no biometrics, no multi-window,
/// no sidebar navigation. Tracks vault status, phone connectivity,
/// and the active `WatchCoreClient`.
@Observable
@MainActor
final class WatchAppState {

    /// Vault status as reported by the paired iPhone.
    enum VaultStatus: Sendable {
        case locked
        case unlocked
        case unknown
    }

    /// Phone connectivity status.
    enum ConnectionStatus: Sendable {
        case connected
        case disconnected
        case stale(lastSync: Date)
    }

    /// Current tab in the watch TabView.
    enum Tab: Hashable, Sendable {
        case today
        case habits
        case timer
    }

    var vaultStatus: VaultStatus = .unknown
    var connectionStatus: ConnectionStatus = .disconnected
    var selectedTab: Tab = .today

    /// The active core client. Reads from the iPhone snapshot replica and
    /// forwards mutations to the phone over WatchConnectivity.
    var client: any WatchCoreClient = LiveWatchCoreClient()

    /// Number of pending intents queued for sync.
    var pendingIntentCount: Int = 0

    /// Whether the watch has received at least one snapshot from iPhone.
    var hasReceivedInitialSync: Bool = false

    private let session = WatchSessionManager.shared

    /// Activate connectivity and start mirroring the iPhone's snapshot.
    /// Call once when the app appears.
    func start() {
        session.onDataUpdate = { [weak self] in
            self?.refresh()
        }
        session.activate()
        refresh()
    }

    /// Recompute UI state from the latest snapshot and connectivity status.
    func refresh() {
        let snapshot = LiveSnapshotStore.shared.current
        hasReceivedInitialSync = snapshot != nil
        pendingIntentCount = IntentQueue.shared.count

        if let snapshot {
            vaultStatus = snapshot.vaultLocked ? .locked : .unlocked
            connectionStatus = session.isReachable
                ? .connected
                : .stale(lastSync: snapshot.generatedAt)
        } else {
            vaultStatus = .unknown
            connectionStatus = session.isReachable ? .connected : .disconnected
        }
    }

    /// Human-readable connection description for the UI.
    var connectionLabel: String {
        switch connectionStatus {
        case .connected:
            return "Connected to iPhone"
        case .disconnected:
            return "iPhone not reachable"
        case .stale(let lastSync):
            let formatter = RelativeDateTimeFormatter()
            formatter.unitsStyle = .abbreviated
            let relative = formatter.localizedString(for: lastSync, relativeTo: Date())
            return "Last sync \(relative)"
        }
    }
}
