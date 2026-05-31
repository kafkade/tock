import SwiftUI

/// Vault create / unlock screen.
///
/// Shown when `AppState.vaultStatus` is `.locked` or `.error`.
/// Offers biometric unlock (Face ID / Touch ID) when available and enabled,
/// with master password as fallback.
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
                    .accessibilityHidden(true)

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
                            .accessibilityLabel("Error")
                            .accessibilityValue(message)
                    }

                    // Biometric unlock button — shown prominently when available
                    if appState.canOfferBiometricUnlock && !showCreate {
                        biometricUnlockButton
                    }

                    // Password form
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
                    .accessibilityHint(primaryActionAccessibilityHint)

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
            .onAppear {
                attemptAutoUnlock()
            }
        }
    }

    // MARK: - Biometric button

    @ViewBuilder
    private var biometricUnlockButton: some View {
        let bioType = appState.biometricType

        Button {
            Task { await appState.unlockWithBiometrics() }
        } label: {
            Label("Unlock with \(bioType.label)", systemImage: bioType.systemImage)
                .frame(maxWidth: .infinity)
        }
        .buttonStyle(.bordered)
        .controlSize(.large)
        .tint(TockTheme.Colors.accent)
    }

    // MARK: - Auto-unlock

    /// Auto-trigger biometric unlock on first appearance if conditions are met.
    private func attemptAutoUnlock() {
        guard appState.canOfferBiometricUnlock,
              !appState.didAttemptAutoUnlock else { return }

        appState.didAttemptAutoUnlock = true
        Task {
            // Small delay to let the view appear before showing biometric prompt
            try? await Task.sleep(for: .milliseconds(300))
            await appState.unlockWithBiometrics()
        }
    }

    // MARK: - Helpers

    private var isUnlocking: Bool {
        if case .unlocking = appState.vaultStatus { return true }
        return false
    }

    private var primaryActionAccessibilityHint: String {
        if isUnlocking {
            return "Wait for the current vault action to finish."
        }

        if password.isEmpty {
            return showCreate
                ? "Enter a master password to create your vault."
                : "Enter your master password to unlock the vault."
        }

        return showCreate
            ? "Creates a new encrypted vault using the entered master password."
            : "Unlocks your encrypted vault using the entered master password."
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
