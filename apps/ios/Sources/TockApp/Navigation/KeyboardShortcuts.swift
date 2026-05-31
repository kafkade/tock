import SwiftUI

/// Keyboard shortcut commands for iPad external keyboards and macOS.
///
/// Applied via `.commands { TockCommands() }` on the WindowGroup scene.
/// Routes actions through `FocusedValues` to the active window's state.
struct TockCommands: Commands {
    @FocusedValue(\.quickAddAction) private var quickAdd
    @FocusedValue(\.sidebarItem) private var sidebarItem
    @FocusedValue(\.completeTaskAction) private var completeTask
    @FocusedValue(\.toggleEveningAction) private var toggleEvening
    @FocusedValue(\.toggleTimerAction) private var toggleTimer
    @FocusedValue(\.startFocusAction) private var startFocus

    var body: some Commands {
        // Task commands
        CommandGroup(after: .newItem) {
            Button("New Task") {
                quickAdd?()
            }
            .keyboardShortcut("n", modifiers: .command)
        }

        // View switching — ⌘1 through ⌘5
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
    }
}
