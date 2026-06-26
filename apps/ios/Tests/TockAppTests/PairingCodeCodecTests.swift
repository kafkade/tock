import Foundation
import XCTest

@testable import TockApp
import TockSwift

final class PairingCodeCodecTests: XCTestCase {
    func testInviteRoundTripNormalizesWhitespace() throws {
        let invite = TockPairingInvite(
            vaultId: "550e8400-e29b-41d4-a716-446655440000",
            serverUrl: "https://sync.example.com",
            inviterPubkey: "pubkey-value",
            inviterFingerprint: "ABCD1234",
            createdAt: "2026-06-26T13:00:00Z"
        )

        let encoded = try PairingCodeCodec.encodeInvite(invite)
        let normalized = encoded.replacingOccurrences(of: " ", with: "\n")
        let decoded = try PairingCodeCodec.decodeInvite(normalized)

        XCTAssertEqual(decoded.vaultId, invite.vaultId)
        XCTAssertEqual(decoded.serverUrl, invite.serverUrl)
        XCTAssertEqual(decoded.inviterPubkey, invite.inviterPubkey)
        XCTAssertEqual(decoded.inviterFingerprint, invite.inviterFingerprint)
        XCTAssertEqual(decoded.createdAt, invite.createdAt)
        XCTAssertEqual(encoded.split(separator: " ").count, 6)
    }

    func testAcceptorRoundTripPreservesPayload() throws {
        let info = TockPairingAcceptorInfo(
            accepterPubkey: "acceptor-pubkey",
            accepterFingerprint: "EFGH5678",
            rendezvousDeviceId: "device-123"
        )

        let encoded = try PairingCodeCodec.encodeAcceptor(info)
        let decoded = try PairingCodeCodec.decodeAcceptor(encoded)

        XCTAssertEqual(decoded.accepterPubkey, info.accepterPubkey)
        XCTAssertEqual(decoded.accepterFingerprint, info.accepterFingerprint)
        XCTAssertEqual(decoded.rendezvousDeviceId, info.rendezvousDeviceId)
    }

    func testDecodeRejectsInvalidCode() {
        XCTAssertThrowsError(try PairingCodeCodec.decodeInvite("not-a-valid-code")) { error in
            guard case PairingCodeError.invalidCode = error else {
                return XCTFail("expected invalidCode, got \(error)")
            }
        }
    }
}
