// NOTE: Shared with apps/ios — extract to shared package when apps/shared is created.

import SwiftUI

@Observable
@MainActor
final class TimerViewModel {
    private let client: any CoreClient

    enum Mode: String, CaseIterable {
        case timer = "Timer"
        case focus = "Focus"
    }

    var mode: Mode = .timer
    var isLoading = false
    var error: String?

    // Timer state
    var currentBlock: TimeBlockItem?
    var recentBlocks: [TimeBlockItem] = []
    var newTimerTitle = ""

    // Focus state
    var activeFocus: FocusSessionItem?
    var focusCycles: UInt32 = 4

    init(client: any CoreClient) {
        self.client = client
    }

    func load() async {
        isLoading = true
        error = nil
        do {
            currentBlock = try await client.currentTimer()
            recentBlocks = try await client.listTimeBlocks()
            activeFocus = try await client.focusStatus()
        } catch {
            self.error = error.localizedDescription
        }
        isLoading = false
    }

    // MARK: Timer actions

    func startTimer() async {
        guard !newTimerTitle.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        do {
            currentBlock = try await client.startTimer(
                title: newTimerTitle.trimmingCharacters(in: .whitespaces),
                taskId: nil
            )
            newTimerTitle = ""
        } catch {
            self.error = error.localizedDescription
        }
    }

    func stopTimer() async {
        do {
            if let stopped = try await client.stopTimer() {
                currentBlock = nil
                recentBlocks.insert(stopped, at: 0)
            }
        } catch {
            self.error = error.localizedDescription
        }
    }

    // MARK: Focus actions

    func startFocus() async {
        do {
            activeFocus = try await client.startFocus(taskId: nil, cycles: focusCycles)
        } catch {
            self.error = error.localizedDescription
        }
    }

    func completeCycle() async {
        do { activeFocus = try await client.completeFocusCycle() } catch { self.error = error.localizedDescription }
    }

    func skipBreak() async {
        do { activeFocus = try await client.skipBreak() } catch { self.error = error.localizedDescription }
    }

    func pauseFocus() async {
        do { activeFocus = try await client.pauseFocus() } catch { self.error = error.localizedDescription }
    }

    func resumeFocus() async {
        do { activeFocus = try await client.resumeFocus() } catch { self.error = error.localizedDescription }
    }

    func abortFocus() async {
        do {
            _ = try await client.abortFocus()
            activeFocus = nil
        } catch {
            self.error = error.localizedDescription
        }
    }
}
