//! Tests for the 1Password-style new-device login bootstrap (issue #129):
//! a fresh device recovers a vault from the *remote header* plus the account
//! password + Secret Key, with no device-to-device pairing.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use tock_core::domain::task::NewTask;
use tock_core::vault::VaultHeader;
use tock_storage::repo::task_repo;
use tock_storage::sync;
use tock_storage::vault;

#[test]
fn new_device_recovers_vault_from_header() {
    let dir = tempfile::tempdir().expect("tempdir");

    // Device A creates the account vault and adds a task.
    let path_a = dir.path().join("a.tockvault");
    let (a, secret_key) = vault::init(&path_a, b"correct horse").expect("init a");
    task_repo::insert(
        a.connection(),
        &NewTask {
            title: "from device A".to_string(),
            ..Default::default()
        },
        &tock_core::domain::urgency::UrgencyConfig::default(),
    )
    .expect("add task");

    // The non-secret header travels through the server (here: bytes by value).
    let header_bytes = a.header().to_bytes();
    let header = VaultHeader::from_bytes(&header_bytes).expect("parse header");

    // Device B signs in with only password + Secret Key + the fetched header.
    let path_b = dir.path().join("b.tockvault");
    let b = vault::init_from_header(&path_b, b"correct horse", &secret_key, &header, Some("B"))
        .expect("init from header");

    // Same vault identity + Vault Key → B decrypts A's events.
    assert_eq!(b.header().vault_id, a.header().vault_id);
    assert_eq!(b.header().account_id, a.header().account_id);
    let events = sync::collect_local_changes(&a).expect("collect a");
    sync::ingest_events(&b, &events).expect("ingest into b");
    let tasks = task_repo::list(b.connection(), false).expect("list b");
    assert!(tasks.iter().any(|t| t.title == "from device A"));
}

#[test]
fn wrong_password_rejected_on_header_recovery() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path_a = dir.path().join("a.tockvault");
    let (a, secret_key) = vault::init(&path_a, b"right-pass").expect("init a");
    let header = VaultHeader::from_bytes(&a.header().to_bytes()).expect("parse");

    let path_b = dir.path().join("b.tockvault");
    let err = vault::init_from_header(&path_b, b"wrong-pass", &secret_key, &header, None);
    assert!(matches!(
        err,
        Err(tock_storage::Error::InvalidVaultOrCredentials)
    ));
}
