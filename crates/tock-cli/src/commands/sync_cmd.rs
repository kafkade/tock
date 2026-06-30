//! `tock sync`, `tock onboard`, and `tock device` subcommands.
//!
//! These commands drive the event-sourced multi-device sync substrate
//! (`tock_storage::sync`) over a concrete [`HttpTransport`] against a
//! self-hosted `tock-server`. A small current-thread Tokio runtime
//! bridges the synchronous CLI to the async [`Transport`] trait.

use std::io::Write as _;

use clap::Subcommand;
use tock_core::event::{DeviceId, SignedEvent};
use tock_storage::OpenVault;
use tock_storage::sync;
use tock_sync::pairing;
use tock_sync::transport::{SyncCursor, Transport};

use crate::http_transport::HttpTransport;
use tock_account::CredentialStore as _;

/// Boxed dynamic error alias for command handlers.
type CmdResult = Result<(), Box<dyn std::error::Error>>;

/// How many events to request per pull page.
const PULL_PAGE: usize = 256;

// ── Argument definitions ─────────────────────────────────────────────

/// `tock sync` — push local changes and pull remote ones.
#[derive(Debug, clap::Args)]
pub struct SyncArgs {
    /// Sync server base URL. Persisted on first use; optional thereafter.
    #[arg(long)]
    pub server: Option<String>,
    /// Compute and locally record pending changes, print what would be
    /// pushed, but do not contact the server.
    #[arg(long)]
    pub dry_run: bool,
    /// Sub-action (conflict review). Omit to run a normal sync.
    #[command(subcommand)]
    pub cmd: Option<SyncCmd>,
}

/// Conflict-review sub-actions under `tock sync`.
#[derive(Debug, Subcommand)]
pub enum SyncCmd {
    /// List unresolved sync conflicts awaiting review.
    Conflicts,
    /// Mark a conflict resolved by its id (from `tock sync conflicts`).
    Resolve {
        /// Conflict id (UUID).
        id: String,
    },
}

/// `tock onboard` — pair a new device with an existing vault.
#[derive(Debug, clap::Args)]
pub struct OnboardArgs {
    /// Onboarding role.
    #[command(subcommand)]
    pub cmd: OnboardCmd,
}

/// Onboarding roles.
#[derive(Debug, Subcommand)]
pub enum OnboardCmd {
    /// On an existing device: invite a new device and transfer the vault key.
    Invite {
        /// Sync server base URL (defaults to the persisted one).
        #[arg(long)]
        server: Option<String>,
    },
    /// On the new device: accept an invite and create a local vault.
    Accept {
        /// Sync server base URL.
        #[arg(long)]
        server: String,
        /// Vault id (hex), from the inviter's output.
        #[arg(long)]
        vault_id: String,
        /// Inviter's ephemeral X25519 public key (hex), from the invite.
        #[arg(long)]
        inviter_pubkey: String,
        /// Inviter's fingerprint (hex), for out-of-band confirmation.
        #[arg(long)]
        inviter_fingerprint: String,
    },
}

/// `tock device` — inspect and revoke registered devices.
#[derive(Debug, clap::Args)]
pub struct DeviceArgs {
    /// Device sub-action.
    #[command(subcommand)]
    pub cmd: DeviceCmd,
}

/// Device sub-actions.
#[derive(Debug, Subcommand)]
pub enum DeviceCmd {
    /// List registered devices.
    #[command(alias = "list")]
    Ls,
    /// Revoke a device by id (hex prefix accepted).
    Revoke {
        /// Device id (full or unambiguous hex prefix).
        device: String,
    },
}

// ── `tock sync` ──────────────────────────────────────────────────────

/// Run a sync (or a conflict-review sub-action).
///
/// # Errors
/// Propagates storage, transport, or argument-parsing failures.
pub fn run_sync(vault: &OpenVault, args: &SyncArgs) -> CmdResult {
    if let Some(cmd) = &args.cmd {
        return match cmd {
            SyncCmd::Conflicts => list_conflicts(vault),
            SyncCmd::Resolve { id } => {
                let uuid = uuid::Uuid::parse_str(id)
                    .map_err(|_| Box::<dyn std::error::Error>::from("invalid conflict id"))?;
                if sync::resolve_conflict(vault.connection(), uuid)? {
                    println!("Resolved conflict {id}.");
                } else {
                    println!("No unresolved conflict with id {id}.");
                }
                Ok(())
            }
        };
    }

    let server = resolve_server(vault, args.server.as_deref())?;

    // Outbound events are genuine local-state deltas; recording them in
    // the log is correct regardless of whether we reach the server.
    let outbound = sync::collect_local_changes(vault)?;

    if args.dry_run {
        println!(
            "Dry run: {} local change(s) pending; server not contacted.",
            outbound.len()
        );
        return Ok(());
    }

    let device = vault.local_device();
    let device_id = DeviceId::from_bytes(device.device_id);
    let vk = device.signing_key.verifying_key().to_bytes();
    let label = sync::device_label(vault)?;
    let vault_id = vault.header().vault_id;

    let transport = authed_transport(&server, vault_id)?;
    let runtime = tokio_runtime()?;

    let outcome = runtime.block_on(async {
        transport
            .register_device(device_id, &vk, label.as_deref())
            .await?;

        let pushed = push_all(&transport, &outbound).await?;

        let mut cursor = SyncCursor::at(sync::pull_cursor(vault)?);
        let mut pulled = 0_usize;
        let mut conflicts = 0_usize;
        loop {
            let batch = transport.pull(cursor, PULL_PAGE).await?;
            if !batch.events.is_empty() {
                let summary = sync::ingest_events(vault, &batch.events)?;
                pulled += summary.applied;
                conflicts += summary.conflicts;
            }
            cursor = batch.next_cursor;
            sync::set_pull_cursor(vault, cursor.position)?;
            if !batch.more {
                break;
            }
        }
        Ok::<_, Box<dyn std::error::Error>>(SyncOutcome {
            pushed,
            pulled,
            conflicts,
        })
    })?;

    println!(
        "Synced with {server}: pushed {}, pulled {}, conflicts {}.",
        outcome.pushed, outcome.pulled, outcome.conflicts
    );
    if outcome.conflicts > 0 {
        println!("Review conflicts with `tock sync conflicts`.");
    }
    Ok(())
}

/// Result of a completed sync round.
struct SyncOutcome {
    pushed: usize,
    pulled: usize,
    conflicts: usize,
}

/// Push every event the server doesn't already have, returning the count
/// it newly accepted.
async fn push_all(
    transport: &HttpTransport,
    events: &[SignedEvent],
) -> Result<usize, Box<dyn std::error::Error>> {
    if events.is_empty() {
        return Ok(0);
    }
    let ack = transport.push(events).await?;
    Ok(ack.accepted)
}

/// Print unresolved conflicts.
fn list_conflicts(vault: &OpenVault) -> CmdResult {
    let conflicts = sync::list_conflicts(vault.connection())?;
    if conflicts.is_empty() {
        println!("No unresolved conflicts.");
        return Ok(());
    }
    println!("Unresolved conflicts:");
    for c in &conflicts {
        println!(
            "  [{}] {} {} — {}",
            c.id, c.entity_kind, c.entity_id, c.detail
        );
    }
    println!("Resolve with `tock sync resolve <id>`.");
    Ok(())
}

/// Determine the server URL, persisting an explicitly supplied one.
fn resolve_server(
    vault: &OpenVault,
    flag: Option<&str>,
) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(url) = flag {
        sync::set_server_url(vault, url)?;
        return Ok(url.to_string());
    }
    sync::server_url(vault)?.ok_or_else(|| {
        Box::<dyn std::error::Error>::from(
            "no sync server configured; pass --server <url> once to set it",
        )
    })
}

// ── `tock onboard` ───────────────────────────────────────────────────

/// Existing-device side of onboarding: transfer the vault key to a new
/// device over the pairing channel.
///
/// # Errors
/// Propagates storage, transport, crypto, and I/O failures.
pub fn run_onboard_invite(vault: &OpenVault, server_flag: Option<&str>) -> CmdResult {
    let server = resolve_server(vault, server_flag)?;
    let vault_id = vault.header().vault_id;

    let (secret, invite) = pairing::generate_invite(vault_id, &server)?;
    println!("Share these values with the new device's `tock onboard accept`:");
    println!("  --server {server}");
    println!("  --vault-id {}", hex_encode(vault_id.as_bytes()));
    println!(
        "  --inviter-pubkey {}",
        hex_encode(invite.ephemeral_pubkey.as_bytes())
    );
    println!(
        "  --inviter-fingerprint {}",
        hex_encode(&invite.fingerprint)
    );
    println!();

    let peer_pubkey = prompt_hex32("Acceptor public key (hex): ")?;
    let peer_fp = prompt_hex8("Acceptor fingerprint (hex): ")?;
    let rendezvous = prompt_hex16("Acceptor device id (hex): ")?;

    let target = DeviceId::from_bytes(rendezvous);
    let blob = pairing::compute_onboarding_blob(
        secret,
        &tock_crypto::keyexchange::PublicKey::from_bytes(peer_pubkey),
        &peer_fp,
        vault.vault_key(),
        vault_id,
        target,
    )?;

    let transport = authed_transport(&server, vault_id)?;
    let runtime = tokio_runtime()?;
    runtime.block_on(transport.put_onboarding_blob(target, blob))?;

    println!("Vault key blob uploaded. The new device can now finish onboarding.");
    Ok(())
}

/// New-device side of onboarding: recover the vault key and create a
/// local vault file at `path`.
///
/// # Errors
/// Propagates pairing, transport, storage, and I/O failures.
pub fn run_onboard_accept(
    path: &std::path::Path,
    password: &[u8],
    secret_key: Option<&str>,
    server: &str,
    vault_id_hex: &str,
    inviter_pubkey_hex: &str,
    inviter_fp_hex: &str,
) -> CmdResult {
    if path.exists() {
        return Err(format!(
            "vault file {} already exists; choose a fresh --vault path for the new device",
            path.display()
        )
        .into());
    }
    let raw_sk = secret_key.ok_or(
        "missing account Secret Key: pass --secret-key or set TOCK_SECRET_KEY \
         (the `A4-…` string from your Emergency Kit) so this device joins the same account",
    )?;
    let (account_id, account_secret_key) = tock_crypto::SecretKey::parse(raw_sk)
        .map_err(|_| "invalid account Secret Key: check the Emergency-Kit string and try again")?;
    let account_id = uuid::Uuid::from_bytes(account_id);
    let vault_id = uuid_from_hex(vault_id_hex)?;
    let inviter_pk =
        tock_crypto::keyexchange::PublicKey::from_bytes(parse_hex32(inviter_pubkey_hex)?);
    let inviter_fp = parse_hex8(inviter_fp_hex)?;
    // Out-of-band fingerprint check: confirm the inviter pubkey we were
    // given matches the fingerprint the inviter displayed, to defeat a
    // man-in-the-middle swapping the key.
    if pairing::fingerprint(&inviter_pk) != inviter_fp {
        return Err("inviter fingerprint mismatch — aborting (possible MITM)".into());
    }

    let acceptor = pairing::accept_invite()?;
    let mut rendezvous = [0_u8; 16];
    tock_crypto::random::fill_random(&mut rendezvous)?;
    let rendezvous_id = DeviceId::from_bytes(rendezvous);

    println!("Give these values to the inviter's `tock onboard invite` prompt:");
    println!(
        "  Acceptor public key: {}",
        hex_encode(acceptor.public_key().as_bytes())
    );
    println!(
        "  Acceptor fingerprint: {}",
        hex_encode(&acceptor.fingerprint())
    );
    println!("  Acceptor device id: {}", hex_encode(&rendezvous));
    println!();
    println!("Waiting for the inviter to upload the vault key...");

    let transport = HttpTransport::new(server, vault_id)?;
    let runtime = tokio_runtime()?;
    let blob = runtime.block_on(poll_blob(&transport, rendezvous_id))?;

    let vk = pairing::open_onboarding_blob(acceptor, &inviter_pk, &blob, vault_id)?;
    let new_vault = tock_storage::vault::init_with_key(
        path,
        password,
        &account_secret_key,
        account_id,
        vault_id,
        vk,
        Some("paired"),
    )?;

    // Persist the server URL and register this device so it can sync.
    sync::set_server_url(&new_vault, server)?;
    let device = new_vault.local_device();
    let device_id = DeviceId::from_bytes(device.device_id);
    let vkey = device.signing_key.verifying_key().to_bytes();
    let reg_transport = HttpTransport::new(server, vault_id)?;
    runtime.block_on(reg_transport.register_device(device_id, &vkey, Some("paired")))?;

    // Pull existing history so the new vault is populated immediately.
    let mut cursor = SyncCursor::start();
    runtime.block_on(async {
        loop {
            let batch = reg_transport.pull(cursor, PULL_PAGE).await?;
            if !batch.events.is_empty() {
                sync::ingest_events(&new_vault, &batch.events)?;
            }
            cursor = batch.next_cursor;
            sync::set_pull_cursor(&new_vault, cursor.position)?;
            if !batch.more {
                break;
            }
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    new_vault.lock();

    println!("Onboarding complete. Vault created at {}.", path.display());
    Ok(())
}

/// Poll the server for the onboarding blob until it appears or the
/// pairing window expires.
async fn poll_blob(
    transport: &HttpTransport,
    device: DeviceId,
) -> Result<tock_sync::transport::OnboardingBlob, Box<dyn std::error::Error>> {
    let deadline = std::time::Instant::now()
        + std::time::Duration::from_secs(
            u64::try_from(pairing::PAIRING_EXPIRY_SECS).unwrap_or(300),
        );
    loop {
        if let Some(blob) = transport.get_onboarding_blob(device).await? {
            return Ok(blob);
        }
        if std::time::Instant::now() >= deadline {
            return Err("pairing timed out waiting for the vault key blob".into());
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

// ── `tock device` ────────────────────────────────────────────────────

/// Handle `tock device` sub-actions.
///
/// # Errors
/// Propagates storage failures.
pub fn run_device(vault: &OpenVault, cmd: &DeviceCmd) -> CmdResult {
    match cmd {
        DeviceCmd::Ls => device_ls(vault),
        DeviceCmd::Revoke { device } => device_revoke(vault, device),
    }
}

fn device_ls(vault: &OpenVault) -> CmdResult {
    let local = vault.local_device().device_id;
    let conn = vault.connection();
    let mut stmt = conn.prepare(
        "SELECT device_id, label, registered_at, revoked_at FROM devices ORDER BY registered_at",
    )?;
    let rows = stmt.query_map([], |row| {
        let id: Vec<u8> = row.get(0)?;
        let label: Option<String> = row.get(1)?;
        let registered: String = row.get(2)?;
        let revoked: Option<String> = row.get(3)?;
        Ok((id, label, registered, revoked))
    })?;

    println!(
        "{:<34} {:<12} {:<20} STATUS",
        "DEVICE ID", "LABEL", "REGISTERED"
    );
    for row in rows {
        let (id, label, registered, revoked) = row?;
        let is_local = id.as_slice() == local.as_slice();
        let id_hex = hex_encode(&id);
        let marker = if is_local { " (this device)" } else { "" };
        let status = revoked.map_or_else(|| "active".to_string(), |when| format!("revoked {when}"));
        println!(
            "{id_hex:<34} {:<12} {registered:<20} {status}{marker}",
            label.unwrap_or_default()
        );
    }
    Ok(())
}

fn device_revoke(vault: &OpenVault, prefix: &str) -> CmdResult {
    let local = vault.local_device().device_id;
    let conn = vault.connection();
    let mut stmt = conn.prepare("SELECT device_id FROM devices")?;
    let ids: Vec<Vec<u8>> = stmt
        .query_map([], |row| row.get::<_, Vec<u8>>(0))?
        .collect::<Result<_, _>>()?;

    let needle = prefix.to_ascii_lowercase();
    let matches: Vec<&Vec<u8>> = ids
        .iter()
        .filter(|id| hex_encode(id).starts_with(&needle))
        .collect();

    let target = match matches.as_slice() {
        [] => return Err(format!("no device matches '{prefix}'").into()),
        [one] => *one,
        _ => return Err(format!("'{prefix}' is ambiguous; supply more characters").into()),
    };

    if target.as_slice() == local.as_slice() {
        return Err("refusing to revoke the current device".into());
    }

    let now = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))?;
    conn.execute(
        "UPDATE devices SET revoked_at = ?1 WHERE device_id = ?2",
        rusqlite::params![now, target],
    )?;
    println!(
        "Revoked device {}. The revocation propagates on the next `tock sync`.",
        hex_encode(target)
    );
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Build a current-thread Tokio runtime for the duration of a command.
fn tokio_runtime() -> Result<tokio::runtime::Runtime, Box<dyn std::error::Error>> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Into::into)
}

/// Build an [`HttpTransport`], attaching SRP session credentials from the OS
/// keyring when an account is signed in (issue #129). Pre-account device
/// pairing still works against unauthenticated servers.
fn authed_transport(
    server: &str,
    vault_id: uuid::Uuid,
) -> Result<HttpTransport, Box<dyn std::error::Error>> {
    let transport = HttpTransport::new(server, vault_id)?;
    Ok(match crate::commands::account::KeyringStore.load() {
        Ok(Some(creds)) if !creds.bearer_token.is_empty() => {
            transport.with_auth(creds.bearer_token, creds.channel_binding)
        }
        _ => transport,
    })
}

fn prompt_hex32(prompt: &str) -> Result<[u8; 32], Box<dyn std::error::Error>> {
    parse_hex32(&prompt_line(prompt)?)
}

fn prompt_hex16(prompt: &str) -> Result<[u8; 16], Box<dyn std::error::Error>> {
    parse_hex16(&prompt_line(prompt)?)
}

fn prompt_hex8(prompt: &str) -> Result<[u8; 8], Box<dyn std::error::Error>> {
    parse_hex8(&prompt_line(prompt)?)
}

fn prompt_line(prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
    print!("{prompt}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn uuid_from_hex(s: &str) -> Result<uuid::Uuid, Box<dyn std::error::Error>> {
    Ok(uuid::Uuid::from_bytes(parse_hex16(s)?))
}

fn parse_hex32(s: &str) -> Result<[u8; 32], Box<dyn std::error::Error>> {
    parse_hex_n::<32>(s)
}

fn parse_hex16(s: &str) -> Result<[u8; 16], Box<dyn std::error::Error>> {
    parse_hex_n::<16>(s)
}

fn parse_hex8(s: &str) -> Result<[u8; 8], Box<dyn std::error::Error>> {
    parse_hex_n::<8>(s)
}

fn parse_hex_n<const N: usize>(s: &str) -> Result<[u8; N], Box<dyn std::error::Error>> {
    let bytes = parse_hex(s)?;
    bytes
        .try_into()
        .map_err(|_| format!("expected {N} bytes of hex").into())
}

fn parse_hex(s: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return Err("odd-length hex string".into());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = hex_nibble(bytes[i])?;
        let lo = hex_nibble(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Result<u8, Box<dyn std::error::Error>> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err("invalid hex character".into()),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}
