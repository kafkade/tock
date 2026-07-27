//! Scheduled, server-retained ciphertext snapshots (issue #201, part 3).
//!
//! This is **server-side backup**, distinct from the two portability features in
//! `routes.rs`:
//!
//! - *export* (`GET …/export`) and *import* (`POST …/import`) let a client pull
//!   or restore its own ciphertext — portability, driven by the client.
//! - *retained snapshots* (here) run on the server on a schedule so the data
//!   survives if the server database is lost **before** the user ever downloads
//!   it. A download endpoint is not a backup.
//!
//! Each snapshot is a consistent, self-contained `SQLite` copy taken with
//! `VACUUM INTO` (the same primitive the client `tock backup` uses), written to
//! a timestamped file under a retention directory, with the oldest files pruned
//! to keep the newest N. Snapshots are **ciphertext only** — the server never
//! decrypts.
//!
//! ## PITR scope
//!
//! This implements *scheduled snapshot retention*, not strict write-ahead-log
//! point-in-time recovery. Continuous WAL archiving + replay to an arbitrary
//! instant is materially heavier and low-value for a personal-scale ciphertext
//! relay whose clients hold the authoritative log and can re-push; it is
//! deliberately deferred (see the issue #201 discussion).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::db::ServerDb;
use crate::error::Error;

/// Filename prefix for retained snapshots. The fixed-width UTC timestamp that
/// follows makes lexical order equal to chronological order, so pruning can sort
/// by name.
const SNAPSHOT_PREFIX: &str = "tock-server-";
/// Filename suffix for retained snapshots.
const SNAPSHOT_SUFFIX: &str = ".db";

/// Configuration for the scheduled snapshot task.
#[derive(Clone, Debug)]
pub struct RetentionConfig {
    /// Interval between snapshots. A zero interval disables retention.
    pub interval: Duration,
    /// Directory snapshots are written to (created on demand).
    pub dir: PathBuf,
    /// Number of most-recent snapshots to keep; older ones are pruned.
    pub keep: usize,
}

impl RetentionConfig {
    /// Build a config from raw parts (as parsed from flags/env).
    #[must_use]
    pub const fn new(interval_secs: u64, dir: PathBuf, keep: usize) -> Self {
        Self {
            interval: Duration::from_secs(interval_secs),
            dir,
            keep,
        }
    }

    /// Whether scheduled retention should run. Disabled when the interval is
    /// zero or nothing would be kept.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        !self.interval.is_zero() && self.keep > 0
    }
}

/// Build the timestamped filename for a snapshot taken at `now`.
///
/// Format: `tock-server-YYYYMMDDThhmmssZ.db` (UTC). Zero-padded and colon-free so
/// it is filesystem-safe and sorts chronologically by name.
#[must_use]
pub fn snapshot_filename(now: time::OffsetDateTime) -> String {
    let now = now.to_offset(time::UtcOffset::UTC);
    format!(
        "{SNAPSHOT_PREFIX}{:04}{:02}{:02}T{:02}{:02}{:02}Z{SNAPSHOT_SUFFIX}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
    )
}

/// Take one snapshot now: `VACUUM INTO` a fresh timestamped file under
/// `cfg.dir`, then prune older snapshots to `cfg.keep`.
///
/// Returns the path written. `VACUUM INTO` refuses to overwrite, so on the rare
/// same-second collision a numeric disambiguator is appended.
///
/// # Errors
/// Returns an error if the directory cannot be created or the snapshot fails.
pub fn take_snapshot(db: &ServerDb, cfg: &RetentionConfig) -> Result<PathBuf, Error> {
    std::fs::create_dir_all(&cfg.dir)
        .map_err(|e| Error::Internal(format!("create snapshot dir: {e}")))?;

    let base = snapshot_filename(time::OffsetDateTime::now_utc());
    let mut path = cfg.dir.join(&base);
    let mut n = 1_u32;
    while path.exists() {
        // Use `_` (which sorts after `.`) so a same-second collision suffix keeps
        // lexical order equal to creation order for pruning.
        let stem = base.strip_suffix(SNAPSHOT_SUFFIX).unwrap_or(&base);
        path = cfg.dir.join(format!("{stem}_{n}{SNAPSHOT_SUFFIX}"));
        n += 1;
    }

    db.snapshot_to(&path)?;
    prune(&cfg.dir, cfg.keep)?;
    Ok(path)
}

/// Delete all but the `keep` newest snapshot files in `dir`. Returns the paths
/// removed (useful for logging/tests). Only files matching the snapshot naming
/// convention are considered; anything else in the directory is left untouched.
///
/// # Errors
/// Returns an error if the directory cannot be read or a file cannot be removed.
pub fn prune(dir: &Path, keep: usize) -> Result<Vec<PathBuf>, Error> {
    let mut snapshots: Vec<PathBuf> = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // No directory yet means nothing to prune.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::Internal(format!("read snapshot dir: {e}"))),
    };
    for entry in entries {
        let entry = entry.map_err(|e| Error::Internal(format!("read snapshot entry: {e}")))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(SNAPSHOT_PREFIX) && name.ends_with(SNAPSHOT_SUFFIX) {
            snapshots.push(entry.path());
        }
    }
    // Fixed-width timestamp ⇒ lexical order is chronological.
    snapshots.sort();
    if snapshots.len() <= keep {
        return Ok(Vec::new());
    }
    let remove_count = snapshots.len() - keep;
    let mut removed = Vec::with_capacity(remove_count);
    for path in snapshots.into_iter().take(remove_count) {
        std::fs::remove_file(&path)
            .map_err(|e| Error::Internal(format!("prune snapshot {}: {e}", path.display())))?;
        removed.push(path);
    }
    Ok(removed)
}

/// Spawn the background retention loop on the current Tokio runtime.
///
/// Does nothing (beyond logging) when retention is disabled. The first snapshot
/// is taken one full interval after startup, not immediately. Each snapshot runs
/// on a blocking thread so it never stalls the async runtime.
pub fn spawn(db: Arc<ServerDb>, cfg: RetentionConfig) {
    if !cfg.enabled() {
        tracing::info!("server-retained snapshots disabled (set a non-zero interval to enable)");
        return;
    }
    tracing::info!(
        interval_secs = cfg.interval.as_secs(),
        dir = %cfg.dir.display(),
        keep = cfg.keep,
        "server-retained snapshots enabled"
    );
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(cfg.interval);
        // The first tick fires immediately; consume it so snapshots align to the
        // configured interval rather than firing on startup.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let db = db.clone();
            let cfg = cfg.clone();
            let result = tokio::task::spawn_blocking(move || take_snapshot(&db, &cfg)).await;
            match result {
                Ok(Ok(path)) => {
                    tracing::info!(snapshot = %path.display(), "wrote retained snapshot");
                }
                Ok(Err(e)) => tracing::error!(error = %e, "retained snapshot failed"),
                Err(e) => tracing::error!(error = %e, "retained snapshot task panicked"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn disabled_when_interval_zero_or_keep_zero() {
        let dir = PathBuf::from("/tmp/tock-snapshots");
        assert!(!RetentionConfig::new(0, dir.clone(), 7).enabled());
        assert!(!RetentionConfig::new(3600, dir.clone(), 0).enabled());
        assert!(RetentionConfig::new(3600, dir, 7).enabled());
    }

    #[test]
    fn snapshot_filename_is_sortable_and_safe() {
        let t = time::OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let name = snapshot_filename(t);
        assert!(name.starts_with(SNAPSHOT_PREFIX));
        assert!(name.ends_with(SNAPSHOT_SUFFIX));
        assert!(!name.contains(':'), "filename must be filesystem-safe");
        // Earlier instants sort before later ones lexically.
        let later = time::OffsetDateTime::from_unix_timestamp(1_700_000_060).unwrap();
        assert!(snapshot_filename(t) < snapshot_filename(later));
    }

    #[test]
    fn prune_keeps_newest_n() {
        let tmp = tempfile::tempdir().expect("tmp");
        let dir = tmp.path();
        // Chronologically increasing, fixed-width names.
        let names = [
            "tock-server-20260101T000000Z.db",
            "tock-server-20260102T000000Z.db",
            "tock-server-20260103T000000Z.db",
            "tock-server-20260104T000000Z.db",
        ];
        for name in names {
            std::fs::write(dir.join(name), b"x").expect("write");
        }
        // An unrelated file must be left alone.
        std::fs::write(dir.join("notes.txt"), b"keep me").expect("write notes");

        let removed = prune(dir, 2).expect("prune");
        assert_eq!(removed.len(), 2);
        assert!(!dir.join(names[0]).exists());
        assert!(!dir.join(names[1]).exists());
        assert!(dir.join(names[2]).exists());
        assert!(dir.join(names[3]).exists());
        assert!(dir.join("notes.txt").exists());
    }

    #[test]
    fn prune_missing_dir_is_noop() {
        let removed = prune(Path::new("/no/such/tock/dir"), 3).expect("prune");
        assert!(removed.is_empty());
    }

    #[test]
    fn take_snapshot_creates_and_prunes() {
        let tmp = tempfile::tempdir().expect("tmp");
        let db_path = tmp.path().join("tock-server.db");
        let db = ServerDb::open(&db_path).expect("open");
        db.ensure_vault(&[1_u8; 16]).expect("vault");

        let cfg = RetentionConfig::new(3600, tmp.path().join("snapshots"), 1);
        // Two snapshots in quick succession collide on the same second; the
        // disambiguator keeps both distinct, then keep=1 prunes to one.
        let first = take_snapshot(&db, &cfg).expect("first");
        assert!(first.exists());
        let second = take_snapshot(&db, &cfg).expect("second");
        assert!(second.exists());

        let remaining: Vec<_> = std::fs::read_dir(&cfg.dir)
            .expect("read dir")
            .filter_map(Result::ok)
            .filter(|e| {
                let n = e.file_name();
                let n = n.to_string_lossy();
                n.starts_with(SNAPSHOT_PREFIX) && n.ends_with(SNAPSHOT_SUFFIX)
            })
            .collect();
        assert_eq!(remaining.len(), 1, "keep=1 must leave exactly one snapshot");
    }
}
