import SwiftUI
import TockSwift
import CoreImage
import CoreImage.CIFilterBuiltins

#if canImport(UIKit)
import UIKit
private typealias PlatformImage = UIImage
#elseif canImport(AppKit)
import AppKit
private typealias PlatformImage = NSImage
#endif

struct PairNewDeviceSheet: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    @State private var inviteSession: TockPairingInviteSession?
    @State private var invite: TockPairingInvite?
    @State private var inviteCode = ""
    @State private var responseCode = ""
    @State private var message: String?
    @State private var isBusy = false

    var body: some View {
        NavigationStack {
            Form {
                Section("Invite") {
                    if !inviteCode.isEmpty {
                        QRCodeCardView(payload: inviteCode)
                        Text(inviteCode)
                            .font(.system(.footnote, design: .monospaced))
                            .textSelection(.enabled)
                    } else {
                        Text("Generate an invite after saving your sync settings.")
                            .foregroundStyle(.secondary)
                    }

                    Button("Generate Invite") {
                        Task { await generateInvite() }
                    }
                    .disabled(isBusy)
                }

                Section("Acceptor response") {
                    TextField("Paste the response code from the new device", text: $responseCode, axis: .vertical)
                        .platformTextInputAutocapitalizationCharacters()
                        .autocorrectionDisabled()

                    Button("Upload Vault Key") {
                        Task { await uploadBlob() }
                    }
                    .disabled(isBusy || inviteSession == nil || responseCode.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }

                if let message {
                    Section("Status") {
                        Text(message)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .navigationTitle("Pair New Device")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }

    private func generateInvite() async {
        isBusy = true
        defer { isBusy = false }

        do {
            await appState.saveSyncSettings()
            let session = try await appState.beginPairingInvite()
            let invite = try await session.invite()
            inviteSession = session
            self.invite = invite
            inviteCode = try PairingCodeCodec.encodeInvite(invite)
            message = "Share the QR or code with the new device, then paste its response code here."
        } catch {
            message = error.localizedDescription
        }
    }

    private func uploadBlob() async {
        guard let inviteSession else { return }
        isBusy = true
        defer { isBusy = false }

        do {
            try await appState.uploadOnboardingBlob(
                session: inviteSession,
                responseCode: responseCode
            )
            message = "Vault key uploaded. The new device can finish pairing now."
        } catch {
            message = error.localizedDescription
        }
    }
}

struct JoinExistingVaultSheet: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    @State private var inviteCode = ""
    @State private var hostedToken = ""
    @State private var deviceLabel = ""
    @State private var password = ""
    @State private var confirmPassword = ""
    @State private var secretKey = ""
    @State private var acceptSession: TockPairingAcceptSession?
    @State private var invite: TockPairingInvite?
    @State private var responseCode = ""
    @State private var message: String?
    @State private var isBusy = false

    var body: some View {
        NavigationStack {
            Form {
                Section("Invite") {
                    TextField("Paste the invite code", text: $inviteCode, axis: .vertical)
                        .platformTextInputAutocapitalizationCharacters()
                        .autocorrectionDisabled()

                    TextField("Hosted auth token (optional)", text: $hostedToken)
                        .platformTextInputAutocapitalizationNever()
                        .autocorrectionDisabled()

                    TextField("Device label (optional)", text: $deviceLabel)

                    SecureField("New local password", text: $password)
                    SecureField("Confirm password", text: $confirmPassword)

                    TextField("Account Secret Key (A4-…)", text: $secretKey, axis: .vertical)
                        .platformTextInputAutocapitalizationCharacters()
                        .autocorrectionDisabled()

                    Button("Generate Response Code") {
                        Task { await generateResponseCode() }
                    }
                    .disabled(isBusy || inviteCode.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || password.isEmpty || confirmPassword.isEmpty)
                }

                if !responseCode.isEmpty {
                    Section("Response code") {
                        Text(responseCode)
                            .font(.system(.footnote, design: .monospaced))
                            .textSelection(.enabled)
                        Text("Give this code to the existing device, then come back here to finish pairing.")
                            .foregroundStyle(.secondary)
                    }

                    Section {
                        Button("Finish Pairing") {
                            Task { await finishPairing() }
                        }
                        .disabled(isBusy || acceptSession == nil || invite == nil)
                    }
                }

                if let message {
                    Section("Status") {
                        Text(message)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .navigationTitle("Join Existing Vault")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }

    private func generateResponseCode() async {
        guard password == confirmPassword else {
            message = "Passwords do not match."
            return
        }

        isBusy = true
        defer { isBusy = false }

        do {
            let invite = try PairingCodeCodec.decodeInvite(inviteCode)
            let session = try await TockWorkspace.beginPairingAccept()
            let details = try await session.details()
            self.invite = invite
            acceptSession = session
            responseCode = try PairingCodeCodec.encodeAcceptor(details)
            message = "Response code generated. Give it to the existing device, then finish pairing once it uploads the vault key."
        } catch {
            message = error.localizedDescription
        }
    }

    private func finishPairing() async {
        guard let acceptSession, let invite else { return }
        let trimmedSecretKey = secretKey.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedSecretKey.isEmpty else {
            message = "Enter your account Secret Key (from your Emergency Kit) to join this account."
            return
        }
        isBusy = true
        defer { isBusy = false }

        do {
            let details = try await acceptSession.details()
            let token = normalized(hostedToken)
            let blob = try await pollForBlob(invite: invite, rendezvousDeviceID: details.rendezvousDeviceId, authToken: token)
            if let token {
                try KeychainService.saveSyncAuthToken(token)
            } else {
                KeychainService.deleteSyncAuthToken()
            }
            let workspace = try await acceptSession.completeOnboarding(
                path: AppGroup.vaultPath(),
                password: Data(password.utf8),
                secretKey: trimmedSecretKey,
                invite: invite,
                blob: blob,
                deviceLabel: normalized(deviceLabel)
            )
            // Cache the Secret Key so this device can reopen the vault later.
            try KeychainService.saveSecretKey(trimmedSecretKey)
            await appState.adoptPairedWorkspace(workspace, password: password)
            dismiss()
        } catch {
            message = error.localizedDescription
        }
    }

    private func pollForBlob(
        invite: TockPairingInvite,
        rendezvousDeviceID: String,
        authToken: String?
    ) async throws -> Data {
        for _ in 0..<150 {
            if let blob = try await SyncClient.fetchOnboardingBlob(
                invite: invite,
                rendezvousDeviceID: rendezvousDeviceID,
                authToken: authToken
            ) {
                return blob
            }
            try await Task.sleep(for: .seconds(2))
        }
        throw SyncClientError.invalidResponse
    }

    private func normalized(_ value: String) -> String? {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}

private struct QRCodeCardView: View {
    let payload: String

    var body: some View {
        if let image = qrImage(payload: payload) {
            #if canImport(UIKit)
            Image(uiImage: image)
                .interpolation(.none)
                .resizable()
                .scaledToFit()
                .frame(maxWidth: 220)
                .frame(maxWidth: .infinity)
            #elseif canImport(AppKit)
            Image(nsImage: image)
                .interpolation(.none)
                .resizable()
                .scaledToFit()
                .frame(maxWidth: 220)
                .frame(maxWidth: .infinity)
            #endif
        }
    }

    private func qrImage(payload: String) -> PlatformImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.setValue(Data(payload.utf8), forKey: "inputMessage")
        filter.correctionLevel = "M"
        guard let output = filter.outputImage?.transformed(by: CGAffineTransform(scaleX: 8, y: 8)) else {
            return nil
        }
        let context = CIContext()
        guard let cgImage = context.createCGImage(output, from: output.extent) else {
            return nil
        }
        #if canImport(UIKit)
        return UIImage(cgImage: cgImage)
        #elseif canImport(AppKit)
        return NSImage(cgImage: cgImage, size: .zero)
        #endif
    }
}
