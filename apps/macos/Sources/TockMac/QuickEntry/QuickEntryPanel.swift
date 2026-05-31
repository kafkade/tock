import AppKit
import SwiftUI

/// Manages the global hotkey and floating quick-entry panel.
///
/// Registers a Carbon global hotkey (⌃⌥Space) that opens a borderless
/// `NSPanel` for single-line natural-language task input. `⌘Return` submits,
/// `Esc` cancels.
///
/// The panel controller is retained at the app level and manages a single
/// `NSPanel` instance — panel lifetime does not depend on SwiftUI view lifecycle.
@MainActor
final class QuickEntryPanelController {
    private var panel: NSPanel?
    private var hotKeyRef: EventHotKeyRef?
    private var localMonitor: Any?
    private let appState: AppSessionState

    /// Unique hot key ID for the Carbon event system.
    private static let hotKeyID = EventHotKeyID(
        signature: OSType(0x746F636B), // "tock"
        id: 1
    )

    init(appState: AppSessionState) {
        self.appState = appState
    }

    // MARK: - Global Hotkey

    /// Register the global hotkey (⌃⌥Space).
    ///
    /// Uses the Carbon `RegisterEventHotKey` API for reliable system-wide
    /// hotkey capture without requiring accessibility permissions.
    func registerHotKey() {
        // Space = keycode 49, modifiers: control + option
        let modifiers: UInt32 = UInt32(controlKey | optionKey)
        let keyCode: UInt32 = 49 // kVK_Space

        var hotKeyID = Self.hotKeyID
        var eventType = EventTypeSpec(
            eventClass: OSType(kEventClassKeyboard),
            eventKind: UInt32(kEventHotKeyPressed)
        )

        // Install handler on the application event target
        let handler: EventHandlerUPP = { _, event, userData -> OSStatus in
            guard let userData else { return OSStatus(eventNotHandledErr) }
            let controller = Unmanaged<QuickEntryPanelController>.fromOpaque(userData)
                .takeUnretainedValue()

            var hotKeyID = EventHotKeyID()
            GetEventParameter(
                event,
                EventParamName(kEventParamDirectObject),
                EventParamType(typeEventHotKeyID),
                nil,
                MemoryLayout<EventHotKeyID>.size,
                nil,
                &hotKeyID
            )

            if hotKeyID.id == QuickEntryPanelController.hotKeyID.id {
                Task { @MainActor in
                    controller.togglePanel()
                }
            }
            return noErr
        }

        let selfPtr = Unmanaged.passUnretained(self).toOpaque()
        InstallEventHandler(
            GetApplicationEventTarget(),
            handler,
            1,
            &eventType,
            selfPtr,
            nil
        )

        RegisterEventHotKey(
            keyCode,
            modifiers,
            hotKeyID,
            GetApplicationEventTarget(),
            0,
            &hotKeyRef
        )
    }

    /// Unregister the global hotkey.
    func unregisterHotKey() {
        if let ref = hotKeyRef {
            UnregisterEventHotKey(ref)
            hotKeyRef = nil
        }
    }

    // MARK: - Panel Management

    /// Toggle the quick-entry panel visibility.
    func togglePanel() {
        if let panel, panel.isVisible {
            dismissPanel()
        } else {
            showPanel()
        }
    }

    /// Show the quick-entry panel.
    func showPanel() {
        guard case .unlocked = appState.vaultStatus else { return }

        if panel == nil {
            createPanel()
        }

        guard let panel else { return }

        // Position near the center of the active screen
        if let screen = NSScreen.main {
            let screenFrame = screen.visibleFrame
            let panelSize = panel.frame.size
            let origin = NSPoint(
                x: screenFrame.midX - panelSize.width / 2,
                y: screenFrame.midY + 100
            )
            panel.setFrameOrigin(origin)
        }

        panel.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    /// Dismiss the quick-entry panel.
    func dismissPanel() {
        panel?.orderOut(nil)
    }

    // MARK: - Panel Creation

    private func createPanel() {
        let contentView = QuickEntryContentView(
            appState: appState,
            onDismiss: { [weak self] in
                self?.dismissPanel()
            }
        )

        let hostingView = NSHostingView(rootView: contentView)

        let newPanel = QuickEntryNSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 600, height: 80),
            styleMask: [.nonactivatingPanel, .titled, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )

        newPanel.isMovableByWindowBackground = true
        newPanel.titlebarAppearsTransparent = true
        newPanel.titleVisibility = .hidden
        newPanel.backgroundColor = .clear
        newPanel.isOpaque = false
        newPanel.level = .floating
        newPanel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        newPanel.contentView = hostingView
        newPanel.hidesOnDeactivate = true

        // Monitor for Esc key to dismiss
        localMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            if event.keyCode == 53 { // Esc
                self?.dismissPanel()
                return nil
            }
            return event
        }

        panel = newPanel
    }

    deinit {
        if let monitor = localMonitor {
            NSEvent.removeMonitor(monitor)
        }
        unregisterHotKey()
    }
}

// MARK: - NSPanel subclass

/// Custom `NSPanel` that accepts keyboard input and becomes key window.
private class QuickEntryNSPanel: NSPanel {
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }
}

// MARK: - SwiftUI content for the panel

/// The SwiftUI content hosted inside the floating quick-entry panel.
///
/// Single text field for natural-language task input. `⌘Return` submits,
/// `Esc` cancels (handled by the panel controller).
private struct QuickEntryContentView: View {
    let appState: AppSessionState
    let onDismiss: () -> Void

    @State private var input = ""
    @State private var isSubmitting = false

    var body: some View {
        HStack(spacing: TockTheme.Spacing.md) {
            Image(systemName: "plus.circle.fill")
                .font(.title2)
                .foregroundStyle(TockTheme.Colors.accent)
                .accessibilityHidden(true)

            TextField("Add a task…", text: $input)
                .textFieldStyle(.plain)
                .font(.title3)
                .onSubmit {
                    Task { await submit() }
                }

            if isSubmitting {
                ProgressView()
                    .controlSize(.small)
            } else {
                Button {
                    Task { await submit() }
                } label: {
                    Text("Add")
                        .font(.caption)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.small)
                .keyboardShortcut(.return, modifiers: .command)
                .disabled(input.trimmingCharacters(in: .whitespaces).isEmpty)
                .accessibilityHint("Adds the task and closes panel")
            }
        }
        .padding(TockTheme.Spacing.lg)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: TockTheme.Radius.lg))
        .shadow(color: .black.opacity(0.2), radius: 20, y: 10)
        .padding(TockTheme.Spacing.sm)
    }

    private func submit() async {
        let trimmed = input.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return }

        isSubmitting = true
        let taskInput = NewTaskInput(title: trimmed)
        _ = try? await appState.client.addTask(taskInput)
        input = ""
        isSubmitting = false
        await appState.refreshMenuBarState()
        onDismiss()
    }
}
