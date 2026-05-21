//! On-disk vault: open / init / lock / status.
//!
//! ## Layout
//!
//! A vault is a single `SQLite` file. The vault header lives in the
//! `vault_meta` table (one row per metadata field, key/blob pairs).
//! Sensitive data — currently event payloads — is encrypted at the
//! application layer with `tock-crypto`. `SQLCipher` integration is a
//! follow-up; the `storage_layout` header field marks this build's
//! format so a future migration can detect and upgrade it.
//!
//! ## Threat model in this build
//!
//! - **In motion / in event log**: every event payload is AEAD-encrypted
//!   under a key derived from the Vault Key.
//! - **At rest (rest of the database)**: NOT encrypted on disk. A
//!   stolen `.db` file leaks schema-version metadata and the encrypted
//!   event payloads (which are confidential by themselves) but does
//!   not expose the Vault Key without the password (it is stored
//!   wrapped under MEK).
//! - When `SQLCipher` integration lands, the whole database page is
//!   keyed by VK and at-rest exposure drops to header metadata only.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, params};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tock_core::Error as CoreError;
use tock_core::vault::header::{
    Argon2HeaderParams, FORMAT_VERSION, MAGIC, MIN_COMPAT_VERSION, STORAGE_LAYOUT_V0,
};
use tock_core::vault::{KeyHierarchy, VaultHeader, VaultKey, generate_vault_key};
use tock_crypto::signature::{SigningKey, VerifyingKey};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::Error;
use crate::migrations;

/// Status of a vault on disk, readable without unwrapping VK.
#[derive(Clone, Debug)]
pub struct VaultStatus {
    /// Vault identifier (`UUIDv7`).
    pub vault_id: Uuid,
    /// Vault format version.
    pub format_version: u16,
    /// Lowest tock version that can open this vault.
    pub min_compatible_version: u16,
    /// Storage layout identifier (see `tock_core::vault::header::STORAGE_LAYOUT_V0`).
    pub storage_layout: String,
    /// When the vault was created.
    pub created_at: OffsetDateTime,
    /// Schema version (from `PRAGMA user_version`).
    pub schema_version: u32,
}

/// Per-device local key material kept inside the vault.
///
/// Ed25519 signing key bytes are encrypted with a small key derived
/// from VK before being stored, so opening the vault requires VK to
/// recover them.
pub struct LocalDevice {
    /// 16-byte device identifier.
    pub device_id: [u8; 16],
    /// Ed25519 signing key for events produced on this device.
    pub signing_key: SigningKey,
}

impl core::fmt::Debug for LocalDevice {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LocalDevice")
            .field("device_id", &hex_short(&self.device_id))
            .field("signing_key", &"<REDACTED>")
            .finish()
    }
}

fn hex_short(b: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(b.len() * 2);
    for c in b.iter().take(8) {
        let _ = write!(&mut s, "{c:02x}");
    }
    if b.len() > 8 {
        s.push('…');
    }
    s
}

/// Argon2 parameters used when initializing fresh vaults in this build.
///
/// Matches `Argon2Params::TOCK_V1` (architecture §5.2):
/// `t = 3, m = 64 MiB, p = 1`.
const VAULT_INIT_ARGON2: Argon2HeaderParams = Argon2HeaderParams {
    t: 3,
    m_kib: 65_536,
    p: 1,
};

/// An unlocked vault — holds an open `SQLite` connection and the
/// decrypted Vault Key in memory.
///
/// `OpenVault` is `Send + !Sync`: only one writer at a time.
pub struct OpenVault {
    conn: Connection,
    vk: VaultKey,
    header: VaultHeader,
    device: LocalDevice,
    path: PathBuf,
}

// rusqlite::Connection is Send+!Sync; VaultKey is Send+Sync; this
// composes to Send+!Sync, which is what we want.
const _: fn() = || {
    const fn assert_send<T: Send>() {}
    assert_send::<OpenVault>();
};

impl core::fmt::Debug for OpenVault {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OpenVault")
            .field("path", &self.path)
            .field("vault_id", &self.header.vault_id)
            .field("vk", &"<REDACTED>")
            .field("device", &self.device)
            .finish_non_exhaustive()
    }
}

impl OpenVault {
    /// Borrow the open `SQLite` connection.
    #[must_use]
    pub const fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Mutable borrow of the connection — needed for transactions.
    pub const fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Borrow the unlocked Vault Key.
    #[must_use]
    pub const fn vault_key(&self) -> &VaultKey {
        &self.vk
    }

    /// Borrow the vault header.
    #[must_use]
    pub const fn header(&self) -> &VaultHeader {
        &self.header
    }

    /// Borrow the local device's signing key + id.
    #[must_use]
    pub const fn local_device(&self) -> &LocalDevice {
        &self.device
    }

    /// Path the vault was opened from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Drop the in-memory Vault Key and close the connection.
    pub fn lock(self) {
        drop(self);
    }
}

/// Open an existing vault file with the given password.
///
/// # Errors
/// - [`Error::NotFound`] if the file does not exist.
/// - [`Error::InvalidVaultOrCredentials`] for wrong password, tampered
///   header, or malformed metadata.
/// - [`Error::Sqlite`] / [`Error::MigrationChecksumMismatch`] on
///   schema-runtime failures.
pub fn open(path: &Path, password: &[u8]) -> Result<OpenVault, Error> {
    if !path.exists() {
        return Err(Error::NotFound);
    }
    let mut conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    migrations::migrate(&mut conn)?;
    let meta = load_meta(&conn)?;
    let header = VaultHeader::from_meta(&meta).map_err(map_core_err)?;
    let mk = KeyHierarchy::derive_master_key(password, &header)
        .map_err(|_| Error::InvalidVaultOrCredentials)?;
    let mek =
        KeyHierarchy::derive_mek(&mk, &header).map_err(|_| Error::InvalidVaultOrCredentials)?;
    let vk =
        KeyHierarchy::unwrap_vk(&mek, &header).map_err(|_| Error::InvalidVaultOrCredentials)?;
    let device = load_local_device(&conn, &vk)?;

    Ok(OpenVault {
        conn,
        vk,
        header,
        device,
        path: path.to_path_buf(),
    })
}

/// Initialize a fresh vault at `path` (must not exist) with `password`.
///
/// Generates the vault id, KDF salts, vault key, a per-device random
/// `device_id`, and an Ed25519 signing key; writes them all into the
/// new `SQLite` file; registers the device.
///
/// # Errors
/// - [`Error::Io`] if `path` already exists.
/// - [`Error::Sqlite`] / [`Error::Crypto`] on the usual failure modes.
pub fn init(path: &Path, password: &[u8]) -> Result<OpenVault, Error> {
    if path.exists() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "vault file already exists",
        )));
    }
    let mut conn = Connection::open(path)?;
    migrations::migrate(&mut conn)?;

    // Build header with random salts, generate VK, wrap it.
    let mut kdf_salt = [0_u8; 16];
    let mut hkdf_salt = [0_u8; 32];
    tock_crypto::random::fill_random(&mut kdf_salt)?;
    tock_crypto::random::fill_random(&mut hkdf_salt)?;
    let header_skel = VaultHeader {
        magic: MAGIC,
        format_version: FORMAT_VERSION,
        min_compatible_version: MIN_COMPAT_VERSION,
        vault_id: Uuid::now_v7(),
        kdf_salt,
        hkdf_salt,
        argon2: VAULT_INIT_ARGON2,
        vk_wrap_nonce: [0; 12],
        vk_wrap_ct: Vec::new(),
        created_at: OffsetDateTime::now_utc(),
        storage_layout: STORAGE_LAYOUT_V0.to_string(),
    };

    let mk = KeyHierarchy::derive_master_key(password, &header_skel).map_err(map_core_err)?;
    let mek = KeyHierarchy::derive_mek(&mk, &header_skel).map_err(map_core_err)?;
    let vk = generate_vault_key().map_err(map_core_err)?;
    let (nonce, ct) = KeyHierarchy::wrap_vk(&mek, &vk, &header_skel).map_err(map_core_err)?;

    let header = VaultHeader {
        vk_wrap_nonce: *nonce.as_bytes(),
        vk_wrap_ct: ct,
        ..header_skel
    };
    save_meta(&conn, &header.to_meta())?;

    // Generate local device + signing key, register both.
    let mut device_id = [0_u8; 16];
    tock_crypto::random::fill_random(&mut device_id)?;
    let signing_key = SigningKey::try_generate().map_err(map_crypto_err)?;
    let verifying = signing_key.verifying_key();
    save_local_device(&conn, &vk, &device_id, &signing_key)?;
    register_device(&conn, &device_id, &verifying, Some("local"))?;

    Ok(OpenVault {
        conn,
        vk,
        header,
        device: LocalDevice {
            device_id,
            signing_key,
        },
        path: path.to_path_buf(),
    })
}

/// Read the vault status (header + schema version) without unwrapping VK.
///
/// # Errors
/// - [`Error::NotFound`] if `path` does not exist.
/// - [`Error::InvalidVaultOrCredentials`] if the metadata is missing or
///   malformed (we don't reveal whether it's a tampered or merely
///   uninitialized vault).
pub fn status(path: &Path) -> Result<VaultStatus, Error> {
    if !path.exists() {
        return Err(Error::NotFound);
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let meta = load_meta(&conn)?;
    let header = VaultHeader::from_meta(&meta).map_err(map_core_err)?;
    let schema_version: u32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap_or(0);
    Ok(VaultStatus {
        vault_id: header.vault_id,
        format_version: header.format_version,
        min_compatible_version: header.min_compatible_version,
        storage_layout: header.storage_layout,
        created_at: header.created_at,
        schema_version,
    })
}

fn load_meta(conn: &Connection) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    let mut stmt = conn.prepare("SELECT key, value FROM vault_meta")?;
    let rows = stmt.query_map([], |r| {
        let k: String = r.get(0)?;
        let v: Vec<u8> = r.get(1)?;
        Ok((k, v))
    })?;
    let mut out = BTreeMap::new();
    for row in rows {
        let (k, v) = row?;
        out.insert(k, v);
    }
    Ok(out)
}

fn save_meta(conn: &Connection, meta: &BTreeMap<&'static str, Vec<u8>>) -> Result<(), Error> {
    let mut stmt =
        conn.prepare("INSERT OR REPLACE INTO vault_meta (key, value) VALUES (?1, ?2)")?;
    for (k, v) in meta {
        stmt.execute(params![k, v])?;
    }
    Ok(())
}

/// Local device key encryption: HKDF a small key from VK and AEAD the
/// signing key bytes. Stored in `vault_meta` under reserved keys.
fn local_device_key(vk: &VaultKey) -> Result<tock_crypto::aead::Key, Error> {
    let derived = tock_crypto::kdf::hkdf_sha256_32(
        vk.as_secret().expose_secret(),
        &[],
        b"tock/v1/local-device-key",
    )?;
    Ok(tock_crypto::aead::Key::from_secret(derived))
}

fn save_local_device(
    conn: &Connection,
    vk: &VaultKey,
    device_id: &[u8; 16],
    signing_key: &SigningKey,
) -> Result<(), Error> {
    let seed = signing_key.to_secret_bytes();
    let key = local_device_key(vk)?;
    let nonce = tock_crypto::aead::Nonce::try_random()?;
    let ct = tock_crypto::aead::seal(&key, &nonce, b"tock-local-device-v1", seed.expose_secret())?;
    let mut stmt =
        conn.prepare("INSERT OR REPLACE INTO vault_meta (key, value) VALUES (?1, ?2)")?;
    stmt.execute(params!["local_device_id", device_id.to_vec()])?;
    stmt.execute(params!["local_device_nonce", nonce.as_bytes().to_vec()])?;
    stmt.execute(params!["local_device_ct", ct])?;
    Ok(())
}

fn load_local_device(conn: &Connection, vk: &VaultKey) -> Result<LocalDevice, Error> {
    let device_id: Vec<u8> = conn.query_row(
        "SELECT value FROM vault_meta WHERE key = 'local_device_id'",
        [],
        |r| r.get(0),
    )?;
    let nonce_bytes: Vec<u8> = conn.query_row(
        "SELECT value FROM vault_meta WHERE key = 'local_device_nonce'",
        [],
        |r| r.get(0),
    )?;
    let ct: Vec<u8> = conn.query_row(
        "SELECT value FROM vault_meta WHERE key = 'local_device_ct'",
        [],
        |r| r.get(0),
    )?;
    let device_id_arr: [u8; 16] = device_id
        .as_slice()
        .try_into()
        .map_err(|_| Error::InvalidVaultOrCredentials)?;
    let nonce_arr: [u8; 12] = nonce_bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::InvalidVaultOrCredentials)?;
    let key = local_device_key(vk)?;
    let nonce = tock_crypto::aead::Nonce::from_bytes(nonce_arr);
    let seed: Zeroizing<Vec<u8>> =
        tock_crypto::aead::open(&key, &nonce, b"tock-local-device-v1", &ct)
            .map_err(|_| Error::InvalidVaultOrCredentials)?;
    let seed_arr: [u8; 32] = seed
        .as_slice()
        .try_into()
        .map_err(|_| Error::InvalidVaultOrCredentials)?;
    let signing_key = SigningKey::from_bytes(&seed_arr);
    Ok(LocalDevice {
        device_id: device_id_arr,
        signing_key,
    })
}

fn register_device(
    conn: &Connection,
    device_id: &[u8; 16],
    verifying: &VerifyingKey,
    label: Option<&str>,
) -> Result<(), Error> {
    let ts = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default();
    conn.execute(
        "INSERT OR REPLACE INTO devices (device_id, verifying_key, label, registered_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![device_id.to_vec(), verifying.to_bytes().to_vec(), label, ts],
    )?;
    Ok(())
}

const fn map_core_err(e: CoreError) -> Error {
    match e {
        CoreError::InvalidVaultOrCredentials => Error::InvalidVaultOrCredentials,
        CoreError::UnsupportedVaultVersion { .. } => Error::UnsupportedVaultVersion,
        other => Error::Core(other),
    }
}

const fn map_crypto_err(e: tock_crypto::Error) -> Error {
    Error::Crypto(e)
}
