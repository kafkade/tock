// TockSwift — account onboarding helpers (signup, login, Setup Code).
//
// Thin idiomatic wrappers over the UniFFI account surface in `tock-uniffi`.
// HTTP stays on the app edge (URLSession); these just orchestrate SRP and the
// signup artifacts. Secrets cross as hex; the password is never stored.

import Foundation

@_exported import TockFFI

/// Drives the SRP login state machine across the three network round-trips.
///
/// 1. `start()` → POST `srp/start` with `startRequestJSON`.
/// 2. `finish(...)` with the start response → POST `srp/finish`.
/// 3. `verify(...)` with the finish response → session material.
public final class AccountLoginSession: @unchecked Sendable {
    private let handle: AccountLogin

    private init(handle: AccountLogin) {
        self.handle = handle
    }

    /// Begin a login for `email`. Returns the session and the JSON body to
    /// POST to `/v1/auth/srp/start`.
    public static func start(email: String) async throws -> (AccountLoginSession, String) {
        let h = try accountLoginStart(username: email)
        return (AccountLoginSession(handle: h), h.startRequestJson())
    }

    /// Feed the `srp/start` response; returns the `srp/finish` request body.
    public func finish(startResponseJSON: String, password: String, secretKey: String) throws -> String {
        try handle.finish(
            startResponseJson: startResponseJSON,
            password: password,
            secretKey: secretKey
        )
    }

    /// Verify the `srp/finish` response, yielding bearer + channel-binding.
    public func verify(finishResponseJSON: String) throws -> TockSessionMaterial {
        try handle.verify(finishResponseJson: finishResponseJSON)
    }
}

/// Decode a `TOCK1:` Setup Code into prefill fields.
public func parseTockSetupCode(_ code: String) throws -> TockSetupCode {
    try parseSetupCode(code: code)
}

/// Render an Emergency Kit text block (matches signup layout).
public func tockEmergencyKitText(serverURL: String, email: String, secretKey: String) -> String {
    emergencyKitText(serverUrl: serverURL, email: email, secretKey: secretKey)
}
