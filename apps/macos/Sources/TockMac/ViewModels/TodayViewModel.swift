// NOTE: Shared with apps/ios — extract to shared package when apps/shared is created.

import SwiftUI

@Observable
@MainActor
final class TodayViewModel {
    private let client: any CoreClient

    var tasks: [TaskItem] = []
    var isLoading = false
    var error: String?

    init(client: any CoreClient) {
        self.client = client
    }

    func load() async {
        isLoading = true
        error = nil
        do {
            tasks = try await client.listTasks(filter: .today)
                .sorted { $0.urgency > $1.urgency }
        } catch {
            self.error = error.localizedDescription
        }
        isLoading = false
    }

    func completeTask(_ task: TaskItem) async {
        do {
            try await client.completeTask(id: task.id)
            tasks.removeAll { $0.id == task.id }
        } catch {
            self.error = error.localizedDescription
        }
    }
}
