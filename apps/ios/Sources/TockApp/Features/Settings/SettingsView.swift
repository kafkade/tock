import SwiftUI

/// Settings view — vault info, security, sync, and preferences.
struct SettingsView: View {
    @Environment(AppState.self) private var appState

    @State private var showEnableBiometricAlert = false
    @State private var showDisableBiometricAlert = false
    @State private var biometricError: String?

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

            securitySection

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

    // MARK: - Security section

    @ViewBuilder
    private var securitySection: some View {
        let bioType = appState.biometricType

        Section {
            if appState.biometricAvailable {
                Toggle(isOn: biometricToggleBinding) {
                    Label(
                        "Unlock with \(bioType.label)",
                        systemImage: bioType.systemImage
                    )
                }
            } else {
                Label(
                    "Biometric unlock not available",
                    systemImage: "lock.slash"
                )
                .foregroundStyle(.secondary)
            }

            if let error = biometricError {
                Label(error, systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(.red)
                    .font(.caption)
            }
        } header: {
            Text("Security")
        } footer: {
            if appState.biometricAvailable {
                Text(
                    "\(bioType.label) does not replace your master password. "
                    + "It stores an unlock key in this device's secure Keychain "
                    + "so this device can open your vault after biometric authentication. "
                    + "You'll still need your master password after reinstalling the app, "
                    + "changing \(bioType.label) settings, or using another device."
                )
            }
        }
        .alert(
            "Enable \(bioType.label) Unlock",
            isPresented: $showEnableBiometricAlert
        ) {
            Button("Enable") {
                enableBiometrics()
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(
                "Your vault key will be stored in this device's Keychain, "
                + "protected by \(bioType.label). The key is automatically "
                + "removed if \(bioType.label) settings change or the app "
                + "is reinstalled."
            )
        }
        .alert(
            "Disable \(bioType.label) Unlock",
            isPresented: $showDisableBiometricAlert
        ) {
            Button("Disable", role: .destructive) {
                appState.disableBiometrics()
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(
                "The cached vault key will be removed from this device's "
                + "Keychain. You'll need your master password to unlock."
            )
        }
    }

    // MARK: - Biometric toggle binding

    private var biometricToggleBinding: Binding<Bool> {
        Binding<Bool>(
            get: { appState.biometricEnabled },
            set: { newValue in
                if newValue {
                    showEnableBiometricAlert = true
                } else {
                    showDisableBiometricAlert = true
                }
            }
        )
    }

    private func enableBiometrics() {
        do {
            try appState.enableBiometrics()
            biometricError = nil
        } catch {
            biometricError = error.localizedDescription
        }
    }
}
