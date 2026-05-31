import SwiftUI

/// Vault create / unlock screen — macOS-adapted.
///
/// Shown when `AppSessionState.vaultStatus` is `.locked` or `.error`.
/// Uses macOS-native form layout without iOS-specific modifiers.
struct VaultSetupView: View {
    @Environment(AppSessionState.self) private var appState

    @State private var password = ""
    @State private var confirmPassword = ""
    @State private var showCreate = false

    var body: some View {
        VStack(spacing: TockTheme.Spacing.xxl) {
            Spacer()

            Image(systemName: "checkmark.seal.fill")
                .font(.system(size: 80))
                .foregroundStyle(TockTheme.Colors.accent)

            Text("tock")
                .font(.largeTitle)
                .bold()

            Text("Your encrypted productivity vault")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            Spacer()

            VStack(spacing: TockTheme.Spacing.md) {
                if case .error(let message) = appState.vaultStatus {
                    Label(message, systemImage: "exclamationmark.triangle.fill")
                        .foregroundStyle(.red)
                        .font(.caption)
                        .multilineTextAlignment(.center)
                }

                SecureField("Master password", text: $password)
                    .textFieldStyle(.roundedBorder)

                if showCreate {
                    SecureField("Confirm password", text: $confirmPassword)
                        .textFieldStyle(.roundedBorder)
                }

                Button {
                    Task { await unlock() }
                } label: {
                    if case .unlocking = appState.vaultStatus {
                        ProgressView()
                            .controlSize(.small)
                            .frame(maxWidth: .infinity)
                    } else {
                        Text(showCreate ? "Create Vault" : "Unlock")
                            .frame(maxWidth: .infinity)
                    }
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(password.isEmpty || isUnlocking)
                .keyboardShortcut(.defaultAction)

                Button(showCreate ? "I have a vault" : "Create new vault") {
                    withAnimation {
                        showCreate.toggle()
                        confirmPassword = ""
                    }
                }
                .font(.caption)
                .buttonStyle(.plain)
            }
            .frame(maxWidth: 300)

            Spacer()
        }
        .frame(minWidth: 400, minHeight: 400)
    }

    // MARK: - Helpers

    private var isUnlocking: Bool {
        if case .unlocking = appState.vaultStatus { return true }
        return false
    }

    private func unlock() async {
        if showCreate {
            guard password == confirmPassword else {
                appState.vaultStatus = .error("Passwords don't match")
                return
            }
            await appState.createVault(path: defaultVaultPath(), password: password)
        } else {
            await appState.unlock(path: defaultVaultPath(), password: password)
        }
    }

    private func defaultVaultPath() -> String {
        let appSupport = FileManager.default.urls(
            for: .applicationSupportDirectory, in: .userDomainMask
        ).first!
        let tockDir = appSupport.appendingPathComponent("tock")
        try? FileManager.default.createDirectory(at: tockDir, withIntermediateDirectories: true)
        return tockDir.appendingPathComponent("vault.tock").path
    }
}
