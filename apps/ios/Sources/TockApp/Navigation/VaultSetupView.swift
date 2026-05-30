import SwiftUI

/// Vault create / unlock screen.
///
/// Shown when `AppState.vaultStatus` is `.locked` or `.error`.
struct VaultSetupView: View {
    @Environment(AppState.self) private var appState

    @State private var password = ""
    @State private var confirmPassword = ""
    @State private var isCreating = false
    @State private var showCreate = false

    var body: some View {
        NavigationStack {
            VStack(spacing: TockTheme.Spacing.xxl) {
                Spacer()

                // App icon placeholder
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
                    }

                    SecureField("Master password", text: $password)
                        .textFieldStyle(.roundedBorder)
                        .textContentType(.password)

                    if showCreate {
                        SecureField("Confirm password", text: $confirmPassword)
                            .textFieldStyle(.roundedBorder)
                            .textContentType(.newPassword)
                    }

                    Button {
                        Task { await unlock() }
                    } label: {
                        if case .unlocking = appState.vaultStatus {
                            ProgressView()
                                .frame(maxWidth: .infinity)
                        } else {
                            Text(showCreate ? "Create Vault" : "Unlock")
                                .frame(maxWidth: .infinity)
                        }
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
                    .disabled(password.isEmpty || isUnlocking)

                    Button(showCreate ? "I have a vault" : "Create new vault") {
                        withAnimation {
                            showCreate.toggle()
                            confirmPassword = ""
                        }
                    }
                    .font(.caption)
                }
                .padding(.horizontal, TockTheme.Spacing.xxl)

                Spacer()
            }
            .navigationBarHidden(true)
        }
    }

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
        let docs = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first!
        return docs.appendingPathComponent("tock.vault").path
    }
}
