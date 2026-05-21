# tock-crypto

Cryptographic primitives for tock. Pure computation, no I/O, no `unsafe`.

Built on audited [RustCrypto](https://github.com/RustCrypto) and
[dalek](https://github.com/dalek-cryptography) crates:

| Primitive             | Crate            | Module                              |
|-----------------------|------------------|-------------------------------------|
| AES-256-GCM AEAD      | `aes-gcm`        | [`aead`](src/aead.rs)               |
| Argon2id password KDF | `argon2`         | [`kdf`](src/kdf.rs)                 |
| HKDF-SHA256           | `hkdf` + `sha2`  | [`kdf`](src/kdf.rs)                 |
| X25519 DH             | `x25519-dalek`   | [`keyexchange`](src/keyexchange.rs) |
| Ed25519 signatures    | `ed25519-dalek`  | [`signature`](src/signature.rs)     |

All secret types implement `Zeroize` + `ZeroizeOnDrop`, have redacted
`Debug` impls, and use constant-time equality.

All RNG-touching constructors are fallible (`try_random` / `try_generate`)
because the workspace forbids panicking code.

Licensed under [Apache-2.0](../../LICENSE-APACHE). See
[ADR-002](../../docs/adr/ADR-002-end-to-end-encryption.md) for the
cryptographic design.
