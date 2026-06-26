import Foundation

/// Persists the most recent ``WatchSnapshotDTO`` received from the iPhone.
///
/// Stored in `UserDefaults` so both the watch app process and the complication
/// timeline provider (a separate process) read the same data, and so the last
/// snapshot survives app relaunch. This is the watch's read-replica of the
/// vault — the phone owns the source of truth.
final class LiveSnapshotStore: @unchecked Sendable {

    static let shared = LiveSnapshotStore()

    private let defaults = UserDefaults.standard
    private let storageKey = "com.kafkade.tock.watch.liveSnapshot"
    private let lock = NSLock()

    /// The latest snapshot, or `nil` if none has arrived yet.
    var current: WatchSnapshotDTO? {
        lock.withLock {
            guard let data = defaults.data(forKey: storageKey) else { return nil }
            return try? WatchSync.decoder.decode(WatchSnapshotDTO.self, from: data)
        }
    }

    /// Persist a decoded snapshot.
    func save(_ snapshot: WatchSnapshotDTO) {
        lock.withLock {
            if let data = try? WatchSync.encoder.encode(snapshot) {
                defaults.set(data, forKey: storageKey)
            }
        }
    }

    /// Decode and persist a snapshot delivered in a WatchConnectivity payload.
    /// Returns the decoded snapshot on success.
    @discardableResult
    func ingest(_ context: [String: Any]) -> WatchSnapshotDTO? {
        guard
            let data = context[WatchSync.snapshotKey] as? Data,
            let snapshot = try? WatchSync.decoder.decode(WatchSnapshotDTO.self, from: data)
        else { return nil }
        save(snapshot)
        return snapshot
    }
}
