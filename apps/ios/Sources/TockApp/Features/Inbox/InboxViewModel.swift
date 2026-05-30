import SwiftUI

@Observable
@MainActor
final class InboxViewModel {
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
            tasks = try await client.listTasks(filter: .inbox)
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

    func deleteTask(_ task: TaskItem) async {
        do {
            try await client.deleteTask(id: task.id)
            tasks.removeAll { $0.id == task.id }
        } catch {
            self.error = error.localizedDescription
        }
    }
}
