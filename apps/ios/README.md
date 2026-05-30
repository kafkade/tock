# apps/ios

SwiftUI app for iPhone and iPad, targeting iOS 17+ and macOS 14+.

## Architecture

Thin SwiftUI shell over the Rust core via UniFFI (see
[`docs/architecture.md`](../../docs/architecture.md) §8.1):

- **`CoreActor`** — `@globalActor` serializing all UniFFI calls
- **`CoreClient` protocol** — dependency boundary between views and
  Rust core. `MockCoreClient` for previews/dev, `TockCoreClient` (future)
  for production
- **`AppState`** — `@Observable` app-wide state (vault status, quick-add)
- **Presentation DTOs** — `TaskItem`, `ProjectItem`, `HabitItem`, etc.
  Mapped from UniFFI types when bindings are connected

## Views

| Tab | View | Description |
| --- | ---- | ----------- |
| Today | `TodayView` | Agenda of urgent/due tasks |
| Inbox | `InboxView` | Unprocessed tasks for triage |
| Projects | `ProjectsView` → `ProjectDetailView` | Hierarchical project browser |
| Habits | `HabitsView` → `HabitDetailView` | Daily tracker with streaks |
| Timer | `TimerView` | Time tracking + Pomodoro focus |

Additional: `SettingsView`, `TaskDetailView`, `QuickAddSheet`,
`VaultSetupView`.

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
- [ ] Connect to UniFFI bindings (requires macOS + `TockFFI` generation)
- [ ] iPadOS `NavigationSplitView` (#50)
- [ ] Biometric unlock (#51)
- [ ] Widgets (#52), App Intents (#53), Share Extension (#54)
