//! End-to-end tests for the sync-time state-diff substrate.
//!
//! These exercise [`tock_storage::sync`] across two vaults that share a
//! Vault Key and vault id (the onboarding outcome), passing the signed
//! event frames between them by value (the role the network transport
//! plays in production). They assert clean one-way propagation, a clean
//! offline merge of disjoint edits, and that the wire frames carry no
//! plaintext.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;

use tock_core::domain::task::{NewTask, Priority, TaskPatch};
use tock_core::domain::urgency::UrgencyConfig;
use tock_core::vault::VaultKey;
use tock_storage::repo::task_repo;
use tock_storage::sync;
use tock_storage::vault::{self, OpenVault};

/// Clone a vault key by value (test-only; production transfers the key
/// over the encrypted onboarding channel).
#[allow(clippy::missing_const_for_fn)]
fn clone_vault_key(v: &OpenVault) -> VaultKey {
    VaultKey::from_secret(v.vault_key().as_secret().clone_secret())
}

/// Create device A (fresh vault) and device B (same VK + vault id).
fn two_devices(dir: &Path) -> (OpenVault, OpenVault) {
    let path_a = dir.join("a.tockvault");
    let (a, secret_key) = vault::init(&path_a, b"password-a").expect("init a");
    let vk_b = clone_vault_key(&a);
    let vault_id = a.header().vault_id;
    let account_id = a.header().account_id;
    let path_b = dir.join("b.tockvault");
    let b = vault::init_with_key(
        &path_b,
        b"password-b",
        &secret_key,
        account_id,
        vault_id,
        vk_b,
        Some("device-b"),
    )
    .expect("init b");
    (a, b)
}

/// Ship every event `from` produced into `to` by value.
fn ship(from: &OpenVault, to: &OpenVault) -> usize {
    let events = sync::collect_local_changes(from).expect("collect");
    sync::ingest_events(to, &events).expect("ingest");
    events.len()
}

/// A realistic bidirectional sync round: each device first captures its
/// own local edits as events (the push phase), then ingests the other's
/// (the pull phase). Capturing before ingesting is what lets disjoint
/// concurrent edits field-merge instead of clobbering each other.
fn sync_round(a: &OpenVault, b: &OpenVault) {
    let ea = sync::collect_local_changes(a).expect("collect a");
    let eb = sync::collect_local_changes(b).expect("collect b");
    sync::ingest_events(b, &ea).expect("ingest into b");
    sync::ingest_events(a, &eb).expect("ingest into a");
}

#[test]
fn clean_one_way_propagation() {
    let dir = tempfile::tempdir().expect("tmp");
    let (a, b) = two_devices(dir.path());

    let task = task_repo::insert(
        a.connection(),
        &NewTask {
            title: "buy milk".into(),
            ..NewTask::default()
        },
        &UrgencyConfig::default(),
    )
    .expect("insert");

    let shipped = ship(&a, &b);
    assert!(shipped >= 2, "expected device + task events, got {shipped}");

    let on_b = task_repo::get_by_id(b.connection(), task.id)
        .expect("get")
        .expect("task present on b");
    assert_eq!(on_b.title, "buy milk");
}

#[test]
fn clean_offline_merge_disjoint_fields() {
    let dir = tempfile::tempdir().expect("tmp");
    let (a, b) = two_devices(dir.path());

    let task = task_repo::insert(
        a.connection(),
        &NewTask {
            title: "draft report".into(),
            ..NewTask::default()
        },
        &UrgencyConfig::default(),
    )
    .expect("insert");
    ship(&a, &b);
    ship(&b, &a);

    let sid_a = task_repo::get_by_id(a.connection(), task.id)
        .expect("get")
        .expect("present")
        .sid;
    let sid_b = task_repo::get_by_id(b.connection(), task.id)
        .expect("get")
        .expect("present")
        .sid;

    // Disjoint edits while "offline": A edits notes, B edits priority.
    task_repo::update(
        a.connection(),
        sid_a,
        &TaskPatch {
            notes: Some(Some("first pass".into())),
            ..Default::default()
        },
        &UrgencyConfig::default(),
    )
    .expect("update a");
    task_repo::update(
        b.connection(),
        sid_b,
        &TaskPatch {
            priority: Some(Some(Priority::High)),
            ..Default::default()
        },
        &UrgencyConfig::default(),
    )
    .expect("update b");

    sync_round(&a, &b);

    let final_a = task_repo::get_by_id(a.connection(), task.id)
        .expect("get")
        .expect("present");
    assert_eq!(final_a.notes.as_deref(), Some("first pass"));
    assert_eq!(final_a.priority, Some(Priority::High));
}

#[test]
fn wire_frames_are_ciphertext_only() {
    let dir = tempfile::tempdir().expect("tmp");
    let (a, _b) = two_devices(dir.path());

    task_repo::insert(
        a.connection(),
        &NewTask {
            title: "SECRETTITLE".into(),
            ..NewTask::default()
        },
        &UrgencyConfig::default(),
    )
    .expect("insert");

    let events = sync::collect_local_changes(&a).expect("collect");
    let frame = tock_sync::wire::encode_batch(&events).expect("encode");
    let needle = b"SECRETTITLE";
    assert!(
        !frame.windows(needle.len()).any(|w| w == needle),
        "plaintext title must not appear in the wire frame"
    );
}

#[test]
fn concurrent_same_field_edits_surface_a_conflict() {
    let dir = tempfile::tempdir().expect("tmp");
    let (a, b) = two_devices(dir.path());

    // Shared starting point: a task known to both devices.
    let task = task_repo::insert(
        a.connection(),
        &NewTask {
            title: "plan trip".into(),
            ..NewTask::default()
        },
        &UrgencyConfig::default(),
    )
    .expect("insert");
    ship(&a, &b);
    ship(&b, &a);

    let sid_a = task_repo::get_by_id(a.connection(), task.id)
        .expect("get")
        .expect("present")
        .sid;
    let sid_b = task_repo::get_by_id(b.connection(), task.id)
        .expect("get")
        .expect("present")
        .sid;

    // Both devices edit the SAME field while offline.
    task_repo::update(
        a.connection(),
        sid_a,
        &TaskPatch {
            title: Some("plan trip to Lisbon".into()),
            ..Default::default()
        },
        &UrgencyConfig::default(),
    )
    .expect("update a");
    task_repo::update(
        b.connection(),
        sid_b,
        &TaskPatch {
            title: Some("plan trip to Tokyo".into()),
            ..Default::default()
        },
        &UrgencyConfig::default(),
    )
    .expect("update b");

    sync_round(&a, &b);

    // Per ADR-003 there is no silent last-write-wins: an overlapping
    // edit on the same field must be recorded for review rather than
    // quietly clobbered. Each side ingested the other's conflicting
    // edit, so each should have logged a conflict.
    let conflicts_a = sync::list_conflicts(a.connection()).expect("list a");
    let conflicts_b = sync::list_conflicts(b.connection()).expect("list b");
    assert!(
        !conflicts_a.is_empty(),
        "device A should surface the concurrent edit for review"
    );
    assert!(
        !conflicts_b.is_empty(),
        "device B should surface the concurrent edit for review"
    );

    // The conflict is resolvable: marking it resolved clears it from the
    // review list (what `tock sync resolve <id>` drives).
    let first = conflicts_a[0].id;
    assert!(
        sync::resolve_conflict(a.connection(), first).expect("resolve"),
        "resolving a known conflict id should report success"
    );
    let remaining = sync::list_conflicts(a.connection()).expect("list a again");
    assert_eq!(
        remaining.len(),
        conflicts_a.len() - 1,
        "resolving a conflict should remove exactly one from the review list"
    );
}
