# ADR-010: SRP-6a authentication

**Status:** Accepted (amended by [ADR-011](ADR-011-account-based-self-host-two-secret-auth.md))  
**Date:** 2026-05-20

> **Amended by [ADR-011](ADR-011-account-based-self-host-two-secret-auth.md):**
> - The SRP private exponent `x` (and thus verifier `v = g^x`) now derives from the two-secret
>   **Unlock Root Key (URK)** — `x = HKDF(URK, salt_srp, "Tock/v1/srp-x")` — instead of
>   `x = H(salt_srp || H(username:password))`. A stolen verifier is therefore offline-crackable
>   only by an attacker who also holds the 128-bit Secret Key.
> - SRP-based accounts and sessions are now available in **self-hosted mode** (multi-user:
>   admin + N users), not hosted mode only.
> - The "out-of-band recovery (24-word recovery key)" note below is **superseded** by the
>   Emergency Kit (see ADR-011).
>
> The SRP-6a protocol mechanics (registration, mutual-auth login, session key `K`) are unchanged.

## Context

Tock's hosted sync service must authenticate users without ever receiving their password or enough information to derive encryption keys. Traditional authentication sends passwords (or hashes) to the server, creating these risks:

1. **Server compromise:** Plaintext passwords or weak hashes (MD5, SHA-1) are immediately exploitable. Even bcrypt/scrypt hashes can be brute-forced offline.
2. **Insider threat:** Server operators can log passwords during login.
3. **Key derivation:** If the server receives the password or a password-derived hash, it can derive the Master Key (MK) and decrypt the vault.

We need **zero-knowledge authentication:** the server must verify the user knows the password without the server ever learning it.

## Decision

Tock uses **SRP-6a (Secure Remote Password, RFC 5054)** over a 4096-bit safe-prime group with SHA-256.

**Registration flow:**
1. Client derives `x = H(salt_srp || H(username || ":" || password))` (salt is random, independent of the KDF salt for MK).
2. Client computes verifier `v = g^x mod N` and sends `(username, salt_srp, v)` to the server.
3. Server stores `(username, salt_srp, v)`. It never sees `x` or the password.

**Login flow (mutual authentication):**
1. Client picks random `a`, computes `A = g^a mod N`, sends `(username, A)`.
2. Server looks up `(salt_srp, v)`, picks random `b`, computes `B = (k*v + g^b) mod N`, sends `(salt_srp, B)`.
3. Both sides derive shared secret `S` and session key `K = H(S)`:
   - Client: `S_c = (B - k*g^x)^(a + u*x) mod N`
   - Server: `S_s = (A * v^u)^b mod N`
   - Where `u = H(A, B)` and `k = H(N, g)`.
4. Client proves knowledge by sending `M1 = H(H(N) XOR H(g) || H(username) || salt_srp || A || B || K)`.
5. Server verifies `M1`, sends `M2 = H(A || M1 || K)` for mutual authentication.
6. Session key `K` is used via HKDF to derive:
   - **Bearer token** (HMAC-signed, short-lived, sent with sync requests).
   - **Channel-binding tag** (included in event AAD for defense-in-depth against TLS-stripping).

**Key property:** The server stores only `(username, salt_srp, v)`. A compromised database reveals no plaintext passwords, no MK, no VK. Offline brute-force attacks against `v` are computationally equivalent to brute-forcing discrete logarithm in a 4096-bit group (infeasible).

## Consequences

**Positive:**
- Server never receives password or password-derived key material (zero-knowledge proof).
- Database compromise does not leak passwords or encryption keys.
- Mutual authentication (server proves possession of `v`, client proves possession of `x`).
- No password transmission (not even hashed) reduces insider threat and MITM risk.

**Negative:**
- SRP-6a is complex (harder to implement correctly than bcrypt + HTTPS).
- 4096-bit modular exponentiation is slower than simple hashing (~50ms on modern CPUs; acceptable for login).
- Less widely deployed than password-over-HTTPS + bcrypt (but SRP is an RFC standard with vetted implementations).

**Neutral:**
- Relies on the `srp` crate's correctness (we use a maintained, audited implementation).
- Password reset requires out-of-band recovery (24-word recovery key), as the server cannot reset a password it never knew.
