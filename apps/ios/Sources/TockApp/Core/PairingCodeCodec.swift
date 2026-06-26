import Foundation
import TockSwift

enum PairingCodeCodec {
    static func encodeInvite(_ invite: TockPairingInvite) throws -> String {
        let payload = InvitePayload(
            vaultId: invite.vaultId,
            serverUrl: invite.serverUrl,
            inviterPubkey: invite.inviterPubkey,
            inviterFingerprint: invite.inviterFingerprint,
            createdAt: invite.createdAt
        )
        return try encode(payload)
    }

    static func decodeInvite(_ code: String) throws -> TockPairingInvite {
        let payload: InvitePayload = try decode(code)
        return TockPairingInvite(
            vaultId: payload.vaultId,
            serverUrl: payload.serverUrl,
            inviterPubkey: payload.inviterPubkey,
            inviterFingerprint: payload.inviterFingerprint,
            createdAt: payload.createdAt
        )
    }

    static func encodeAcceptor(_ details: TockPairingAcceptorInfo) throws -> String {
        let payload = AcceptorPayload(
            accepterPubkey: details.accepterPubkey,
            accepterFingerprint: details.accepterFingerprint,
            rendezvousDeviceId: details.rendezvousDeviceId
        )
        return try encode(payload)
    }

    static func decodeAcceptor(_ code: String) throws -> TockPairingAcceptorInfo {
        let payload: AcceptorPayload = try decode(code)
        return TockPairingAcceptorInfo(
            accepterPubkey: payload.accepterPubkey,
            accepterFingerprint: payload.accepterFingerprint,
            rendezvousDeviceId: payload.rendezvousDeviceId
        )
    }

    private static func encode<Payload: Codable>(_ payload: Payload) throws -> String {
        let data = try JSONEncoder().encode(payload)
        return chunk(base64URL(data), groups: 6)
    }

    private static func decode<Payload: Codable>(_ code: String) throws -> Payload {
        let compact = code
            .split(whereSeparator: \.isWhitespace)
            .joined()
        guard let data = Data(base64URLEncoded: compact) else {
            throw PairingCodeError.invalidCode
        }
        do {
            return try JSONDecoder().decode(Payload.self, from: data)
        } catch {
            throw PairingCodeError.invalidCode
        }
    }

    private static func chunk(_ value: String, groups: Int) -> String {
        guard groups > 1, !value.isEmpty else { return value }
        let chars = Array(value)
        let base = chars.count / groups
        let remainder = chars.count % groups
        var start = 0
        var parts: [String] = []
        for index in 0..<groups {
            let size = base + (index < remainder ? 1 : 0)
            guard size > 0 else { continue }
            let end = start + size
            parts.append(String(chars[start..<end]))
            start = end
        }
        return parts.joined(separator: " ")
    }
}

enum PairingCodeError: LocalizedError {
    case invalidCode

    var errorDescription: String? {
        switch self {
        case .invalidCode:
            "That pairing code is not valid."
        }
    }
}

private struct InvitePayload: Codable {
    let vaultId: String
    let serverUrl: String
    let inviterPubkey: String
    let inviterFingerprint: String
    let createdAt: String
}

private struct AcceptorPayload: Codable {
    let accepterPubkey: String
    let accepterFingerprint: String
    let rendezvousDeviceId: String
}

private func base64URL(_ data: Data) -> String {
    data.base64EncodedString()
        .replacingOccurrences(of: "+", with: "-")
        .replacingOccurrences(of: "/", with: "_")
        .replacingOccurrences(of: "=", with: "")
}

private extension Data {
    init?(base64URLEncoded value: String) {
        var normalized = value
            .replacingOccurrences(of: "-", with: "+")
            .replacingOccurrences(of: "_", with: "/")
        let remainder = normalized.count % 4
        if remainder != 0 {
            normalized += String(repeating: "=", count: 4 - remainder)
        }
        self.init(base64Encoded: normalized)
    }
}
