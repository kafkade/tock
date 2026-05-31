// NOTE: Shared with apps/ios — extract to shared package when apps/shared is created.

import SwiftUI

@Observable
@MainActor
final class ProjectsViewModel {
    private let client: any CoreClient

    var projects: [ProjectItem] = []
    var isLoading = false
    var error: String?
    var showNewProject = false
    var newProjectName = ""

    init(client: any CoreClient) {
        self.client = client
    }

    func load() async {
        isLoading = true
        error = nil
        do {
            projects = try await client.listProjects()
                .filter { $0.status == .active || $0.status == .paused }
        } catch {
            self.error = error.localizedDescription
        }
        isLoading = false
    }

    func addProject() async {
        guard !newProjectName.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        do {
            let project = try await client.addProject(
                NewProjectInput(name: newProjectName.trimmingCharacters(in: .whitespaces))
            )
            projects.append(project)
            newProjectName = ""
            showNewProject = false
        } catch {
            self.error = error.localizedDescription
        }
    }
}
