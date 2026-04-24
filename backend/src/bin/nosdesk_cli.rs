//! Nosdesk administrative command-line tool.
//!
//! Intended for operations that shouldn't be web-reachable: account
//! recovery when an admin is locked out, plugin signing, and
//! similar server-side tasks. Most commands require `DATABASE_URL`
//! and the same encryption env vars as the backend. Commands are
//! typically run inside the backend container:
//!
//!     docker compose exec backend nosdesk-cli admin reset-password alice@example.com
//!
//! Subcommand groups planned but not yet implemented (keep this
//! list current so the shape stays predictable):
//!
//!   - `nosdesk-cli db backup|restore` — thin wrappers around
//!     pg_dump / pg_restore, with awareness of attachments + plugin
//!     bundles under /app/uploads.
//!   - `nosdesk-cli secrets generate jwt` — one-shot replacement
//!     for `openssl rand -base64 32`.
//!   - `nosdesk-cli secrets rotate encryption-key` — re-encrypt
//!     MFA secrets and the plugin local signing key under a new
//!     master key in lockstep (needs a careful migration story).
//!   - `nosdesk-cli admin unlock <email>` — clear the Redis-based
//!     login lockout keys. Needs `REDIS_URL`.
//!   - `nosdesk-cli registry install <plugin>` — once the Nosdesk
//!     registry ships, pull a signed zip from nosdesk.com and
//!     install it locally without going through the web UI.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use bcrypt::{hash, DEFAULT_COST};
use clap::{Parser, Subcommand};
use ring::signature::Ed25519KeyPair;

extern crate backend;

use backend::db;
use backend::repository::user_auth_identities;
use backend::repository::user_helpers;
use backend::repository::users as users_repo;
use backend::services::plugins::{install, signing, trust};

#[derive(Parser)]
#[command(
    name = "nosdesk-cli",
    about = "Nosdesk server administration and plugin tooling",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Plugin lifecycle: generate signing keys, sign archives,
    /// verify signatures, and install signed zips.
    #[command(subcommand)]
    Plugin(PluginCommand),

    /// Administrative overrides. Only use when an admin is locked
    /// out of the web UI; normal flows should go through it.
    #[command(subcommand)]
    Admin(AdminCommand),
}

// ---------------------------------------------------------------
// Plugin subcommands
// ---------------------------------------------------------------

#[derive(Subcommand)]
enum PluginCommand {
    /// Generate a fresh Ed25519 keypair for plugin signing. Writes
    /// `<prefix>.sk` (PKCS8 private key, 0600) and `<prefix>.pub`
    /// (base64 public key). No database access.
    GenKey {
        #[arg(
            long,
            short = 'o',
            value_name = "PREFIX",
            help = "Output prefix; writes <PREFIX>.sk and <PREFIX>.pub"
        )]
        out: PathBuf,
    },

    /// Sign a plugin source directory, producing a signed zip that
    /// can be installed. Reads every top-level file in <DIR> (the
    /// manifest, bundle, and any other assets), computes the
    /// canonical digest, and embeds an Ed25519 signature envelope.
    /// No database access.
    Sign {
        #[arg(value_name = "DIR", help = "Plugin source directory")]
        dir: PathBuf,
        #[arg(
            long,
            value_name = "SK",
            help = "Path to the PKCS8 private key (the .sk file from gen-key)"
        )]
        key: PathBuf,
        #[arg(long, short = 'o', value_name = "ZIP", help = "Output zip path")]
        out: PathBuf,
        #[arg(
            long,
            default_value = "local",
            help = "signer_source label stamped into the envelope"
        )]
        source: String,
    },

    /// Verify a signed zip. Checks the canonical digest and the
    /// signature against the pubkey in the envelope. Does NOT
    /// consult the DB trust chain — use `install` for that.
    Verify {
        #[arg(value_name = "ZIP", help = "Signed plugin zip")]
        zip: PathBuf,
    },

    /// Install a signed zip into this instance's database. Verifies
    /// the signature, resolves the signer against the trust chain
    /// (Nosdesk root / registered publishers / local signing key),
    /// and upserts the plugin. Requires DATABASE_URL and the
    /// encryption env.
    Install {
        #[arg(value_name = "ZIP", help = "Signed plugin zip")]
        zip: PathBuf,
    },
}

// ---------------------------------------------------------------
// Admin subcommands
// ---------------------------------------------------------------

#[derive(Subcommand)]
enum AdminCommand {
    /// Generate a strong random password, bcrypt it, and replace
    /// the user's local auth password. Prints the new password
    /// exactly once on stdout; pipe to a secret manager or hand it
    /// directly to the user. All of the user's sessions are
    /// revoked as a side-effect so the old credentials stop working
    /// everywhere.
    ResetPassword {
        #[arg(value_name = "EMAIL", help = "User email")]
        email: String,
    },

    /// Disable MFA for a user. Clears the TOTP secret and backup
    /// codes; the user can re-enrol on their next login. Use this
    /// when an admin has lost their second factor and can't get
    /// back in.
    ClearMfa {
        #[arg(value_name = "EMAIL", help = "User email")]
        email: String,
    },
}

fn main() -> ExitCode {
    // Best-effort .env load, same as the server binary. Missing
    // .env is fine — env vars may be supplied by docker compose.
    let _ = dotenv::dotenv();

    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Plugin(cmd) => run_plugin(cmd),
        Commands::Admin(cmd) => run_admin(cmd),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------
// Plugin handlers
// ---------------------------------------------------------------

fn run_plugin(cmd: PluginCommand) -> Result<()> {
    match cmd {
        PluginCommand::GenKey { out } => plugin_gen_key(&out),
        PluginCommand::Sign {
            dir,
            key,
            out,
            source,
        } => plugin_sign(&dir, &key, &out, &source),
        PluginCommand::Verify { zip } => plugin_verify(&zip),
        PluginCommand::Install { zip } => plugin_install(&zip),
    }
}

fn plugin_gen_key(prefix: &Path) -> Result<()> {
    let (pkcs8, pubkey) =
        signing::generate_keypair().map_err(|e| anyhow!("keypair generation failed: {e}"))?;

    let sk_path = prefix.with_extension("sk");
    let pub_path = prefix.with_extension("pub");

    if sk_path.exists() || pub_path.exists() {
        bail!(
            "refusing to overwrite existing key files: {} / {}",
            sk_path.display(),
            pub_path.display()
        );
    }

    if let Some(parent) = sk_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
    }

    write_private_key(&sk_path, &pkcs8)?;
    fs::write(&pub_path, BASE64.encode(&pubkey))
        .with_context(|| format!("writing {}", pub_path.display()))?;

    println!("wrote private key: {}", sk_path.display());
    println!("wrote public key:  {}", pub_path.display());
    println!("fingerprint:       {}", signing::fingerprint(&pubkey));
    Ok(())
}

fn plugin_sign(dir: &Path, key_path: &Path, out: &Path, source: &str) -> Result<()> {
    if !dir.is_dir() {
        bail!("{} is not a directory", dir.display());
    }
    if out.exists() {
        bail!("refusing to overwrite existing zip: {}", out.display());
    }

    let pkcs8 = fs::read(key_path)
        .with_context(|| format!("reading signing key {}", key_path.display()))?;
    let keypair = Ed25519KeyPair::from_pkcs8(&pkcs8)
        .map_err(|e| anyhow!("parsing signing key: {e:?}"))?;

    // Read the directory into ArchiveEntry values. Symlinks are
    // skipped (file_type() does not follow links); the same file
    // size caps as verification apply so a signer can't accidentally
    // build a zip that'll be refused at install time.
    let entries = read_signable_entries(dir)?;

    let envelope = signing::sign_entries(&entries, &keypair, source);

    // Embed the envelope as nosdesk-signature.json and zip the
    // whole set. The signing envelope is NOT covered by the
    // canonical digest (signing::SIGNATURE_FILE is filtered out),
    // but it IS stored inside the zip so verifiers can find it.
    let envelope_bytes = serde_json::to_vec_pretty(&envelope)
        .with_context(|| "serialising signature envelope")?;

    let zip_bytes = build_zip(&entries, &envelope_bytes)?;
    fs::write(out, zip_bytes).with_context(|| format!("writing {}", out.display()))?;

    println!("signed {} entries", entries.len());
    println!("signer pubkey:  {}", envelope.signer_pubkey);
    println!("fingerprint:    {}", signing::fingerprint(&base64_decode(&envelope.signer_pubkey)?));
    println!("signer source:  {}", envelope.signer_source);
    println!("wrote {}", out.display());
    Ok(())
}

fn plugin_verify(zip_path: &Path) -> Result<()> {
    let bytes = fs::read(zip_path).with_context(|| format!("reading {}", zip_path.display()))?;
    let verified = signing::verify_archive(&bytes).map_err(|e| anyhow!("{e}"))?;
    println!("signature is valid");
    println!("envelope version: {}", verified.envelope.version);
    println!("algorithm:        {}", verified.envelope.algorithm);
    println!("signer pubkey:    {}", verified.envelope.signer_pubkey);
    println!(
        "fingerprint:      {}",
        signing::fingerprint(&base64_decode(&verified.envelope.signer_pubkey)?)
    );
    println!("signer source:    {}", verified.envelope.signer_source);
    println!("signed_at:        {}", verified.envelope.signed_at);
    println!("digest:           {}", verified.envelope.signed_digest);
    println!("entries:          {}", verified.files.len());
    println!();
    println!(
        "note: this checks the signature primitive only, not the trust chain."
    );
    println!("use `nosdesk-cli plugin install` to resolve the pubkey against the DB.");
    Ok(())
}

fn plugin_install(zip_path: &Path) -> Result<()> {
    let bytes = fs::read(zip_path).with_context(|| format!("reading {}", zip_path.display()))?;
    if bytes.len() > signing::MAX_ARCHIVE_SIZE {
        bail!(
            "zip exceeds {} bytes",
            signing::MAX_ARCHIVE_SIZE
        );
    }

    let verified =
        signing::verify_archive(&bytes).map_err(|e| anyhow!("signature rejected: {e}"))?;

    let mut conn = connect_db()?;

    let tier = trust::resolve(&mut conn, &verified.envelope)
        .map_err(|e| anyhow!("publisher not trusted: {e}"))?;
    let signer = trust::PluginSignerFields::from_verified(&verified, &tier);

    let options = install::InstallOptions {
        source: "cli",
        installed_by: None,
        log_activity: true,
        provision_settings: false,
        skip_if_unchanged: false,
    };

    let outcome = install::install_verified(&mut conn, &verified.files, signer, options)
        .map_err(|e| anyhow!("install failed: {e}"))?;

    let (action, plugin) = match &outcome {
        install::InstallOutcome::Created(p) => ("installed", p),
        install::InstallOutcome::Updated(p) => ("updated", p),
        install::InstallOutcome::Unchanged(p) => ("unchanged", p),
    };
    println!(
        "{} plugin: {} v{} (trust={}, signer_source={})",
        action,
        plugin.name,
        plugin.version,
        plugin.trust_level,
        plugin.signer_source.as_deref().unwrap_or("-")
    );
    Ok(())
}

// ---------------------------------------------------------------
// Admin handlers
// ---------------------------------------------------------------

fn run_admin(cmd: AdminCommand) -> Result<()> {
    match cmd {
        AdminCommand::ResetPassword { email } => admin_reset_password(&email),
        AdminCommand::ClearMfa { email } => admin_clear_mfa(&email),
    }
}

fn admin_reset_password(email: &str) -> Result<()> {
    let mut conn = connect_db()?;

    let user = user_helpers::get_user_by_email(email, &mut conn)
        .map_err(|e| anyhow!("no user found for {email}: {e}"))?;

    let new_password = generate_password(20);
    let hashed = hash(&new_password, DEFAULT_COST).with_context(|| "hashing new password")?;

    let rows = user_auth_identities::update_local_password_hash(&mut conn, &user.uuid, &hashed)
        .with_context(|| "updating password hash")?;
    if rows == 0 {
        bail!(
            "user {} has no `local` auth identity. They may be SSO-only; use their identity provider to reset credentials.",
            email
        );
    }

    // Revoke sessions so the old credentials stop working
    // everywhere the user was logged in. Matches what the web
    // password-reset flow does.
    let revoked = backend::repository::active_sessions::revoke_other_sessions(
        &mut conn,
        &user.uuid,
        None,
    )
    .unwrap_or(0);

    println!("reset password for {} ({})", user.name, user.uuid);
    println!("revoked {revoked} active session(s)");
    println!();
    println!("new password (shown once):");
    println!("  {new_password}");
    Ok(())
}

fn admin_clear_mfa(email: &str) -> Result<()> {
    let mut conn = connect_db()?;

    let user = user_helpers::get_user_by_email(email, &mut conn)
        .map_err(|e| anyhow!("no user found for {email}: {e}"))?;

    let rows = users_repo::clear_user_mfa(&mut conn, &user.uuid)
        .with_context(|| "clearing MFA fields")?;
    if rows == 0 {
        bail!("user {email} not found (unexpected after lookup)");
    }

    println!("cleared MFA for {} ({})", user.name, user.uuid);
    println!("user will re-enrol on next login");
    Ok(())
}

// ---------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------

/// Open a pooled connection to the same database the server uses.
/// Shared by every subcommand that writes to Postgres; a single
/// helper keeps the error message consistent and makes the "does
/// this subcommand need DATABASE_URL?" question obvious at the
/// call site.
fn connect_db() -> Result<db::DbConnection> {
    let pool = db::establish_connection_pool();
    pool.get()
        .map_err(|e| anyhow!("database connection failed: {e}"))
}

fn write_private_key(path: &Path, pkcs8: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    f.write_all(pkcs8)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn read_signable_entries(dir: &Path) -> Result<Vec<signing::ArchiveEntry>> {
    let mut out = Vec::new();
    let mut total: u64 = 0;
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_file() {
            continue;
        }
        let name = entry
            .file_name()
            .to_str()
            .ok_or_else(|| anyhow!("non-UTF8 filename in {}", dir.display()))?
            .to_string();
        if name == signing::SIGNATURE_FILE {
            // A pre-existing signature envelope in the source dir is
            // almost certainly a mistake; canonical_digest filters
            // it out anyway, but refuse here so the signer can't
            // accidentally ship stale metadata.
            bail!(
                "source dir already contains {} — remove it before signing",
                signing::SIGNATURE_FILE
            );
        }
        let bytes = fs::read(entry.path()).with_context(|| format!("reading {name}"))?;
        if bytes.len() as u64 > signing::MAX_ENTRY_SIZE {
            bail!(
                "{name} exceeds per-entry size limit of {} bytes",
                signing::MAX_ENTRY_SIZE
            );
        }
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| anyhow!("total directory size overflow"))?;
        if total > signing::MAX_TOTAL_SIZE {
            bail!(
                "directory exceeds total size limit of {} bytes",
                signing::MAX_TOTAL_SIZE
            );
        }
        out.push(signing::ArchiveEntry { name, bytes });
    }
    if out.is_empty() {
        bail!("no files found to sign under {}", dir.display());
    }
    Ok(out)
}

fn build_zip(entries: &[signing::ArchiveEntry], envelope: &[u8]) -> Result<Vec<u8>> {
    use zip::write::FileOptions;
    let mut buf = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let options = FileOptions::default();
        for e in entries {
            zip.start_file(&e.name, options)?;
            zip.write_all(&e.bytes)?;
        }
        zip.start_file(signing::SIGNATURE_FILE, options)?;
        zip.write_all(envelope)?;
        zip.finish()?;
    }
    Ok(buf)
}

/// Cryptographically random password from an alphabet that excludes
/// visually-confusable characters (no 0/O, 1/l/I, etc). Long enough
/// that bcrypt's 72-byte cap isn't a concern.
fn generate_password(len: usize) -> String {
    use rand::seq::SliceRandom;
    const ALPHABET: &[u8] =
        b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789-_@#%!";
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| *ALPHABET.choose(&mut rng).unwrap() as char)
        .collect()
}

fn base64_decode(s: &str) -> Result<Vec<u8>> {
    BASE64
        .decode(s.as_bytes())
        .map_err(|e| anyhow!("base64 decode failed: {e}"))
}
