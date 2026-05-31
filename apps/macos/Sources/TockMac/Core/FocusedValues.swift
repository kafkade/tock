import SwiftUI

// MARK: - FocusedValue keys for per-window keyboard shortcut routing

/// The currently selected sidebar item in the active window.
struct FocusedSidebarItemKey: FocusedValueKey {
    typealias Value = Binding<SidebarItem>
}

/// The currently selected task ID in the active window.
struct FocusedSelectedTaskKey: FocusedValueKey {
    typealias Value = Binding<String?>
}

/// Action to show the quick-add sheet in the active window.
struct FocusedQuickAddActionKey: FocusedValueKey {
    typealias Value = () -> Void
}

/// Action to complete the currently selected task.
struct FocusedCompleteTaskActionKey: FocusedValueKey {
    typealias Value = () -> Void
}

/// Action to toggle evening on the currently selected task.
struct FocusedToggleEveningActionKey: FocusedValueKey {
    typealias Value = () -> Void
}

/// Action to start/stop timer on the currently selected task.
struct FocusedToggleTimerActionKey: FocusedValueKey {
    typealias Value = () -> Void
}

/// Action to start a focus session.
struct FocusedStartFocusActionKey: FocusedValueKey {
    typealias Value = () -> Void
}

/// Action to lock the vault (macOS-specific).
struct FocusedLockVaultActionKey: FocusedValueKey {
    typealias Value = () -> Void
}

/// Action to show quick entry panel (macOS-specific).
struct FocusedQuickEntryActionKey: FocusedValueKey {
    typealias Value = () -> Void
}

extension FocusedValues {
    var sidebarItem: Binding<SidebarItem>? {
        get { self[FocusedSidebarItemKey.self] }
        set { self[FocusedSidebarItemKey.self] = newValue }
    }

    var selectedTask: Binding<String?>? {
        get { self[FocusedSelectedTaskKey.self] }
        set { self[FocusedSelectedTaskKey.self] = newValue }
    }

    var quickAddAction: (() -> Void)? {
        get { self[FocusedQuickAddActionKey.self] }
        set { self[FocusedQuickAddActionKey.self] = newValue }
    }

    var completeTaskAction: (() -> Void)? {
        get { self[FocusedCompleteTaskActionKey.self] }
        set { self[FocusedCompleteTaskActionKey.self] = newValue }
    }

    var toggleEveningAction: (() -> Void)? {
        get { self[FocusedToggleEveningActionKey.self] }
        set { self[FocusedToggleEveningActionKey.self] = newValue }
    }

    var toggleTimerAction: (() -> Void)? {
        get { self[FocusedToggleTimerActionKey.self] }
        set { self[FocusedToggleTimerActionKey.self] = newValue }
    }

    var startFocusAction: (() -> Void)? {
        get { self[FocusedStartFocusActionKey.self] }
        set { self[FocusedStartFocusActionKey.self] = newValue }
    }

    var lockVaultAction: (() -> Void)? {
        get { self[FocusedLockVaultActionKey.self] }
        set { self[FocusedLockVaultActionKey.self] = newValue }
    }

    var quickEntryAction: (() -> Void)? {
        get { self[FocusedQuickEntryActionKey.self] }
        set { self[FocusedQuickEntryActionKey.self] = newValue }
    }
}
