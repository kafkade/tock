import WidgetKit
import SwiftUI

// MARK: - Individual Complications

/// Habit ring complication — circular progress showing streak.
///
/// Maps to architecture §8.4 families: `.circularSmall`,
/// `.modularSmall`, `.graphicCircular`.
struct HabitRingComplication: Widget {
    static let kind = "com.kafkade.tock.watch.complication.habitRing"

    var body: some WidgetConfiguration {
        StaticConfiguration(kind: Self.kind, provider: HabitRingProvider()) { entry in
            HabitRingComplicationView(entry: entry)
                .containerBackground(.fill.tertiary, for: .widget)
        }
        .configurationDisplayName("Habit Streak")
        .description("Track your habit streak on the watch face.")
        .supportedFamilies([.accessoryCircular])
    }
}

/// Task list / timer complication — rectangular area.
///
/// Maps to architecture §8.4 families: `.modularLarge`,
/// `.graphicRectangular`, `.graphicExtraLarge` (Ultra).
struct TaskListComplication: Widget {
    static let kind = "com.kafkade.tock.watch.complication.taskList"

    var body: some WidgetConfiguration {
        StaticConfiguration(kind: Self.kind, provider: TaskListProvider()) { entry in
            TaskListComplicationView(entry: entry)
                .containerBackground(.fill.tertiary, for: .widget)
        }
        .configurationDisplayName("Tasks")
        .description("Your next tasks or active timer.")
        .supportedFamilies([.accessoryRectangular])
    }
}

/// Status inline complication — single text line.
///
/// Maps to architecture §8.4 families: `.utilitarianSmall`,
/// `.utilitarianLarge`, `.graphicBezel`.
struct StatusInlineComplication: Widget {
    static let kind = "com.kafkade.tock.watch.complication.status"

    var body: some WidgetConfiguration {
        StaticConfiguration(kind: Self.kind, provider: StatusInlineProvider()) { entry in
            StatusInlineComplicationView(entry: entry)
                .containerBackground(.fill.tertiary, for: .widget)
        }
        .configurationDisplayName("Status")
        .description("Due count and timer status.")
        .supportedFamilies([.accessoryInline])
    }
}

/// Corner gauge complication — habit or task gauge on bezel.
///
/// Maps to architecture §8.4 family: `.graphicCorner`.
struct CornerGaugeComplication: Widget {
    static let kind = "com.kafkade.tock.watch.complication.corner"

    var body: some WidgetConfiguration {
        StaticConfiguration(kind: Self.kind, provider: CornerGaugeProvider()) { entry in
            CornerGaugeComplicationView(entry: entry)
                .containerBackground(.fill.tertiary, for: .widget)
        }
        .configurationDisplayName("Habit Gauge")
        .description("Habit streak gauge on the watch face corner.")
        .supportedFamilies([.accessoryCorner])
    }
}

// MARK: - Complication Bundle

/// Tock watchOS complication bundle.
///
/// **Note**: This struct intentionally does NOT use `@main`. The `@main`
/// attribute must be on the Widget Extension target's entry point, which
/// is configured in the Xcode project. This bundle is designed to be
/// referenced from there:
///
/// ```swift
/// @main
/// struct TockWatchWidgetExtension: WidgetBundle {
///     var body: some Widget {
///         TockComplicationBundle().body
///     }
/// }
/// ```
struct TockComplicationBundle: WidgetBundle {
    var body: some Widget {
        HabitRingComplication()
        TaskListComplication()
        StatusInlineComplication()
        CornerGaugeComplication()
    }
}
