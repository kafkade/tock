import WidgetKit
import SwiftUI

// MARK: - Widget Declarations

/// Main Today widget — supports all system widget sizes.
///
/// Shows today's tasks, active timer, habits, and inbox (extra-large).
/// Interactive checkboxes and habit chips use App Intents (iOS 17+).
struct TodayWidget: Widget {
    static let kind = "com.kafkade.tock.widget.today"

    var body: some WidgetConfiguration {
        StaticConfiguration(kind: Self.kind, provider: TodayWidgetProvider()) { entry in
            TodayWidgetView(entry: entry)
                .containerBackground(.fill.tertiary, for: .widget)
        }
        .configurationDisplayName("Today")
        .description("Your tasks, habits, and timer at a glance.")
        .supportedFamilies([
            .systemSmall,
            .systemMedium,
            .systemLarge,
            .systemExtraLarge,
        ])
    }
}

#if !os(macOS)
/// Habit accessory widget — lock screen habit ring.
///
/// Shows the top habit's streak as a circular progress ring,
/// or a task count badge if no habits are configured.
struct HabitAccessoryWidget: Widget {
    static let kind = "com.kafkade.tock.widget.habit"

    var body: some WidgetConfiguration {
        StaticConfiguration(kind: Self.kind, provider: HabitAccessoryProvider()) { entry in
            HabitAccessoryView(entry: entry)
                .containerBackground(.fill.tertiary, for: .widget)
        }
        .configurationDisplayName("Habit Streak")
        .description("Track your habit streak on the lock screen.")
        .supportedFamilies([.accessoryCircular])
    }
}

/// Status accessory widget — lock screen status line and next task.
///
/// Shows due task count and timer status (inline) or next task
/// with deadline (rectangular).
struct StatusAccessoryWidget: Widget {
    static let kind = "com.kafkade.tock.widget.status"

    var body: some WidgetConfiguration {
        StaticConfiguration(kind: Self.kind, provider: StatusAccessoryProvider()) { entry in
            AccessoryWidgetView(entry: entry)
                .containerBackground(.fill.tertiary, for: .widget)
        }
        .configurationDisplayName("Status")
        .description("Due count and timer status at a glance.")
        .supportedFamilies([
            .accessoryRectangular,
            .accessoryInline,
        ])
    }
}
#endif

// MARK: - Widget Bundle

/// Tock widget bundle — registered as the widget extension entry point.
///
/// **Note**: This struct intentionally does NOT use `@main`. The `@main`
/// attribute must be on the Widget Extension target's entry point, which
/// is configured in the Xcode project. This bundle is designed to be
/// referenced from there:
///
/// ```swift
/// @main
/// struct TockWidgetExtension: WidgetBundle {
///     var body: some Widget {
///         TockWidgetBundle().body
///     }
/// }
/// ```
struct TockWidgetBundle: WidgetBundle {
    var body: some Widget {
        TodayWidget()
        #if !os(macOS)
        HabitAccessoryWidget()
        StatusAccessoryWidget()
        #endif
    }
}
