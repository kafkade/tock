import SwiftUI

/// macOS keyboard shortcut commands — full menu bar structure.
///
/// Applied via `.commands { TockCommands() }` on the WindowGroup scene.
/// Routes actions through `FocusedValues` to the active window's state.
///
/// Per architecture §8.3:
/// - ⌘N: New task (sheet)
/// - ⌘1–⌘5: Switch view (Today/Inbox/Upcoming/Anytime/Someday)
/// - ⌘F: Search (handled by .searchable)
/// - Space: Toggle complete on selection
/// - ⌘D: Defer (date picker popover) — stubbed
/// - ⌘E: Evening toggle
/// - ⌘T: Start/stop timer on selection
/// - ⌘⇧F: Start focus session
/// - ⌘,: Settings (handled by Settings scene)
/// - ⌘⌥L: Lock vault
struct TockCommands: Commands {
    @FocusedValue(\.quickAddAction) private var quickAdd
    @FocusedValue(\.sidebarItem) private var sidebarItem
    @FocusedValue(\.completeTaskAction) private var completeTask
    @FocusedValue(\.toggleEveningAction) private var toggleEvening
    @FocusedValue(\.toggleTimerAction) private var toggleTimer
    @FocusedValue(\.startFocusAction) private var startFocus
    @FocusedValue(\.lockVaultAction) private var lockVault
    @FocusedValue(\.quickEntryAction) private var quickEntry

    var body: some Commands {
        // File menu — new task
        CommandGroup(after: .newItem) {
            Button("New Task") {
                quickAdd?()
            }
            .keyboardShortcut("n", modifiers: .command)

            Divider()

            Button("Quick Entry") {
                quickEntry?()
            }
            .keyboardShortcut(.space, modifiers: [.control, .option])
        }

        // View switching — ⌘1 through ⌘5 + ⌘6/⌘7
        CommandMenu("Views") {
            ForEach(Array(SidebarItem.keyboardAccessible.enumerated()), id: \.element) { index, item in
                Button(item.title) {
                    sidebarItem?.wrappedValue = item
                }
                .keyboardShortcut(
                    KeyEquivalent(Character(String(index + 1))),
                    modifiers: .command
                )
            }

            Divider()

            Button("Logbook") {
                sidebarItem?.wrappedValue = .logbook
            }

            Divider()

            Button("Habits") {
                sidebarItem?.wrappedValue = .habits
            }
            .keyboardShortcut("6", modifiers: .command)

            Button("Timer") {
                sidebarItem?.wrappedValue = .timer
            }
            .keyboardShortcut("7", modifiers: .command)
        }

        // Task actions
        CommandMenu("Task") {
            Button("Mark Done") {
                completeTask?()
            }
            .keyboardShortcut(.space, modifiers: [])
            .disabled(completeTask == nil)

            Button("Toggle Evening") {
                toggleEvening?()
            }
            .keyboardShortcut("e", modifiers: .command)
            .disabled(toggleEvening == nil)

            // Defer is stubbed — needs date picker popover implementation
            Button("Defer…") {}
                .keyboardShortcut("d", modifiers: .command)
                .disabled(true)

            Divider()

            Button("Start/Stop Timer") {
                toggleTimer?()
            }
            .keyboardShortcut("t", modifiers: .command)
            .disabled(toggleTimer == nil)

            Button("Start Focus Session") {
                startFocus?()
            }
            .keyboardShortcut("f", modifiers: [.command, .shift])
            .disabled(startFocus == nil)
        }

        // Vault — lock
        CommandGroup(before: .appTermination) {
            Divider()

            Button("Lock Vault") {
                lockVault?()
            }
            .keyboardShortcut("l", modifiers: [.command, .option])
            .disabled(lockVault == nil)
        }
    }
}
