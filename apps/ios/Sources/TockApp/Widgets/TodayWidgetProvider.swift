import WidgetKit
import SwiftUI

// MARK: - Today Widget Entry

/// Timeline entry for the main Today widget.
///
/// Supports `.systemSmall` (timer or next task), `.systemMedium` (3–4 tasks),
/// `.systemLarge` (6 tasks + habits + timer), `.systemExtraLarge` (two-column).
struct TodayWidgetEntry: TimelineEntry {
    let date: Date
    let snapshot: WidgetSnapshot
}

// MARK: - Today Widget Provider

/// Timeline provider for the Today widget across all system sizes.
struct TodayWidgetProvider: TimelineProvider {
    private let store: any WidgetSnapshotStore

    init(store: any WidgetSnapshotStore = MockWidgetSnapshotStore.shared) {
        self.store = store
    }

    func placeholder(in context: Context) -> TodayWidgetEntry {
        TodayWidgetEntry(date: Date(), snapshot: .placeholder)
    }

    func getSnapshot(in context: Context, completion: @escaping (TodayWidgetEntry) -> Void) {
        Task {
            let snapshot = await store.loadSnapshot()
            completion(TodayWidgetEntry(date: Date(), snapshot: snapshot))
        }
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<TodayWidgetEntry>) -> Void) {
        Task {
            let snapshot = await store.loadSnapshot()
            let entry = TodayWidgetEntry(date: Date(), snapshot: snapshot)

            // Refresh every 15 minutes, or at timer end if active
            let nextRefresh: Date
            if let timer = snapshot.activeTimer {
                // Refresh at next minute boundary for elapsed time display
                nextRefresh = Calendar.current.date(
                    byAdding: .minute, value: 1,
                    to: Date()
                ) ?? Date().addingTimeInterval(60)
                _ = timer // used for future timer-end scheduling
            } else {
                nextRefresh = Date().addingTimeInterval(15 * 60)
            }

            completion(Timeline(entries: [entry], policy: .after(nextRefresh)))
        }
    }
}

// MARK: - Placeholder snapshot

extension WidgetSnapshot {
    /// Placeholder snapshot with redacted content for widget gallery.
    static let placeholder = WidgetSnapshot(
        todayTasks: [
            WidgetTask(id: "p1", sid: 1, title: "Loading…", priority: .high,
                       deadline: Date(), evening: false, urgency: 8.0),
            WidgetTask(id: "p2", sid: 2, title: "Loading…", priority: .medium,
                       deadline: nil, evening: false, urgency: 5.0),
            WidgetTask(id: "p3", sid: 3, title: "Loading…", priority: nil,
                       deadline: nil, evening: false, urgency: 3.0),
        ],
        inboxTasks: [
            WidgetTask(id: "p4", sid: 4, title: "Loading…", priority: nil,
                       deadline: nil, evening: false, urgency: 1.0),
        ],
        habits: [
            WidgetHabit(id: "ph1", title: "Loading…", direction: .build,
                        streakCurrent: 7, streakBest: 14, level: 2),
        ],
        activeTimer: nil,
        activeFocus: nil,
        isVaultLocked: false,
        dueCount: 0
    )
}
