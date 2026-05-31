import WidgetKit

// MARK: - Habit Ring Provider (accessoryCircular)

/// Timeline provider for the habit ring complication.
///
/// Shows the top habit's streak as a circular progress ring,
/// or a task count badge if no habits are configured.
struct HabitRingProvider: TimelineProvider {
    typealias Entry = ComplicationEntry

    private let store = ComplicationSnapshotStore.shared

    func placeholder(in context: Context) -> ComplicationEntry {
        ComplicationEntry(date: Date(), snapshot: .placeholder)
    }

    func getSnapshot(in context: Context, completion: @escaping (ComplicationEntry) -> Void) {
        let snapshot = store.loadSnapshot()
        completion(ComplicationEntry(date: Date(), snapshot: snapshot))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<ComplicationEntry>) -> Void) {
        let snapshot = store.loadSnapshot()
        let entry = ComplicationEntry(date: Date(), snapshot: snapshot)
        let nextRefresh = Date().addingTimeInterval(30 * 60)
        completion(Timeline(entries: [entry], policy: .after(nextRefresh)))
    }
}

// MARK: - Task List Provider (accessoryRectangular)

/// Timeline provider for the task list / timer complication.
///
/// Shows next 3 tasks sorted by urgency, or the active timer countdown.
struct TaskListProvider: TimelineProvider {
    typealias Entry = ComplicationEntry

    private let store = ComplicationSnapshotStore.shared

    func placeholder(in context: Context) -> ComplicationEntry {
        ComplicationEntry(date: Date(), snapshot: .placeholder)
    }

    func getSnapshot(in context: Context, completion: @escaping (ComplicationEntry) -> Void) {
        let snapshot = store.loadSnapshot()
        completion(ComplicationEntry(date: Date(), snapshot: snapshot))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<ComplicationEntry>) -> Void) {
        let snapshot = store.loadSnapshot()
        let entry = ComplicationEntry(date: Date(), snapshot: snapshot)
        // Refresh every minute if timer is active, else every 15 minutes
        let interval: TimeInterval = snapshot.activeTimer != nil ? 60 : 15 * 60
        let nextRefresh = Date().addingTimeInterval(interval)
        completion(Timeline(entries: [entry], policy: .after(nextRefresh)))
    }
}

// MARK: - Status Provider (accessoryInline)

/// Timeline provider for the inline status complication.
///
/// Shows due count and timer status: "3 due · 🍅 12:34"
struct StatusInlineProvider: TimelineProvider {
    typealias Entry = ComplicationEntry

    private let store = ComplicationSnapshotStore.shared

    func placeholder(in context: Context) -> ComplicationEntry {
        ComplicationEntry(date: Date(), snapshot: .placeholder)
    }

    func getSnapshot(in context: Context, completion: @escaping (ComplicationEntry) -> Void) {
        let snapshot = store.loadSnapshot()
        completion(ComplicationEntry(date: Date(), snapshot: snapshot))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<ComplicationEntry>) -> Void) {
        let snapshot = store.loadSnapshot()
        let entry = ComplicationEntry(date: Date(), snapshot: snapshot)
        let interval: TimeInterval = snapshot.activeTimer != nil ? 60 : 15 * 60
        let nextRefresh = Date().addingTimeInterval(interval)
        completion(Timeline(entries: [entry], policy: .after(nextRefresh)))
    }
}

// MARK: - Corner Provider (accessoryCorner)

/// Timeline provider for the corner gauge complication.
///
/// Shows a habit ring with gauge along the corner bezel.
struct CornerGaugeProvider: TimelineProvider {
    typealias Entry = ComplicationEntry

    private let store = ComplicationSnapshotStore.shared

    func placeholder(in context: Context) -> ComplicationEntry {
        ComplicationEntry(date: Date(), snapshot: .placeholder)
    }

    func getSnapshot(in context: Context, completion: @escaping (ComplicationEntry) -> Void) {
        let snapshot = store.loadSnapshot()
        completion(ComplicationEntry(date: Date(), snapshot: snapshot))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<ComplicationEntry>) -> Void) {
        let snapshot = store.loadSnapshot()
        let entry = ComplicationEntry(date: Date(), snapshot: snapshot)
        let nextRefresh = Date().addingTimeInterval(30 * 60)
        completion(Timeline(entries: [entry], policy: .after(nextRefresh)))
    }
}
