import WidgetKit
import SwiftUI

// MARK: - Accessory Widget Entry

/// Timeline entry for lock screen / accessory widgets.
struct AccessoryWidgetEntry: TimelineEntry {
    let date: Date
    let snapshot: WidgetSnapshot
}

// MARK: - Habit Accessory Provider

/// Timeline provider for the habit ring (`.accessoryCircular`).
struct HabitAccessoryProvider: TimelineProvider {
    private let store: any WidgetSnapshotStore

    init(store: any WidgetSnapshotStore = MockWidgetSnapshotStore.shared) {
        self.store = store
    }

    func placeholder(in context: Context) -> AccessoryWidgetEntry {
        AccessoryWidgetEntry(date: Date(), snapshot: .placeholder)
    }

    func getSnapshot(in context: Context, completion: @escaping (AccessoryWidgetEntry) -> Void) {
        Task {
            let snapshot = await store.loadSnapshot()
            completion(AccessoryWidgetEntry(date: Date(), snapshot: snapshot))
        }
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<AccessoryWidgetEntry>) -> Void) {
        Task {
            let snapshot = await store.loadSnapshot()
            let entry = AccessoryWidgetEntry(date: Date(), snapshot: snapshot)
            let nextRefresh = Date().addingTimeInterval(30 * 60)
            completion(Timeline(entries: [entry], policy: .after(nextRefresh)))
        }
    }
}

// MARK: - Status Accessory Provider

/// Timeline provider for status widgets (`.accessoryRectangular`, `.accessoryInline`).
struct StatusAccessoryProvider: TimelineProvider {
    private let store: any WidgetSnapshotStore

    init(store: any WidgetSnapshotStore = MockWidgetSnapshotStore.shared) {
        self.store = store
    }

    func placeholder(in context: Context) -> AccessoryWidgetEntry {
        AccessoryWidgetEntry(date: Date(), snapshot: .placeholder)
    }

    func getSnapshot(in context: Context, completion: @escaping (AccessoryWidgetEntry) -> Void) {
        Task {
            let snapshot = await store.loadSnapshot()
            completion(AccessoryWidgetEntry(date: Date(), snapshot: snapshot))
        }
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<AccessoryWidgetEntry>) -> Void) {
        Task {
            let snapshot = await store.loadSnapshot()
            let entry = AccessoryWidgetEntry(date: Date(), snapshot: snapshot)
            // Refresh more frequently if timer is active
            let interval: TimeInterval = snapshot.activeTimer != nil ? 60 : 15 * 60
            let nextRefresh = Date().addingTimeInterval(interval)
            completion(Timeline(entries: [entry], policy: .after(nextRefresh)))
        }
    }
}
