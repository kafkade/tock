//! `tock account` — signup, login, logout, status.
//!
//! These commands run before the vault is opened (signup/login may *create*
//! it) so they live alongside `onboard` in the early dispatch. HTTP and
//! credential storage are the CLI's I/O edge; the wire orchestration comes
//! from the zero-I/O `tock-account` crate.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use tock_account::login::LoginStart;
use tock_account::signup::SignupMaterial;
use tock_account::{
    AccountCredentials, CredentialStore, FinishResponse, RegisterResponse, StartResponse,
};

use crate::http_transport::HttpTransport;

type CmdResult = Result<(), Box<dyn std::error::Error>>;

/// Response shape for the account-scoped vault-header fetch.
#[derive(serde::Deserialize)]
struct HeaderResp {
    header: String,
}

/// `tock account` argument root.
#[derive(Debug, clap::Args)]
pub struct AccountArgs {
    /// Account sub-action.
    #[command(subcommand)]
    pub cmd: AccountCmd,
}

/// Account sub-actions.
#[derive(Debug, Subcommand)]
pub enum AccountCmd {
    /// Create a new account + local vault, then print the Emergency Kit.
    Signup {
        /// Sync server base URL.
        #[arg(long)]
        server: String,
        /// Account email / login.
        #[arg(long)]
        email: String,
        /// Optional path to write a printable Emergency Kit PDF.
        #[arg(long)]
        kit_pdf: Option<PathBuf>,
    },
    /// Sign in on this device using password + Secret Key (or a Setup Code).
    Login {
        /// Sync server base URL (ignored when --setup-code is given).
        #[arg(long)]
        server: Option<String>,
        /// Account email (ignored when --setup-code is given).
        #[arg(long)]
        email: Option<String>,
        /// A `TOCK1:` Setup Code bundling server, email, and Secret Key.
        #[arg(long)]
        setup_code: Option<String>,
    },
    /// Forget stored credentials on this device.
    Logout,
    /// Show the current account + session status.
    Status,
}

/// Run a `tock account` subcommand.
///
/// # Errors
/// Propagates HTTP, crypto, storage, and credential-store failures.
pub fn run_account_cmd(cli: &crate::Cli, cmd: &AccountCmd) -> CmdResult {
    match cmd {
        AccountCmd::Signup {
            server,
            email,
            kit_pdf,
        } => signup(cli, server, email, kit_pdf.as_deref()),
        AccountCmd::Login {
            server,
            email,
            setup_code,
        } => login(
            cli,
            server.as_deref(),
            email.as_deref(),
            setup_code.as_deref(),
        ),
        AccountCmd::Logout => logout(),
        AccountCmd::Status => status(),
    }
}

fn password_bytes(cli: &crate::Cli) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    cli.password
        .as_deref()
        .map(|p| p.as_bytes().to_vec())
        .ok_or_else(|| "missing password: pass --password (it is never stored)".into())
}

fn signup(cli: &crate::Cli, server: &str, email: &str, kit_pdf: Option<&Path>) -> CmdResult {
    if cli.vault.exists() {
        return Err("vault already exists; use `tock account login` instead".into());
    }
    let password = password_bytes(cli)?;
    let (vault, secret_key) = tock_storage::init(&cli.vault, &password)?;
    let header = vault.header();
    let material =
        SignupMaterial::derive(email, &password_string(cli)?, &secret_key, header, server)?;
    let vault_id = header.vault_id;
    let header_bytes = header.to_bytes();

    let rt = tokio_runtime()?;
    rt.block_on(async {
        let http = reqwest::Client::new();
        let r = http
            .post(format!(
                "{}/v1/accounts/register",
                server.trim_end_matches('/')
            ))
            .json(&material.register_request)
            .send()
            .await?;
        if r.status() != reqwest::StatusCode::CREATED {
            return Err(format!("registration failed: {}", r.status()).into());
        }
        let _reg: RegisterResponse = r.json().await?;
        let session = srp_login(&http, server, email, &password_string(cli)?, &secret_key).await?;
        let transport = HttpTransport::new(server, vault_id)?.with_auth(
            session.bearer_token.clone(),
            session.channel_binding.clone(),
        );
        transport.put_vault_header(&header_bytes).await?;
        save_credentials(server, email, &vault_id, &secret_key, &session)?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;

    println!("Account created for {email} at {server}.");
    print!("{}", material.emergency_kit.render_text());
    println!(
        "Setup Code (other devices): {}",
        material.setup_code.encode()
    );
    println!("\n{}", render_qr(&material.setup_code.encode()));
    if let Some(path) = kit_pdf {
        write_kit_pdf(path, &material.emergency_kit.render_text())?;
        println!("Wrote printable Emergency Kit to {}", path.display());
    }
    Ok(())
}

fn login(
    cli: &crate::Cli,
    server: Option<&str>,
    email: Option<&str>,
    setup_code: Option<&str>,
) -> CmdResult {
    let (server, email, secret_key) = if let Some(code) = setup_code {
        let sc = tock_account::SetupCode::parse(code)?;
        let (_id, sk) = tock_crypto::SecretKey::parse(&sc.secret_key)?;
        (sc.server_url, sc.email, sk)
    } else {
        let server = server
            .ok_or("missing --server (or pass --setup-code)")?
            .to_string();
        let email = email
            .ok_or("missing --email (or pass --setup-code)")?
            .to_string();
        let sk_raw = cli.secret_key.as_deref().ok_or("missing --secret-key")?;
        let (_id, sk) = tock_crypto::SecretKey::parse(sk_raw)?;
        (server, email, sk)
    };
    let password = password_string(cli)?;

    let rt = tokio_runtime()?;
    rt.block_on(async {
        let http = reqwest::Client::new();
        let session = srp_login(&http, &server, &email, &password, &secret_key).await?;
        if cli.vault.exists() {
            let vault = tock_storage::open(&cli.vault, password.as_bytes(), &secret_key)?;
            save_credentials(
                &server,
                &email,
                &vault.header().vault_id,
                &secret_key,
                &session,
            )?;
        } else {
            // New device: fetch the non-secret header and materialise the vault.
            let header = fetch_vault_header(&http, &server, &session).await?;
            let parsed = tock_core::vault::VaultHeader::from_bytes(&header)?;
            tock_storage::init_from_header(
                &cli.vault,
                password.as_bytes(),
                &secret_key,
                &parsed,
                Some("cli"),
            )?;
            save_credentials(&server, &email, &parsed.vault_id, &secret_key, &session)?;
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    println!("Signed in as {email}. Run `tock sync` to pull your vault.");
    Ok(())
}

fn logout() -> CmdResult {
    KeyringStore.clear()?;
    let cfg = account_config_path()?;
    if cfg.exists() {
        std::fs::remove_file(&cfg)?;
    }
    println!("Logged out. Local vault file is unchanged.");
    Ok(())
}

fn status() -> CmdResult {
    match KeyringStore.load()? {
        None => println!("Not signed in. Use `tock account signup` or `tock account login`."),
        Some(c) => {
            let now = current_unix();
            println!("Signed in as {} @ {}", c.username, c.server_url);
            println!("Account id : {}", c.account_id);
            println!(
                "Session    : {}",
                if c.is_expired(now) {
                    "expired (re-login or sync to refresh)"
                } else {
                    "active"
                }
            );
        }
    }
    Ok(())
}

// ── Shared SRP login + header fetch ──────────────────────────────────

async fn srp_login(
    http: &reqwest::Client,
    server: &str,
    email: &str,
    password: &str,
    secret_key: &tock_crypto::SecretKey,
) -> Result<tock_account::SessionMaterial, Box<dyn std::error::Error>> {
    let base = server.trim_end_matches('/');
    let (start, start_req) = LoginStart::new(email)?;
    let sresp: StartResponse = http
        .post(format!("{base}/v1/auth/srp/start"))
        .json(&start_req)
        .send()
        .await?
        .json()
        .await?;
    let (pending, finish_req) = start.finish(&sresp, password, secret_key)?;
    let fresp = http
        .post(format!("{base}/v1/auth/srp/finish"))
        .json(&finish_req)
        .send()
        .await?;
    if fresp.status() != reqwest::StatusCode::OK {
        return Err("login failed: check email, password, and Secret Key".into());
    }
    let fresp: FinishResponse = fresp.json().await?;
    Ok(pending.verify(&fresp)?)
}

async fn fetch_vault_header(
    http: &reqwest::Client,
    server: &str,
    session: &tock_account::SessionMaterial,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // The vault id equals the account vault; the server resolves it from the
    // session's account, so the client lists it via the credential account.
    let base = server.trim_end_matches('/');
    let resp = http
        .get(format!("{base}/v1/account/header"))
        .header("authorization", format!("Bearer {}", session.bearer_token))
        .header("x-tock-channel-binding", &session.channel_binding)
        .send()
        .await?;
    if resp.status() != reqwest::StatusCode::OK {
        return Err("no vault header on server; sign in on the original device first".into());
    }
    let h: HeaderResp = resp.json().await?;
    Ok(tock_account::codec::base64_decode(&h.header)?)
}

// ── Credential storage ───────────────────────────────────────────────

/// Keyring-backed [`CredentialStore`] with a file fallback when no OS secret
/// service is available (headless servers, CI). The file lives beside the
/// account config with `0600` perms; it still omits the password.
pub struct KeyringStore;

const KEYRING_SERVICE: &str = "tock";
const KEYRING_USER: &str = "account-credentials";

fn creds_file() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(account_config_path()?.with_file_name("credentials.json"))
}

/// Whether to attempt the OS keyring. Disabled via `TOCK_NO_KEYRING=1` for
/// headless/multi-device tests that need per-config-dir credential isolation.
fn use_keyring() -> bool {
    std::env::var_os("TOCK_NO_KEYRING").is_none()
}

impl CredentialStore for KeyringStore {
    type Error = Box<dyn std::error::Error>;
    fn save(&self, creds: &AccountCredentials) -> Result<(), Self::Error> {
        let json = serde_json::to_string(creds)?;
        if use_keyring()
            && let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
            && entry.set_password(&json).is_ok()
        {
            return Ok(());
        }
        write_creds_file(&json)
    }
    fn load(&self) -> Result<Option<AccountCredentials>, Self::Error> {
        if use_keyring()
            && let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
            && let Ok(s) = entry.get_password()
        {
            return Ok(Some(serde_json::from_str(&s)?));
        }
        let path = creds_file()?;
        if path.exists() {
            return Ok(Some(serde_json::from_str(&std::fs::read_to_string(path)?)?));
        }
        Ok(None)
    }
    fn clear(&self) -> Result<(), Self::Error> {
        if use_keyring()
            && let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        {
            let _ = entry.delete_credential();
        }
        let path = creds_file()?;
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}

fn write_creds_file(json: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = creds_file()?;
    std::fs::write(&path, json)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn save_credentials(
    server: &str,
    email: &str,
    vault_id: &uuid::Uuid,
    secret_key: &tock_crypto::SecretKey,
    session: &tock_account::SessionMaterial,
) -> CmdResult {
    let creds = AccountCredentials {
        server_url: server.trim_end_matches('/').to_string(),
        username: email.to_string(),
        account_id: vault_id.to_string(),
        secret_key: secret_key.to_emergency_kit(vault_id.as_bytes()),
        bearer_token: session.bearer_token.clone(),
        channel_binding: session.channel_binding.clone(),
        expires_at: session.expires_at,
    };
    KeyringStore.save(&creds)?;
    write_account_config(server, email)?;
    Ok(())
}

fn account_config_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(account_config_dir()?.join("account.toml"))
}

/// The `…/tock` directory holding the account config and credential file.
///
/// Honors `XDG_CONFIG_HOME` on every platform for parity with the main config
/// resolver (`config::resolve_path`) — `dirs::config_dir()` alone ignores XDG
/// on Windows/macOS and can be `None` in restricted environments. Falls back
/// to the platform config dir when XDG is unset.
fn account_config_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .map_or_else(|| dirs::config_dir().ok_or("no config dir"), Ok)?;
    let dir = base.join("tock");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn write_account_config(server: &str, email: &str) -> CmdResult {
    let body = format!(
        "server_url = \"{}\"\nemail = \"{}\"\n",
        server.trim_end_matches('/'),
        email
    );
    std::fs::write(account_config_path()?, body)?;
    Ok(())
}

// ── Emergency Kit PDF + Setup-Code QR (presentation only) ────────────

fn render_qr(data: &str) -> String {
    use qrcode::QrCode;
    use qrcode::render::unicode;
    QrCode::new(data.as_bytes()).map_or_else(
        |_| String::new(),
        |code| code.render::<unicode::Dense1x2>().quiet_zone(true).build(),
    )
}

fn write_kit_pdf(path: &Path, text: &str) -> CmdResult {
    use printpdf::{
        BuiltinFont, Mm, Op, PdfDocument, PdfFontHandle, PdfPage, PdfSaveOptions, Point, Pt,
    };
    let font = PdfFontHandle::Builtin(BuiltinFont::Courier);
    let mut ops = vec![
        Op::StartTextSection,
        Op::SetFont {
            font,
            size: Pt(11.0),
        },
    ];
    let mut y = 280.0;
    for line in text.lines() {
        ops.push(Op::SetTextCursor {
            pos: Point::new(Mm(15.0), Mm(y)),
        });
        ops.push(Op::ShowText {
            items: vec![line.into()],
        });
        y -= 6.0;
    }
    ops.push(Op::EndTextSection);
    let page = PdfPage::new(Mm(210.0), Mm(297.0), ops);
    let mut doc = PdfDocument::new("tock Emergency Kit");
    doc.with_pages(vec![page]);
    let bytes = doc.save(&PdfSaveOptions::default(), &mut Vec::new());
    std::fs::write(path, bytes)?;
    Ok(())
}

fn tokio_runtime() -> Result<tokio::runtime::Runtime, Box<dyn std::error::Error>> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?)
}

fn current_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

fn password_string(cli: &crate::Cli) -> Result<String, Box<dyn std::error::Error>> {
    cli.password
        .clone()
        .ok_or_else(|| "missing password: pass --password (it is never stored)".into())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn render_qr_non_empty() {
        let qr = render_qr("TOCK1:abcdef");
        assert!(!qr.is_empty());
    }

    #[test]
    fn render_qr_empty_input_ok() {
        // Empty string still encodes to a small QR; just ensure no panic.
        let _ = render_qr("");
    }

    #[test]
    fn write_kit_pdf_produces_pdf() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("tock-kit-{}.pdf", current_unix()));
        write_kit_pdf(
            &path,
            "tock Emergency Kit\nemail: a@b.c\nSecret Key: A1-XXXX",
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
        let _ = std::fs::remove_file(&path);
    }
}
