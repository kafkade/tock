# apps/ios

SwiftUI app for iPhone and iPad, targeting iOS 17+ and macOS 14+.

## Architecture

Thin SwiftUI shell over the Rust core via UniFFI (see
[`docs/architecture.md`](../../docs/architecture.md) §8.1):

- **`CoreActor`** — `@globalActor` serializing all UniFFI calls
- **`CoreClient` protocol** — dependency boundary between views and
  Rust core. `MockCoreClient` for previews/dev, `TockCoreClient` (future)
  for production
- **`AppState`** — `@Observable` app-wide state (vault status, navigation,
  quick-add). Each Stage Manager window gets its own instance.
- **Presentation DTOs** — `TaskItem`, `ProjectItem`, `HabitItem`, etc.
  Mapped from UniFFI types when bindings are connected

## Navigation

The app uses an **adaptive layout** that switches based on horizontal
size class:

### iPhone (compact)

Tab bar with five tabs: Today, Inbox, Projects, Habits, Timer.
Settings accessible from toolbar. Each tab contains a `NavigationStack`.

### iPad (regular)

Three-column `NavigationSplitView` per architecture §8.2:

- **Sidebar**: Smart views (Today, Inbox, Upcoming, Anytime, Someday,
  Logbook), Projects, Areas, Habits, Timer, Settings
- **Content**: Task list / habits / timer based on sidebar selection
- **Detail**: Selected task detail or placeholder

Supports:

- **Drag and drop**: Drag tasks between sidebar destinations (projects,
  views) to reorganize
- **Keyboard shortcuts** (Smart Keyboard / Magic Keyboard):
  - `⌘N` — New task
  - `⌘1`–`⌘5` — Switch view (Today/Inbox/Upcoming/Anytime/Someday)
  - `⌘6`/`⌘7` — Habits/Timer
  - `Space` — Mark done
  - `⌘E` — Toggle evening
  - `⌘T` — Start/stop timer
  - `⌘⇧F` — Start focus session
- **Stage Manager**: Each window owns independent state

## Views

| Tab/Sidebar | View | Description |
| --- | ---- | ----------- |
| Today | `TodayView` / `TaskListView` | Agenda of urgent/due tasks |
| Inbox | `InboxView` / `TaskListView` | Unprocessed tasks for triage |
| Projects | `ProjectsView` → `ProjectDetailView` | Hierarchical project browser |
| Habits | `HabitsView` → `HabitDetailView` | Daily tracker with streaks |
| Timer | `TimerView` | Time tracking + Pomodoro focus |

Additional: `SettingsView`, `TaskDetailView`, `QuickAddSheet`,
`VaultSetupView`, `SidebarView`.

## Building

The app requires an Xcode project or workspace wrapping this SPM
package. The `Package.swift` depends on `../../bindings/swift` (the
`TockSwift` library).

```bash
# From the repo root — open in Xcode:
open apps/ios/Package.swift
```

Until UniFFI bindings are generated, the app runs with `MockCoreClient`
providing sample data for development and SwiftUI previews.

## Status

- [x] App shell with vault gate + 5-tab navigation
- [x] All feature views with view models
- [x] Reusable components (TaskRow, HabitRow, TimeBlockRow, etc.)
- [x] Quick-add sheet at app level
- [x] Mock data for development/previews
- [x] iPadOS `NavigationSplitView` with three-column layout (#50)
- [x] Keyboard shortcuts for iPad external keyboards (#50)
- [x] Drag-and-drop task organization (#50)
- [x] Stage Manager multi-window support (#50)
- [x] Biometric vault unlock — Face ID / Touch ID (#51)
- [x] WidgetKit widgets — Today, Habit, Status across 7 size families (#52)
- [x] App Intents for Siri and Shortcuts — 11 intents, 4 entity types, 6 pre-built shortcuts (#53)
- [x] Share extension for quick capture from any app (#54)
- [ ] Connect to UniFFI bindings (requires macOS + `TockFFI` generation)
