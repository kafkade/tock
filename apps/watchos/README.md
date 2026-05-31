# Tock watchOS Companion App

Constrained companion app for Apple Watch showing today's tasks, habits, and
timer/focus status. Designed for wrist-scale interactions: glance, tap, time.

## Capabilities

- **Today list** — up to 20 tasks sorted by urgency, tap to complete with haptic
  feedback.
- **Habit tracker** — view streaks, tap to log, grouped by build/break direction.
- **Timer** — quick-start timer or Pomodoro focus session with progress ring.
- **Complications** — WidgetKit complications for all accessory families (circular,
  rectangular, inline, corner) showing habit streaks, task counts, and timer status.

## Architecture

The watch maintains a **read replica** of the actionable surface (today tasks and
habits) received from the paired iPhone via `WatchConnectivity`. Mutations
(complete task, log habit, start timer) are sent to the iPhone as intents. When
the phone is unreachable, intents are persisted in a local queue and replayed on
reconnection.

The watch never holds the full vault, projects, areas, or search.

See `docs/architecture.md` §8.4 for the full watchOS design specification.

## Structure

| Directory          | Purpose                                            |
|--------------------|----------------------------------------------------|
| `App/`             | App entry point, TabView, vault gate               |
| `Core/`            | `WatchCoreClient` protocol, mock client, app state |
| `Features/`        | Today, Habits, Timer/Focus views                   |
| `Components/`      | Reusable row components (task, habit)               |
| `Connectivity/`    | `WatchSessionManager`, persistent intent queue     |
| `Complications/`   | WidgetKit complications for all families            |
| `Theme/`           | Design tokens adapted for watchOS                  |

## Development

The app uses `MockWatchCoreClient` with static sample data. No Rust core or
iPhone pairing is required for SwiftUI preview development.

To build, open the Xcode project (when created) or use SPM:

```bash
# Resolve dependencies
cd apps/watchos && swift package resolve
```

The complication bundle (`TockComplicationBundle`) needs to be referenced from
a WidgetKit extension target configured in the Xcode project.

## Complication Families

| WidgetKit Family       | Content                              | Spec Reference      |
|------------------------|--------------------------------------|----------------------|
| `.accessoryCircular`   | Habit streak ring or task count      | circularSmall        |
| `.accessoryRectangular`| Next 3 tasks or active timer/focus   | modularLarge         |
| `.accessoryInline`     | "3 due · 🍅 12:34" status line       | utilitarianLarge     |
| `.accessoryCorner`     | Habit gauge along bezel              | graphicCorner        |
