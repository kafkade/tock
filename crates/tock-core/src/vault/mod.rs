//! Vault format and key hierarchy.
//!
//! See [ADR-002] and architecture §5 for the cryptographic design.
//!
//! [ADR-002]: https://github.com/kafkade/tock/blob/main/docs/adr/ADR-002-end-to-end-encryption.md
//!
//! ## Composition
//!
//! - [`header::VaultHeader`] is the on-disk metadata: magic, format
//!   version, KDF salts and parameters, the AES-GCM-wrapped Vault Key
//!   ciphertext, and an opaque `storage_layout` tag.
//! - [`keys::KeyHierarchy`] performs all key-derivation operations:
//!   `password → MK` (Argon2id), `MK → MEK` (HKDF), `MEK → VK` (wrap/
//!   unwrap with AES-256-GCM), and `VK → DK_kind → IK` (HKDF) for
//!   per-entity item keys.
//!
//! The header serializes to a `BTreeMap<&str, Vec<u8>>` because this
//! crate is I/O free; the storage layer (`tock-storage`) is responsible
//! for actually persisting that map (in a `SQLite` table, a binary
//! header, etc.).

pub mod header;
pub mod keys;

pub use header::{Argon2HeaderParams, FORMAT_VERSION, MAGIC, MIN_COMPAT_VERSION, VaultHeader};
pub use keys::{KeyHierarchy, VaultKey, generate_vault_key};
