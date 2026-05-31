# apps/macos

SwiftUI native macOS app for tock. Three modes per architecture §8.3:

1. **Full window** — `WindowGroup` with `NavigationSplitView` (same 3-column layout as iPadOS).
2. **MenuBarExtra** — Always-on menu bar item with compact popover: timer/focus status, today tasks, quick-add, focus controls.
3. **Quick entry** — Global hotkey (`⌃⌥Space`) opens a floating `NSPanel` for single-line natural-language task input.

## Building

```sh
# Build (requires macOS 14+ SDK)
cd apps/macos
swift build

# Run
swift run TockMac
```

> **Note:** The Rust core is not yet connected — the app uses `MockCoreClient` with static sample data. Production integration requires generating `TockFFI` via UniFFI and wiring it through `CoreActor`.

## Architecture

- **`App/`** — `@main` entry point (`TockMacApp`) with `WindowGroup`, `MenuBarExtra`, and `Settings` scenes.
- **`Core/`** — Shared types: `AppSessionState` (cross-scene observable state), `CoreClient` protocol, `MockCoreClient`, domain models.
- **`Navigation/`** — `ContentView` (3-column layout), `SidebarView`, `TaskListView`, `VaultSetupView`.
- **`Features/`** — View implementations: Today, Inbox, Habits, Timer, Projects, Settings, detail views.
- **`ViewModels/`** — Presentation logic for feature views.
- **`MenuBar/`** — `MenuBarView` popover content.
- **`QuickEntry/`** — `QuickEntryPanelController` (AppKit `NSPanel` + Carbon global hotkey).
- **`Commands/`** — `TockCommands` defining macOS menu bar keyboard shortcuts.
- **`Components/`** — Reusable UI components: `TaskRow`, `TagChip`, `PriorityBadge`, etc.
- **`Theme/`** — Design tokens (`TockTheme`).

## Code Sharing

Many files are identical copies from `apps/ios/` (marked with `// NOTE: Shared with apps/ios`). These should be extracted to a shared `apps/shared/` package in a future refactoring to eliminate duplication.

## Keyboard Shortcuts

| Shortcut       | Action                           |
|----------------|----------------------------------|
| `⌘N`           | New task (sheet)                 |
| `⌃⌥Space`      | Global quick entry               |
| `⌘1`–`⌘5`      | Switch tab (Today/Inbox/...)     |
| `⌘F`           | Search                           |
| `Space`        | Toggle complete on selection     |
| `⌘D`           | Defer (stubbed)                  |
| `⌘E`           | Evening                          |
| `⌘T`           | Start/stop timer on selection    |
| `⌘⇧F`          | Start focus session              |
| `⌘,`           | Settings                         |
| `⌘⌥L`          | Lock vault                       |
