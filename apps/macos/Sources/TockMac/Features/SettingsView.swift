import SwiftUI

/// macOS Settings scene content — vault info, sync, and about.
///
/// Uses the native macOS `Settings` scene via `TockMacApp`. Presented
/// with ⌘, (handled automatically by SwiftUI).
struct SettingsView: View {
    @Environment(AppSessionState.self) private var appState

    var body: some View {
        TabView {
            generalTab
                .tabItem {
                    Label("General", systemImage: "gear")
                }

            vaultTab
                .tabItem {
                    Label("Vault", systemImage: "lock.shield")
                }

            syncTab
                .tabItem {
                    Label("Sync", systemImage: "arrow.triangle.2.circlepath")
                }

            aboutTab
                .tabItem {
                    Label("About", systemImage: "info.circle")
                }
        }
        .frame(width: 450, height: 300)
    }

    // MARK: - General

    @ViewBuilder
    private var generalTab: some View {
        Form {
            Section {
                Text("General preferences will be available in a future update.")
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }

    // MARK: - Vault

    @ViewBuilder
    private var vaultTab: some View {
        Form {
            Section("Vault") {
                LabeledContent("Path", value: appState.client.vaultPath())
                    .font(.caption)

                LabeledContent("Status") {
                    HStack(spacing: TockTheme.Spacing.xs) {
                        Circle()
                            .fill(vaultStatusColor)
                            .frame(width: 8, height: 8)
                        Text(vaultStatusLabel)
                    }
                }

                if case .unlocked = appState.vaultStatus {
                    Button("Lock Vault", role: .destructive) {
                        appState.lock()
                    }
                }
            }
        }
        .formStyle(.grouped)
    }

    private var vaultStatusColor: Color {
        switch appState.vaultStatus {
        case .unlocked: TockTheme.Colors.success
        case .locked: .secondary
        case .unlocking: TockTheme.Colors.warning
        case .error: TockTheme.Colors.destructive
        }
    }

    private var vaultStatusLabel: String {
        switch appState.vaultStatus {
        case .unlocked: "Unlocked"
        case .locked: "Locked"
        case .unlocking: "Unlocking…"
        case .error(let msg): "Error: \(msg)"
        }
    }

    // MARK: - Sync

    @ViewBuilder
    private var syncTab: some View {
        Form {
            Section("Sync") {
                Label("Not configured", systemImage: "icloud.slash")
                    .foregroundStyle(.secondary)

                Text("Sync will be available in a future update.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }

    // MARK: - About

    @ViewBuilder
    private var aboutTab: some View {
        Form {
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
        .formStyle(.grouped)
    }
}
