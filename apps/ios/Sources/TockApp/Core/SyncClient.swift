import Foundation
import TockSwift

struct SyncRunResult: Sendable {
    let pushed: Int
    let pulled: Int
    let conflicts: Int
}

enum SyncClientError: LocalizedError {
    case missingServerURL
    case invalidServerURL(String)
    case invalidResponse
    case invalidPayload

    var errorDescription: String? {
        switch self {
        case .missingServerURL:
            "Set a sync server URL before syncing."
        case .invalidServerURL(let value):
            "Invalid sync server URL: \(value)"
        case .invalidResponse:
            "The sync server returned an invalid response."
        case .invalidPayload:
            "The sync server returned malformed sync data."
        }
    }
}

actor SyncClient {
    private let workspace: TockWorkspace
    private let session: URLSession

    init(workspace: TockWorkspace, session: URLSession = .shared) {
        self.workspace = workspace
        self.session = session
    }

    func sync(authToken: String?) async throws -> SyncRunResult {
        let info = try await workspace.syncDeviceInfo()
        let serverURL = try Self.normalizedBaseURL(info.serverUrl)

        try await registerDevice(info: info, serverURL: serverURL, authToken: authToken)

        let outbound = try await workspace.collectSyncLocalChanges()
        let pushed = try await push(
            vaultID: info.vaultId,
            serverURL: serverURL,
            authToken: authToken,
            events: outbound
        )

        var cursor = info.pullCursor
        var pulled = 0
        var conflicts = 0

        while true {
            let page = try await pull(
                vaultID: info.vaultId,
                serverURL: serverURL,
                authToken: authToken,
                after: cursor,
                limit: 256
            )
            if !page.frames.isEmpty {
                let summary = try await workspace.ingestSyncFrames(page.frames)
                pulled += Int(summary.applied)
                conflicts += Int(summary.conflicts)
            }
            cursor = page.cursor
            try await workspace.setSyncPullCursor(cursor)
            if !page.more { break }
        }

        return SyncRunResult(pushed: pushed, pulled: pulled, conflicts: conflicts)
    }

    static func putOnboardingBlob(
        invite: TockPairingInvite,
        targetDeviceID: String,
        blob: Data,
        authToken: String?,
        session: URLSession = .shared
    ) async throws {
        let serverURL = try normalizedBaseURL(invite.serverUrl)
        let body = BlobRequest(blob: blob.base64EncodedString())
        let request = try makeRequest(
            serverURL: serverURL,
            path: "/v1/vaults/\(hexVaultPath(invite.vaultId))/onboarding/\(targetDeviceID)",
            method: "PUT",
            authToken: authToken,
            body: body
        )
        let (_, response) = try await session.data(for: request)
        try ensureSuccess(response)
    }

    static func fetchOnboardingBlob(
        invite: TockPairingInvite,
        rendezvousDeviceID: String,
        authToken: String?,
        session: URLSession = .shared
    ) async throws -> Data? {
        let serverURL = try normalizedBaseURL(invite.serverUrl)
        let request = try makeRequest(
            serverURL: serverURL,
            path: "/v1/vaults/\(hexVaultPath(invite.vaultId))/onboarding/\(rendezvousDeviceID)",
            method: "GET",
            authToken: authToken,
            body: Optional<EmptyBody>.none
        )
        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse else {
            throw SyncClientError.invalidResponse
        }
        if http.statusCode == 404 { return nil }
        try ensureSuccess(http)
        let decoded = try JSONDecoder().decode(BlobResponse.self, from: data)
        guard let blob = Data(base64Encoded: decoded.blob) else {
            throw SyncClientError.invalidPayload
        }
        return blob
    }

    private func registerDevice(
        info: TockSyncDeviceInfo,
        serverURL: URL,
        authToken: String?
    ) async throws {
        let body = RegisterDeviceRequest(
            deviceId: info.deviceId,
            verifyingKey: info.verifyingKey,
            label: info.deviceLabel
        )
        let request = try Self.makeRequest(
            serverURL: serverURL,
            path: "/v1/vaults/\(Self.hexVaultPath(info.vaultId))/devices",
            method: "POST",
            authToken: authToken,
            body: body
        )
        let (_, response) = try await session.data(for: request)
        try Self.ensureSuccess(response)
    }

    private func push(
        vaultID: String,
        serverURL: URL,
        authToken: String?,
        events: [TockSyncEventFrame]
    ) async throws -> Int {
        guard !events.isEmpty else { return 0 }
        let body = PushEventsRequest(
            events: events.map {
                PushEventItem(
                    eventId: $0.eventId,
                    deviceId: $0.deviceId,
                    lamport: Int64($0.lamport),
                    payload: $0.payload.base64EncodedString()
                )
            }
        )
        let request = try Self.makeRequest(
            serverURL: serverURL,
            path: "/v1/vaults/\(Self.hexVaultPath(vaultID))/events/push",
            method: "POST",
            authToken: authToken,
            body: body
        )
        let (data, response) = try await session.data(for: request)
        try Self.ensureSuccess(response)
        return try JSONDecoder().decode(PushEventsResponse.self, from: data).accepted
    }

    private func pull(
        vaultID: String,
        serverURL: URL,
        authToken: String?,
        after: UInt64,
        limit: Int
    ) async throws -> PullPage {
        var components = URLComponents(
            url: serverURL.appending(path: "/v1/vaults/\(Self.hexVaultPath(vaultID))/events/pull"),
            resolvingAgainstBaseURL: false
        )
        components?.queryItems = [
            URLQueryItem(name: "after", value: String(after)),
            URLQueryItem(name: "limit", value: String(limit)),
        ]
        guard let url = components?.url else {
            throw SyncClientError.invalidServerURL(serverURL.absoluteString)
        }
        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        if let authToken, !authToken.isEmpty {
            request.setValue("Bearer \(authToken)", forHTTPHeaderField: "Authorization")
        }
        let (data, response) = try await session.data(for: request)
        try Self.ensureSuccess(response)
        let decoded = try JSONDecoder().decode(PullEventsResponse.self, from: data)
        let frames = try decoded.events.map { item in
            guard let payload = Data(base64Encoded: item.payload) else {
                throw SyncClientError.invalidPayload
            }
            return payload
        }
        return PullPage(
            frames: frames,
            cursor: UInt64(max(decoded.cursor, 0)),
            more: decoded.more
        )
    }

    private static func makeRequest<Body: Encodable>(
        serverURL: URL,
        path: String,
        method: String,
        authToken: String?,
        body: Body?
    ) throws -> URLRequest {
        var request = URLRequest(url: serverURL.appending(path: path))
        request.httpMethod = method
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        if let authToken, !authToken.isEmpty {
            request.setValue("Bearer \(authToken)", forHTTPHeaderField: "Authorization")
        }
        if let body {
            let encoder = JSONEncoder()
            encoder.keyEncodingStrategy = .convertToSnakeCase
            request.httpBody = try encoder.encode(body)
        }
        return request
    }

    private static func ensureSuccess(_ response: URLResponse) throws {
        guard let http = response as? HTTPURLResponse else {
            throw SyncClientError.invalidResponse
        }
        guard (200..<300).contains(http.statusCode) else {
            throw SyncClientError.invalidResponse
        }
    }

    private static func normalizedBaseURL(_ raw: String?) throws -> URL {
        guard let raw, !raw.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            throw SyncClientError.missingServerURL
        }
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let url = URL(string: trimmed) else {
            throw SyncClientError.invalidServerURL(trimmed)
        }
        return url
    }

    private static func hexVaultPath(_ value: String) -> String {
        value.replacingOccurrences(of: "-", with: "").lowercased()
    }
}

private struct PullPage: Sendable {
    let frames: [Data]
    let cursor: UInt64
    let more: Bool
}

private struct EmptyBody: Encodable {}

private struct RegisterDeviceRequest: Encodable {
    let deviceId: String
    let verifyingKey: String
    let label: String?
}

private struct PushEventsRequest: Encodable {
    let events: [PushEventItem]
}

private struct PushEventItem: Encodable {
    let eventId: String
    let deviceId: String
    let lamport: Int64
    let payload: String
}

private struct PushEventsResponse: Decodable {
    let accepted: Int
}

private struct PullEventsResponse: Decodable {
    let events: [PullEventItem]
    let cursor: Int64
    let more: Bool
}

private struct PullEventItem: Decodable {
    let payload: String
}

private struct BlobRequest: Encodable {
    let blob: String
}

private struct BlobResponse: Decodable {
    let blob: String
}
