import SwiftUI

/// Settings view — vault info, sync, and preferences.
struct SettingsView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        List {
            Section("Vault") {
                LabeledContent("Path", value: appState.client.vaultPath())
                    .font(.caption)

                LabeledContent("Status") {
                    HStack(spacing: TockTheme.Spacing.xs) {
                        Circle()
                            .fill(TockTheme.Colors.success)
                            .frame(width: 8, height: 8)
                        Text("Unlocked")
                    }
                }

                Button("Lock Vault", role: .destructive) {
                    appState.lock()
                }
            }

            Section("Sync") {
                Label("Not configured", systemImage: "icloud.slash")
                    .foregroundStyle(.secondary)

                Text("Sync will be available in a future update.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section("About") {
                LabeledContent("Version", value: "0.1.0")
                LabeledContent("Build", value: "1")

                Link(destination: URL(string: "https://github.com/kafkade/tock")!) {
                    Label("Source Code", systemImage: "chevron.left.forwardslash.chevron.right")
                }

                Link(destination: URL(string: "https://github.com/kafkade/tock/blob/main/LICENSE-APACHE")!) {
                    Label("License (Apache-2.0)", systemImage: "doc.text")
                }
            }
        }
        .navigationTitle("Settings")
    }
}
