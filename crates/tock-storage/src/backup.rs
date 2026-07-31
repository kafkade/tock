//! Encrypted vault backup & restore (ADR-018).
//!
//! ## Why an outer envelope
//!
//! Per [ADR-014](../../../docs/adr/ADR-014-at-rest-encryption-app-layer-aead.md)
//! the 1.0 build stores a **plain `SQLite`** database whose *materialized
//! domain tables are plaintext at rest* (`tasks.title`/`notes`, every
//! habit text field, `checklist_items.title`). Copying that file — or
//! exporting materialized state — produces a **plaintext** archive.
//! Backup therefore wraps a transactionally-consistent snapshot of the
//! whole database (`VACUUM INTO`) in an **outer AES-256-GCM envelope**
//! under a domain-separated backup key, with an **authenticated manifest**
//! bound as the AEAD AAD.
//!
//! ## Key derivation
//!
//! ```text
//! salt   ← 32 random bytes (fresh per backup)
//! BK     ← HKDF-SHA256(ikm = VK, salt = salt, info = "Tock/v1/backup")
//! nonce  ← 12 random bytes (fresh per backup)
//! body   ← AES-256-GCM(key = BK, nonce = nonce, aad = manifest, plaintext = snapshot)
//! ```
//!
//! The per-backup `salt` makes `BK` unique to each backup even for the
//! same VK, and `"Tock/v1/backup"` domain-separates it from every other
//! derivation. Restore needs **only** the account password + Secret Key:
//! the manifest embeds the (non-secret, MEK-wrapped) vault header, so VK
//! can be unwrapped without a surviving vault file.
//!
//! ## On-disk layout (self-describing)
//!
//! ```text
//! [ MAGIC 8 ][ format_version u16 LE ]
//! [ manifest_len u32 LE ][ manifest bytes (JSON) ]
//! [ AES-256-GCM ciphertext + tag  (to EOF) ]
//! ```
//!
//! The manifest bytes read from the file are used **verbatim** as the AEAD
//! AAD, so any edit to the manifest (or the framed magic/version that the
//! manifest mirrors) invalidates the tag. The `event_high_water_mark` +
//! `snapshot_hash`, authenticated as a unit, defeat truncation and
//! rollback — attacks that per-item event AEAD alone cannot detect.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tock_core::vault::KeyHierarchy;
use tock_core::vault::header::VaultHeader;
use tock_crypto::SecretKey;
use tock_crypto::aead::{self, Key as AeadKey, Nonce as AeadNonce};
use tock_crypto::kdf::hkdf_sha256_32;
use uuid::Uuid;

use crate::Error;
use crate::vault::{self, OpenVault};

/// Eight-byte magic identifying a tock backup archive.
const MAGIC: [u8; 8] = *b"TOCKBKUP";

/// On-disk backup envelope format version.
const BACKUP_FORMAT_VERSION: u16 = 1;

/// Manifest `format_tag`; mirrored into the header framing.
const FORMAT_TAG: &str = "tock-backup";

/// HKDF `info` label domain-separating the backup key from every other
/// derivation (sync snapshot, item keys, local-device key, …).
const BK_INFO: &[u8] = b"Tock/v1/backup";

/// Which restore semantics to apply (ADR-018 §3). The mode is an
/// **explicit** caller choice — never inferred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreMode {
    /// The original device is gone: restore in place and keep its
    /// `device_id`, signing key, Lamport clock, sync cursor, and server
    /// binding (they all ride along inside the snapshot). Resume sync as
    /// the same single writer for that identity.
    DisasterRecovery,
    /// The original device may still exist: mint a fresh `device_id` +
    /// signing key, reset the sync cursor, and clear the server binding so
    /// the clone re-registers as a new device and reconciles from an empty
    /// cursor before pushing.
    Clone,
}

/// Per-device highest lamport plus the global event count, at snapshot
/// time. Authenticated in the manifest to defeat truncation/rollback.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct HighWater {
    /// Total number of rows in the event log.
    event_count: u64,
    /// `(device_id[16], max_lamport)` pairs, sorted by device id.
    per_device: Vec<(Vec<u8>, u64)>,
}

impl HighWater {
    /// Read the current high-water mark from an event log connection.
    fn read(conn: &Connection) -> Result<Self, Error> {
        let event_count: u64 = conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get::<_, i64>(0))
            .optional()?
            .unwrap_or(0)
            .max(0)
            .unsigned_abs();
        let mut stmt =
            conn.prepare("SELECT device_id, MAX(lamport) FROM events GROUP BY device_id")?;
        let mut per_device: Vec<(Vec<u8>, u64)> = stmt
            .query_map([], |r| {
                let dev: Vec<u8> = r.get(0)?;
                let max: i64 = r.get(1)?;
                Ok((dev, max.max(0).unsigned_abs()))
            })?
            .collect::<Result<_, _>>()?;
        per_device.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(Self {
            event_count,
            per_device,
        })
    }

    /// Whether restoring a backup at `self` over a target currently at
    /// `target` would drop committed history — i.e. the target is strictly
    /// ahead (more total events, or a higher lamport for some device).
    fn is_behind(&self, target: &Self) -> bool {
        if target.event_count > self.event_count {
            return true;
        }
        let mine: BTreeMap<&Vec<u8>, u64> = self.per_device.iter().map(|(d, l)| (d, *l)).collect();
        target
            .per_device
            .iter()
            .any(|(dev, tl)| *tl > mine.get(dev).copied().unwrap_or(0))
    }
}

/// Authenticated, cleartext manifest bound into the AEAD as AAD. All
/// byte fields are length-checked on parse.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct BackupManifest {
    /// Archive family tag (`"tock-backup"`).
    format_tag: String,
    /// Manifest/envelope format version.
    format_version: u16,
    /// Client-minted crypto account id **A** (16 bytes).
    account_id: Vec<u8>,
    /// Vault id **V** (16 bytes).
    vault_id: Vec<u8>,
    /// Two-secret KDF generation recorded at snapshot time.
    kdf_version: u16,
    /// Portable, MEK-wrapped vault header (`VaultHeader::to_bytes`), used
    /// to unwrap VK at restore from password + Secret Key alone.
    vault_header: Vec<u8>,
    /// Event high-water mark for truncation/rollback detection.
    high_water: HighWater,
    /// SHA-256 of the plaintext snapshot (32 bytes).
    snapshot_hash: Vec<u8>,
    /// BK salt (32 bytes).
    salt: Vec<u8>,
    /// Body AEAD nonce (12 bytes).
    nonce: Vec<u8>,
    /// When the backup was created (RFC 3339).
    created_at: String,
}

/// Provenance summary returned by [`create`].
#[derive(Clone, Debug)]
pub struct BackupInfo {
    /// Vault id the backup was taken from.
    pub vault_id: Uuid,
    /// Number of events captured.
    pub event_count: u64,
    /// Size of the written archive, in bytes.
    pub archive_bytes: u64,
}

/// Outcome of a [`restore`].
#[derive(Clone, Debug)]
pub struct RestoreOutcome {
    /// Mode the restore ran in.
    pub mode: RestoreMode,
    /// Vault id that was restored.
    pub vault_id: Uuid,
    /// Fresh `device_id` minted for a clone restore (`None` for disaster
    /// recovery, which keeps the original identity).
    pub new_device_id: Option<[u8; 16]>,
}

// ── Create ───────────────────────────────────────────────────────────

/// Write an outer-encrypted full-snapshot backup of `vault` to `out_path`.
///
/// Produces a transactionally-consistent `SQLite` snapshot with
/// `VACUUM INTO`, hashes it, seals it with AES-256-GCM under
/// `BK = HKDF(VK, salt, "Tock/v1/backup")`, and binds an authenticated
/// manifest as the AEAD AAD. The plaintext snapshot is written to a
/// temporary file and removed promptly.
///
/// # Errors
/// - [`Error::Io`] on filesystem failures or if `out_path` cannot be written.
/// - [`Error::Sqlite`] if the snapshot cannot be produced.
/// - [`Error::Crypto`] on RNG / AEAD failure.
pub fn create(vault: &OpenVault, out_path: &Path) -> Result<BackupInfo, Error> {
    let _span = tracing::info_span!("backup::create", out = %out_path.display()).entered();

    // 1. Consistent plaintext snapshot via VACUUM INTO (removed on drop).
    let snap = TempFile::adjacent_to(out_path, "snapshot");
    vacuum_into(vault.connection(), &snap.path)?;
    let snapshot = std::fs::read(&snap.path)?;

    // 2. Integrity hash + 3. high-water mark.
    let snapshot_hash = sha256(&snapshot);
    let high_water = HighWater::read(vault.connection())?;
    let event_count = high_water.event_count;

    // 4. Fresh salt + nonce.
    let mut salt = [0_u8; 32];
    let mut nonce_bytes = [0_u8; 12];
    tock_crypto::random::fill_random(&mut salt)?;
    tock_crypto::random::fill_random(&mut nonce_bytes)?;

    // 5. Derive BK from VK.
    let bk = derive_backup_key(vault.vault_key().as_secret().expose_secret(), &salt)?;
    let nonce = AeadNonce::from_bytes(nonce_bytes);

    // 6. Manifest.
    let header = vault.header();
    let manifest = BackupManifest {
        format_tag: FORMAT_TAG.to_string(),
        format_version: BACKUP_FORMAT_VERSION,
        account_id: header.account_id.as_bytes().to_vec(),
        vault_id: header.vault_id.as_bytes().to_vec(),
        kdf_version: header.kdf_version,
        vault_header: header.to_bytes(),
        high_water,
        snapshot_hash: snapshot_hash.to_vec(),
        salt: salt.to_vec(),
        nonce: nonce_bytes.to_vec(),
        created_at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default(),
    };
    let manifest_bytes = serde_json::to_vec(&manifest).map_err(map_serde_err)?;

    // 7. Seal the snapshot with the manifest as AAD.
    let body = aead::seal(&bk, &nonce, &manifest_bytes, &snapshot)?;

    // 8. Frame + atomic write.
    let framed = frame(&manifest_bytes, &body);
    atomic_write(out_path, &framed)?;

    tracing::info!(
        vault_id = %header.vault_id,
        events = event_count,
        bytes = framed.len(),
        "backup archive written"
    );
    Ok(BackupInfo {
        vault_id: header.vault_id,
        event_count,
        archive_bytes: framed.len() as u64,
    })
}

// ── Restore ──────────────────────────────────────────────────────────

/// Restore a backup archive to `target_path`.
///
/// Unwraps VK from the manifest-embedded header using `password` +
/// `secret_key`, re-derives BK, authenticates the manifest (AAD) and body,
/// verifies the snapshot hash, and swaps the database in place. Then
/// applies the `mode`-specific identity/cursor handling (ADR-018 §3).
///
/// `expected_account_id` is the account id embedded in the caller's Secret
/// Key; a mismatch with the archive's account id is a cross-account
/// restore and is refused. When `target_path` already exists it is checked
/// for a cross-vault (`vault_id`) or cross-account (`account_id`) mismatch,
/// and for a high-water **rollback** (refused unless `force`).
///
/// # Errors
/// - [`Error::InvalidVaultOrCredentials`] for a bad magic/version, a wrong
///   password/Secret Key, a cross-account/cross-vault archive, a failed
///   manifest/body authentication, a snapshot-hash mismatch, or a
///   (non-forced) rollback.
/// - [`Error::Io`] / [`Error::Sqlite`] on filesystem/schema failures.
pub fn restore(
    backup_path: &Path,
    target_path: &Path,
    password: &[u8],
    secret_key: &SecretKey,
    expected_account_id: [u8; 16],
    mode: RestoreMode,
    force: bool,
) -> Result<RestoreOutcome, Error> {
    let _span = tracing::info_span!("backup::restore", src = %backup_path.display()).entered();

    let raw = std::fs::read(backup_path)?;
    let (manifest_bytes, body) = unframe(&raw)?;
    let manifest: BackupManifest =
        serde_json::from_slice(manifest_bytes).map_err(|_| Error::InvalidVaultOrCredentials)?;

    // Structural checks on the (still-unauthenticated) manifest.
    if manifest.format_tag != FORMAT_TAG || manifest.format_version != BACKUP_FORMAT_VERSION {
        return Err(Error::InvalidVaultOrCredentials);
    }
    let manifest_account = as_array::<16>(&manifest.account_id)?;
    let manifest_vault = as_array::<16>(&manifest.vault_id)?;
    let salt = as_array::<32>(&manifest.salt)?;
    let nonce = AeadNonce::from_bytes(as_array::<12>(&manifest.nonce)?);
    let claimed_hash = as_array::<32>(&manifest.snapshot_hash)?;

    // Cross-account: the Secret Key's account must match the archive's.
    if manifest_account != expected_account_id {
        return Err(Error::InvalidVaultOrCredentials);
    }

    // Unwrap VK from the embedded header (password + Secret Key). A wrong
    // password or Secret Key fails here, identically.
    let header = VaultHeader::from_bytes(&manifest.vault_header)
        .map_err(|_| Error::InvalidVaultOrCredentials)?;
    // Defense-in-depth: the embedded header must agree with the manifest's
    // top-level authenticated A/V (which remain the authoritative binding the
    // cross-vault/cross-account guards read).
    if *header.account_id.as_bytes() != manifest_account
        || *header.vault_id.as_bytes() != manifest_vault
    {
        return Err(Error::InvalidVaultOrCredentials);
    }
    let urk = KeyHierarchy::derive_unlock_root_key(password, secret_key, &header)
        .map_err(|_| Error::InvalidVaultOrCredentials)?;
    let mek =
        KeyHierarchy::derive_mek(&urk, &header).map_err(|_| Error::InvalidVaultOrCredentials)?;
    let vk =
        KeyHierarchy::unwrap_vk(&mek, &header).map_err(|_| Error::InvalidVaultOrCredentials)?;

    // Re-derive BK and open the body (manifest is the AAD → any tamper or
    // truncation fails here).
    let bk = derive_backup_key(vk.as_secret().expose_secret(), &salt)?;
    let snapshot = aead::open(&bk, &nonce, manifest_bytes, body)
        .map_err(|_| Error::InvalidVaultOrCredentials)?;

    // Integrity: the plaintext must hash to the authenticated value.
    if sha256(&snapshot) != claimed_hash {
        return Err(Error::EventLogIntegrity);
    }

    // Guard an in-place restore over an existing vault.
    if let Some(existing) = read_target_identity(target_path)? {
        if *existing.vault_id.as_bytes() != manifest_vault
            || *existing.account_id.as_bytes() != manifest_account
        {
            // Cross-vault or cross-account misdirected restore.
            return Err(Error::InvalidVaultOrCredentials);
        }
        if !force && manifest.high_water.is_behind(&existing.high_water) {
            return Err(Error::InvalidState(
                "backup is older than the current vault (rollback); pass --force to override",
            ));
        }
    }

    // Swap the database in place (atomic rename over the target).
    atomic_write(target_path, &snapshot)?;

    let vault_id = Uuid::from_bytes(manifest_vault);
    match mode {
        RestoreMode::DisasterRecovery => {
            // Identity, clock, cursor, and binding all ride along in the
            // snapshot. Open once to confirm it decrypts, then done.
            let v = vault::open(target_path, password, secret_key)?;
            v.lock();
            tracing::info!(%vault_id, "disaster-recovery restore complete");
            Ok(RestoreOutcome {
                mode,
                vault_id,
                new_device_id: None,
            })
        }
        RestoreMode::Clone => {
            let mut v = vault::open(target_path, password, secret_key)?;
            let new_device_id = v.remint_identity()?;
            crate::sync::set_pull_cursor(&v, 0)?;
            crate::sync::clear_binding(&v)?;
            // Enforce reconcile-before-push (ADR-018 §3B step 3): the next
            // sync MUST pull + ingest remote history before pushing any local
            // diffs synthesized from the restored snapshot.
            crate::sync::set_pending_reconcile(&v)?;
            v.lock();
            tracing::info!(%vault_id, "clone restore complete (fresh identity, local-only)");
            Ok(RestoreOutcome {
                mode,
                vault_id,
                new_device_id: Some(new_device_id),
            })
        }
    }
}

// ── Framing ──────────────────────────────────────────────────────────

fn frame(manifest_bytes: &[u8], body: &[u8]) -> Vec<u8> {
    let manifest_len = u32::try_from(manifest_bytes.len()).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(8 + 2 + 4 + manifest_bytes.len() + body.len());
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&BACKUP_FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&manifest_len.to_le_bytes());
    out.extend_from_slice(manifest_bytes);
    out.extend_from_slice(body);
    out
}

/// Split a framed archive into `(manifest_bytes, body)`, validating the
/// magic, format version, and length prefix.
fn unframe(raw: &[u8]) -> Result<(&[u8], &[u8]), Error> {
    let mut pos = 0_usize;
    let take = |pos: &mut usize, n: usize| -> Result<&[u8], Error> {
        let end = pos.checked_add(n).ok_or(Error::InvalidVaultOrCredentials)?;
        let slice = raw.get(*pos..end).ok_or(Error::InvalidVaultOrCredentials)?;
        *pos = end;
        Ok(slice)
    };
    if take(&mut pos, 8)? != MAGIC {
        return Err(Error::InvalidVaultOrCredentials);
    }
    let version = u16::from_le_bytes(
        take(&mut pos, 2)?
            .try_into()
            .map_err(|_| Error::InvalidVaultOrCredentials)?,
    );
    if version != BACKUP_FORMAT_VERSION {
        return Err(Error::InvalidVaultOrCredentials);
    }
    let manifest_len = u32::from_le_bytes(
        take(&mut pos, 4)?
            .try_into()
            .map_err(|_| Error::InvalidVaultOrCredentials)?,
    ) as usize;
    let manifest = take(&mut pos, manifest_len)?;
    let body = raw.get(pos..).ok_or(Error::InvalidVaultOrCredentials)?;
    Ok((manifest, body))
}

// ── Helpers ──────────────────────────────────────────────────────────

fn derive_backup_key(vk_bytes: &[u8], salt: &[u8; 32]) -> Result<AeadKey, Error> {
    let bk = hkdf_sha256_32(vk_bytes, salt, BK_INFO)?;
    Ok(AeadKey::from_secret(bk))
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn as_array<const N: usize>(bytes: &[u8]) -> Result<[u8; N], Error> {
    <[u8; N]>::try_from(bytes).map_err(|_| Error::InvalidVaultOrCredentials)
}

/// Run `VACUUM INTO` to produce a consistent snapshot at `dest` (which
/// must not exist).
fn vacuum_into(conn: &Connection, dest: &Path) -> Result<(), Error> {
    let dest_str = dest
        .to_str()
        .ok_or_else(|| Error::Io(std::io::Error::other("snapshot path is not valid UTF-8")))?;
    conn.execute("VACUUM INTO ?1", params![dest_str])?;
    Ok(())
}

/// Identity + high-water read from an existing target vault, if present.
struct TargetIdentity {
    vault_id: Uuid,
    account_id: Uuid,
    high_water: HighWater,
}

fn read_target_identity(path: &Path) -> Result<Option<TargetIdentity>, Error> {
    if !path.exists() {
        return Ok(None);
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = conn.prepare("SELECT key, value FROM vault_meta")?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
    })?;
    let mut meta = BTreeMap::new();
    for row in rows {
        let (k, v) = row?;
        meta.insert(k, v);
    }
    let header = VaultHeader::from_meta(&meta).map_err(|_| Error::InvalidVaultOrCredentials)?;
    let high_water = HighWater::read(&conn)?;
    Ok(Some(TargetIdentity {
        vault_id: header.vault_id,
        account_id: header.account_id,
        high_water,
    }))
}

/// Write `bytes` to `path` atomically: to a sibling temp file, fsync, then
/// rename over `path`.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let tmp = TempFile::adjacent_to(path, "write");
    {
        let mut f = std::fs::File::create(&tmp.path)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    // Consume the guard: on success we rename (moving the file), so the
    // guard must not also try to remove it afterwards.
    let tmp_path = tmp.into_path();
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

/// A temporary file adjacent to a destination, removed on drop unless
/// explicitly kept.
struct TempFile {
    path: PathBuf,
    keep: bool,
}

impl TempFile {
    /// Pick a fresh, non-existent temp path in the same directory as
    /// `neighbor` (same filesystem → rename stays atomic).
    fn adjacent_to(neighbor: &Path, tag: &str) -> Self {
        let dir = neighbor.parent().filter(|p| !p.as_os_str().is_empty());
        let name = format!(".tock-backup-{tag}-{}.tmp", Uuid::now_v7().as_simple());
        let path = dir.map_or_else(|| PathBuf::from(&name), |d| d.join(&name));
        Self { path, keep: false }
    }

    /// Keep the file and return its path (caller now owns cleanup).
    fn into_path(mut self) -> PathBuf {
        self.keep = true;
        self.path.clone()
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn map_serde_err(_e: serde_json::Error) -> Error {
    Error::Io(std::io::Error::other("failed to serialize backup manifest"))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_const_for_fn
)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tock_core::domain::task::NewTask;
    use tock_core::domain::urgency::UrgencyConfig;
    use tock_core::event::{DeviceId, EntityKind, EventOp, VectorClock};
    use tock_crypto::SecretKey;

    /// Init a vault and give it one task + one raw event, returning
    /// `(path, secret_key, account_id, task title)`.
    fn seed_vault(path: &Path) -> (SecretKey, [u8; 16]) {
        let (v, sk) = vault::init(path, b"pw").expect("init");
        let account_id = *v.header().account_id.as_bytes();
        // A domain-table row (plaintext at rest) + an event-log row.
        let new_task = NewTask {
            title: "secret backup title".to_string(),
            notes: Some("private notes".to_string()),
            ..NewTask::default()
        };
        crate::repo::task_repo::insert(v.connection(), &new_task, &UrgencyConfig::default())
            .expect("create task");
        let device_id = v.local_device().device_id;
        crate::EventLog::new(&v)
            .append(
                EntityKind::Task,
                Uuid::now_v7(),
                EventOp::Create,
                b"event-payload",
                VectorClock::singleton(DeviceId::from_bytes(device_id), 1),
                None,
            )
            .expect("append event");
        v.lock();
        (sk, account_id)
    }

    fn task_titles(path: &Path, sk: &SecretKey) -> Vec<String> {
        let v = vault::open(path, b"pw", sk).expect("open");
        let tasks = crate::repo::task_repo::list(v.connection(), false).expect("list");
        let titles = tasks.into_iter().map(|t| t.title).collect();
        v.lock();
        titles
    }

    #[test]
    fn roundtrip_disaster_recovery_restores_identical_data() {
        let dir = tempdir().expect("dir");
        let vault_path = dir.path().join("v.tockvault");
        let (sk, account_id) = seed_vault(&vault_path);
        let before = task_titles(&vault_path, &sk);

        let backup_path = dir.path().join("out.tockbak");
        {
            let v = vault::open(&vault_path, b"pw", &sk).expect("open");
            create(&v, &backup_path).expect("create");
            v.lock();
        }

        // Simulate disaster: the vault file is gone.
        std::fs::remove_file(&vault_path).expect("rm");
        assert!(!vault_path.exists());

        let outcome = restore(
            &backup_path,
            &vault_path,
            b"pw",
            &sk,
            account_id,
            RestoreMode::DisasterRecovery,
            false,
        )
        .expect("restore");
        assert_eq!(outcome.mode, RestoreMode::DisasterRecovery);
        assert!(outcome.new_device_id.is_none());

        let after = task_titles(&vault_path, &sk);
        assert_eq!(before, after);
        assert_eq!(after, vec!["secret backup title".to_string()]);

        // Disaster recovery keeps the original identity and does NOT arm the
        // clone-only reconcile-before-push flag.
        let v = vault::open(&vault_path, b"pw", &sk).expect("reopen");
        assert!(
            !crate::sync::pending_reconcile(&v).expect("pending_reconcile"),
            "disaster recovery must not arm reconcile-before-push"
        );
        v.lock();
    }

    #[test]
    fn restore_needs_only_password_and_secret_key_no_prior_vault() {
        let dir = tempdir().expect("dir");
        let vault_path = dir.path().join("v.tockvault");
        let (sk, account_id) = seed_vault(&vault_path);

        let backup_path = dir.path().join("out.tockbak");
        {
            let v = vault::open(&vault_path, b"pw", &sk).expect("open");
            create(&v, &backup_path).expect("create");
            v.lock();
        }
        std::fs::remove_file(&vault_path).expect("rm");

        // Wrong password → rejected.
        let bad_pw = restore(
            &backup_path,
            &vault_path,
            b"WRONG",
            &sk,
            account_id,
            RestoreMode::DisasterRecovery,
            false,
        );
        assert!(matches!(bad_pw, Err(Error::InvalidVaultOrCredentials)));
        assert!(!vault_path.exists(), "failed restore must not write target");

        // Wrong Secret Key (same claimed account, wrong key material) →
        // VK unwrap fails.
        let other_sk = SecretKey::from_bytes([9; 16]);
        let bad_sk = restore(
            &backup_path,
            &vault_path,
            b"pw",
            &other_sk,
            account_id,
            RestoreMode::DisasterRecovery,
            false,
        );
        assert!(matches!(bad_sk, Err(Error::InvalidVaultOrCredentials)));

        // Correct secrets → succeeds.
        restore(
            &backup_path,
            &vault_path,
            b"pw",
            &sk,
            account_id,
            RestoreMode::DisasterRecovery,
            false,
        )
        .expect("restore");
        assert_eq!(task_titles(&vault_path, &sk).len(), 1);
    }

    #[test]
    fn tampered_manifest_byte_is_rejected() {
        let dir = tempdir().expect("dir");
        let vault_path = dir.path().join("v.tockvault");
        let (sk, account_id) = seed_vault(&vault_path);
        let backup_path = dir.path().join("out.tockbak");
        {
            let v = vault::open(&vault_path, b"pw", &sk).expect("open");
            create(&v, &backup_path).expect("create");
            v.lock();
        }

        let mut raw = std::fs::read(&backup_path).expect("read");
        // Flip a byte inside the manifest region (after the 14-byte frame
        // header) — invalidates the AEAD AAD.
        raw[20] ^= 0x01;
        let tampered = dir.path().join("tampered.tockbak");
        std::fs::write(&tampered, &raw).expect("write");

        std::fs::remove_file(&vault_path).expect("rm");
        let res = restore(
            &tampered,
            &vault_path,
            b"pw",
            &sk,
            account_id,
            RestoreMode::DisasterRecovery,
            false,
        );
        assert!(res.is_err());
        assert!(!vault_path.exists());
    }

    #[test]
    fn truncated_body_is_rejected() {
        let dir = tempdir().expect("dir");
        let vault_path = dir.path().join("v.tockvault");
        let (sk, account_id) = seed_vault(&vault_path);
        let backup_path = dir.path().join("out.tockbak");
        {
            let v = vault::open(&vault_path, b"pw", &sk).expect("open");
            create(&v, &backup_path).expect("create");
            v.lock();
        }

        let mut raw = std::fs::read(&backup_path).expect("read");
        raw.truncate(raw.len() - 64); // drop the tail of the ciphertext
        let truncated = dir.path().join("trunc.tockbak");
        std::fs::write(&truncated, &raw).expect("write");

        std::fs::remove_file(&vault_path).expect("rm");
        let res = restore(
            &truncated,
            &vault_path,
            b"pw",
            &sk,
            account_id,
            RestoreMode::DisasterRecovery,
            false,
        );
        assert!(res.is_err());
    }

    #[test]
    fn bad_magic_and_version_are_rejected() {
        let dir = tempdir().expect("dir");
        let good = dir.path().join("good.tockbak");
        // Bad magic.
        std::fs::write(&good, b"NOTATOCKxxxxxxxxxx").expect("write");
        assert!(unframe(&std::fs::read(&good).unwrap()).is_err());
        // Good magic, wrong version.
        let mut raw = MAGIC.to_vec();
        raw.extend_from_slice(&99_u16.to_le_bytes());
        raw.extend_from_slice(&0_u32.to_le_bytes());
        assert!(unframe(&raw).is_err());
    }

    #[test]
    fn rollback_over_newer_vault_is_rejected_unless_forced() {
        let dir = tempdir().expect("dir");
        let vault_path = dir.path().join("v.tockvault");
        let (sk, account_id) = seed_vault(&vault_path);

        // Backup A (early state).
        let backup_a = dir.path().join("a.tockbak");
        {
            let v = vault::open(&vault_path, b"pw", &sk).expect("open");
            create(&v, &backup_a).expect("create A");
            v.lock();
        }

        // Advance the live vault: more events → higher high-water.
        {
            let v = vault::open(&vault_path, b"pw", &sk).expect("open");
            let device_id = v.local_device().device_id;
            for i in 0..3 {
                crate::EventLog::new(&v)
                    .append(
                        EntityKind::Task,
                        Uuid::now_v7(),
                        EventOp::Create,
                        b"more",
                        VectorClock::singleton(DeviceId::from_bytes(device_id), 2 + i),
                        None,
                    )
                    .expect("append");
            }
            v.lock();
        }

        // Restoring the older backup A in place is a rollback → rejected.
        let res = restore(
            &backup_a,
            &vault_path,
            b"pw",
            &sk,
            account_id,
            RestoreMode::DisasterRecovery,
            false,
        );
        assert!(matches!(res, Err(Error::InvalidState(_))));

        // With --force it is allowed.
        restore(
            &backup_a,
            &vault_path,
            b"pw",
            &sk,
            account_id,
            RestoreMode::DisasterRecovery,
            true,
        )
        .expect("forced rollback");
    }

    #[test]
    fn cross_vault_restore_is_rejected() {
        let dir = tempdir().expect("dir");
        let path_a = dir.path().join("a.tockvault");
        let (sk_a, acct_a) = seed_vault(&path_a);
        let backup_a = dir.path().join("a.tockbak");
        {
            let v = vault::open(&path_a, b"pw", &sk_a).expect("open");
            create(&v, &backup_a).expect("create");
            v.lock();
        }

        // A different vault B (different V, different account/secret key).
        let path_b = dir.path().join("b.tockvault");
        let (_sk_b, _acct_b) = seed_vault(&path_b);

        // Restoring A's backup over B is a misdirected (cross-vault +
        // cross-account) restore → rejected, B untouched.
        let res = restore(
            &backup_a,
            &path_b,
            b"pw",
            &sk_a,
            acct_a,
            RestoreMode::DisasterRecovery,
            false,
        );
        assert!(matches!(res, Err(Error::InvalidVaultOrCredentials)));
    }

    #[test]
    fn clone_restore_mints_fresh_identity_and_resets_sync() {
        let dir = tempdir().expect("dir");
        let vault_path = dir.path().join("v.tockvault");
        let (sk, account_id) = seed_vault(&vault_path);

        // Bind the vault to a server so we can prove clone clears it.
        let original_device_id = {
            let v = vault::open(&vault_path, b"pw", &sk).expect("open");
            crate::sync::set_server_url(&v, "https://example.test").expect("url");
            crate::sync::set_binding_state(&v, crate::sync::BINDING_SERVER_BACKED).expect("bind");
            crate::sync::set_pull_cursor(&v, 42).expect("cursor");
            let id = v.local_device().device_id;
            v.lock();
            id
        };

        let backup_path = dir.path().join("out.tockbak");
        {
            let v = vault::open(&vault_path, b"pw", &sk).expect("open");
            create(&v, &backup_path).expect("create");
            v.lock();
        }

        // Restore as a clone to a *fresh* path (second device).
        let clone_path = dir.path().join("clone.tockvault");
        let outcome = restore(
            &backup_path,
            &clone_path,
            b"pw",
            &sk,
            account_id,
            RestoreMode::Clone,
            false,
        )
        .expect("clone restore");

        let new_id = outcome.new_device_id.expect("clone mints an id");
        assert_ne!(
            new_id, original_device_id,
            "clone must not reuse the original device id"
        );

        let v = vault::open(&clone_path, b"pw", &sk).expect("open clone");
        assert_eq!(v.local_device().device_id, new_id);
        // Sync state reset to local-only.
        assert_eq!(crate::sync::pull_cursor(&v).expect("cursor"), 0);
        assert_eq!(
            crate::sync::binding_state(&v).expect("binding"),
            crate::sync::BINDING_LOCAL_ONLY
        );
        assert!(crate::sync::server_url(&v).expect("url").is_none());
        // (ADR-018 §3B step 3) The one-shot reconcile-before-push flag is set
        // so the clone pulls remote history before its first push.
        assert!(
            crate::sync::pending_reconcile(&v).expect("pending_reconcile"),
            "clone restore must arm reconcile-before-push"
        );
        // The original device's events keep their lamports (no corruption):
        // the new device starts a fresh sequence.
        let hw = HighWater::read(v.connection()).expect("hw");
        assert!(hw.event_count >= 1);
        v.lock();
    }

    /// The one-shot reconcile flag is orthogonal to the binding: clearing the
    /// server binding (as a clone restore does, before it arms the flag) must
    /// NOT clobber `pending_reconcile`. This guards the ADR-018 §3B invariant
    /// independently of the set-after-clear ordering in the restore path.
    #[test]
    fn clear_binding_preserves_pending_reconcile() {
        let dir = tempdir().expect("dir");
        let vault_path = dir.path().join("v.tockvault");
        let (sk, _account_id) = seed_vault(&vault_path);

        let v = vault::open(&vault_path, b"pw", &sk).expect("open");
        crate::sync::set_pending_reconcile(&v).expect("arm reconcile");
        crate::sync::clear_binding(&v).expect("clear binding");
        assert!(
            crate::sync::pending_reconcile(&v).expect("pending_reconcile"),
            "clear_binding must not clear the one-shot reconcile flag"
        );
        // And the dedicated clear does turn it off.
        crate::sync::clear_pending_reconcile(&v).expect("clear reconcile");
        assert!(
            !crate::sync::pending_reconcile(&v).expect("pending_reconcile"),
            "clear_pending_reconcile must disarm the flag"
        );
        v.lock();
    }
}
