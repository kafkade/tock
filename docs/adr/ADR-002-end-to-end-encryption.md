# ADR-002: End-to-end encryption with per-item envelope encryption

**Status:** Accepted (amended by [ADR-011](ADR-011-account-based-self-host-two-secret-auth.md))  
**Date:** 2026-05-20

> **Amended by [ADR-011](ADR-011-account-based-self-host-two-secret-auth.md):**
> - The password-only **Master Key** at the top of the hierarchy is **superseded** by a
>   two-secret **Unlock Root Key (URK)** = `Argon2id(password) XOR HKDF(secret_key)`. MEK now
>   derives from the URK, not from a password-only MK.
> - The **24-word recovery key** and the vault-header `vk_recover_ct` escrow path described below
>   are **superseded and removed**. The sole recovery path is the 1Password-style **Emergency Kit**
>   (Secret Key + sign-in address). Losing the Secret Key with no Emergency Kit is unrecoverable
>   by design.
>
> The rest of this ADR (per-item envelope encryption, size-bucket padding, AAD discipline) stands.

## Context

Tock stores sensitive personal data: tasks, habits, time tracking, and notes. Synchronization requires transmitting this data through untrusted servers or peer-to-peer relays. Users must trust that:

1. The sync server never sees plaintext.
2. Stolen server data remains unreadable.
3. At-rest vault files are protected against disk theft.
4. Recovery is possible without the original password.

We need cryptographic design that meets these requirements without introducing key management complexity that breaks the user experience.

## Decision

Tock uses **hierarchical key derivation with per-item envelope encryption**:

**Key hierarchy:**
- User password → Argon2id (t=3, m=512 MiB) → Master Key (MK)
- MK → HKDF → Master Encryption Key (MEK)
- MEK wraps the Vault Key (VK) via AES-256-GCM
- VK keys SQLCipher at rest
- VK → HKDF → Domain Key (DK) per entity type
- DK + entity ID → HKDF → Item Key (IK)

**Per-event encryption:**
Each sync event payload is encrypted with AES-256-GCM using its unique IK, a random 12-byte nonce, and domain-separated AAD: `"tock|v1|" || entity_kind || "|" || entity_id || "|" || op || "|" || lamport || "|" || device_id`. This binds ciphertext to context and prevents cross-item key reuse.

**Size-bucket padding** rounds plaintext to power-of-two buckets (64, 128, 256, 512, 1024, 2048, 4096, 8192 bytes; larger payloads rounded to 4 KiB multiples) to prevent metadata leakage from ciphertext size.

**Recovery key:** 256 bits of CSPRNG entropy encoded as 24 words using Crockford Base32 (no ambiguous characters, 4-bit checksum). The vault header stores `vk_recover_ct = AES-GCM(RK, nonce, VK)`. Users store the recovery key offline; it can regenerate MEK and VK without the password.

**Authentication:** SRP-6a (RFC 5054) ensures the server never receives the password or enough information to derive MK or VK.

## Consequences

**Positive:**
- Server compromise reveals only metadata (event count, timing, bucketed sizes).
- Per-item keys prevent cross-item cryptanalysis.
- Recovery flow never exposes the original password.
- HKDF domain separation prevents key reuse across contexts.

**Negative:**
- Argon2id (512 MiB, 3 iterations) takes ~2 seconds on mid-range hardware; may frustrate users on slow devices (acceptable trade-off for password hardening).
- Rotating VK requires full vault re-encryption (a `tock vault rotate` operation).

**Neutral:**
- Depends on audited crates: `aes-gcm`, `argon2`, `x25519-dalek`, `hkdf`. Security relies on their correctness and our implementation discipline.
