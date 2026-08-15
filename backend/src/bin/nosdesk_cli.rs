//! Nosdesk administrative command-line tool.
//!
//! Intended for operations that shouldn't be web-reachable: account
//! recovery when an admin is locked out, plugin signing, and
//! similar server-side tasks. Most commands require `DATABASE_URL`
//! and the same encryption env vars as the backend. Commands are
//! typically run inside the nosdesk container:
//!
//!     docker compose exec nosdesk nosdesk-cli admin reset-password alice@example.com
//!
//! Subcommand groups planned but not yet implemented (keep this
//! list current so the shape stays predictable):
//!
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
use backend::repository::passkey_credentials;
use backend::repository::user_auth_identities;
use backend::repository::user_helpers;
use backend::repository::users as users_repo;
use backend::services::admin_setup;
use backend::services::avatar_thumbnails;
use backend::services::backup as backup_service;
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

    /// Database operations. Lives here so restore is gated on
    /// shell access, not on a TCP race (see AUD-005).
    #[command(subcommand)]
    Db(DbCommand),

    /// Generate or transform secret material that the server
    /// consumes from env vars. No database access — these are
    /// pure transformations, safe to run on a developer laptop
    /// before any container exists.
    #[command(subcommand)]
    Secrets(SecretsCommand),

    /// Print the first-run setup token and onboarding URL. Handy
    /// after a detached `docker compose up -d`, where the startup
    /// banner isn't streamed to your terminal.
    SetupToken,

    /// Enterprise license operations. Signing requires the private
    /// signing key (held by Nosdesk only); the matching public key is
    /// compiled into the server, which verifies NOSDESK_LICENSE_KEY.
    #[command(subcommand)]
    License(LicenseCommand),
}

#[derive(Subcommand)]
enum LicenseCommand {
    /// Mint a signed license token. Generate the keypair once with:
    ///   openssl genpkey -algorithm ed25519 -out license_private.pem
    ///   openssl pkey -in license_private.pem -pubout -out license_pubkey.pem
    /// Commit/embed the public key; keep the private key offline.
    Sign {
        /// Path to the Ed25519 private key (PKCS8 PEM).
        #[arg(long)]
        key: PathBuf,
        /// Licensee (organisation the license is issued to).
        #[arg(long)]
        licensee: String,
        /// Maximum number of active workspaces the license permits.
        #[arg(long, default_value_t = 10)]
        max_workspaces: u32,
        /// Validity period in days from now.
        #[arg(long, default_value_t = 365)]
        days: i64,
        /// License id (jti). Defaults to a generated id.
        #[arg(long)]
        license_id: Option<String>,
    },
}

#[derive(Subcommand)]
enum SecretsCommand {
    /// Hash a password with bcrypt and print the resulting PHC
    /// string. Use the output as the value of
    /// `INITIAL_ADMIN_PASSWORD_HASH` for first-boot admin seeding,
    /// or as a one-off generator when you need a bcrypt hash
    /// elsewhere.
    ///
    /// Reads the password from a TTY prompt (no echo) by default;
    /// pass `--stdin` to read one line from stdin for scripting.
    BcryptHash {
        #[arg(
            long,
            help = "Read the password from stdin (one line). Use for scripts; omit for an interactive prompt."
        )]
        stdin: bool,
    },
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
    ///
    /// Signs with a key file (`--key`) OR the instance's own local
    /// signing key (`--local`). `--local` reads the DB (needs
    /// `DATABASE_URL` + the encryption env) and produces a zip that
    /// installs at the `local` tier on this instance; `--key` needs no
    /// database.
    Sign {
        #[arg(value_name = "DIR", help = "Plugin source directory")]
        dir: PathBuf,
        #[arg(
            long,
            value_name = "SK",
            conflicts_with = "local",
            required_unless_present = "local",
            help = "Path to the PKCS8 private key (the .sk file from gen-key)"
        )]
        key: Option<PathBuf>,
        #[arg(
            long,
            help = "Sign with this instance's own local signing key (installs at the `local` tier). Requires DATABASE_URL + the encryption env."
        )]
        local: bool,
        #[arg(long, short = 'o', value_name = "ZIP", help = "Output zip path")]
        out: PathBuf,
        #[arg(
            long,
            default_value = "local",
            help = "signer_source label stamped into the envelope (ignored with --local, which is always `local`)"
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
// Database subcommands
// ---------------------------------------------------------------

#[derive(Subcommand)]
enum DbCommand {
    /// Restore the database and uploaded files from a backup zip.
    /// Destructive: tables are replaced. Prompts unless --yes.
    /// Refuses on a non-empty target database unless --force.
    /// Requires DATABASE_URL and the encryption env.
    Restore {
        #[arg(value_name = "FILE")]
        file: PathBuf,
        #[arg(long, value_name = "PASSWORD", help = "Backup decryption password")]
        password: Option<String>,
        #[arg(long, value_name = "VAR", help = "Read password from this env var")]
        password_env: Option<String>,
        #[arg(long, short = 'y', help = "Skip the confirmation prompt")]
        yes: bool,
        #[arg(
            long,
            help = "Allow restore over a non-empty target database (replaces existing data)"
        )]
        force: bool,
        #[arg(
            long,
            help = "Restore even if the backup's schema hash doesn't match this build (use only if you've verified compatibility)"
        )]
        ignore_schema_mismatch: bool,
    },
}

// ---------------------------------------------------------------
// Admin subcommands
// ---------------------------------------------------------------

#[derive(Subcommand)]
enum AdminCommand {
    /// Create the initial administrator account. Refuses if any
    /// user already exists; use `reset-password` for recovery in
    /// that case. Reads the password from a TTY prompt by
    /// default (no echo), or from stdin if `--password-stdin` is
    /// passed. This is the HTTP-free alternative to the
    /// `/onboarding` web flow — appropriate when the operator
    /// can't reach the setup URL (Cloudflare Tunnel, headless
    /// CI provisioning, etc.).
    Create {
        #[arg(long, value_name = "NAME", help = "Administrator full name")]
        name: String,
        #[arg(long, value_name = "EMAIL", help = "Administrator email address")]
        email: String,
        #[arg(
            long,
            help = "Read the password from stdin (one line). Use for scripts; omit for an interactive prompt."
        )]
        password_stdin: bool,
    },

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

    /// Remove all of a user's passkeys. A registered passkey is a
    /// hard second-factor gate at login, and WebAuthn only works in a
    /// secure context (HTTPS or localhost) — so a passkey blocks login
    /// from a plain-HTTP LAN origin with no way to complete it. Clearing
    /// the passkeys drops that gate; the user re-enrols from a secure
    /// origin. Sessions are revoked as a side-effect. Admin accounts
    /// still owe a second factor, so the next login will require MFA
    /// setup (use an authenticator app over plain HTTP).
    ClearPasskeys {
        #[arg(value_name = "EMAIL", help = "User email")]
        email: String,
    },
}

fn main() -> ExitCode {
    // Best-effort .env load, same as the server binary. Missing
    // .env is fine — env vars may be supplied by docker compose.
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Plugin(cmd) => run_plugin(cmd),
        Commands::Admin(cmd) => run_admin(cmd),
        Commands::Db(cmd) => run_db(cmd),
        Commands::Secrets(cmd) => run_secrets(cmd),
        Commands::SetupToken => run_setup_token(),
        Commands::License(cmd) => run_license(cmd),
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
// License signing
// ---------------------------------------------------------------

fn run_license(cmd: LicenseCommand) -> Result<()> {
    match cmd {
        LicenseCommand::Sign {
            key,
            licensee,
            max_workspaces,
            days,
            license_id,
        } => license_sign(&key, &licensee, max_workspaces, days, license_id),
    }
}

fn license_sign(
    key_path: &Path,
    licensee: &str,
    max_workspaces: u32,
    days: i64,
    license_id: Option<String>,
) -> Result<()> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    let pem = fs::read_to_string(key_path)
        .with_context(|| format!("reading private key from {}", key_path.display()))?;
    let enc = EncodingKey::from_ed_pem(pem.as_bytes())
        .context("parsing Ed25519 private key (expected PKCS8 PEM)")?;

    let now = chrono::Utc::now().timestamp();
    let exp = now + days.max(1) * 86_400;
    let jti = license_id.unwrap_or_else(|| format!("lic_{}", uuid::Uuid::now_v7()));

    let claims = backend::license::LicenseClaims {
        iss: backend::license::LICENSE_ISSUER.to_string(),
        sub: licensee.to_string(),
        jti,
        iat: now,
        exp,
        max_workspaces,
    };

    let token = encode(&Header::new(Algorithm::EdDSA), &claims, &enc).context("signing license")?;

    // Stderr carries the human-readable summary; stdout is just the token so
    // it can be piped straight into an env file or secret store.
    eprintln!(
        "Signed license for {licensee}: max_workspaces={max_workspaces}, expires in {days} day(s)."
    );
    println!("{}{token}", backend::license::LICENSE_PREFIX);
    Ok(())
}

// ---------------------------------------------------------------
// Setup token
// ---------------------------------------------------------------

fn run_setup_token() -> Result<()> {
    match backend::utils::bootstrap_token::current_token_and_url() {
        Some((token, url)) => {
            println!("Setup token:  {token}");
            println!("Setup URL:    {url}");
            Ok(())
        }
        None => Err(anyhow::anyhow!(
            "no active setup token — this instance is already set up, or the token \
             expired. Restart the backend to mint a fresh one."
        )),
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
            local,
            out,
            source,
        } => plugin_sign(&dir, key.as_deref(), local, &out, &source),
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
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
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

fn plugin_sign(
    dir: &Path,
    key_path: Option<&Path>,
    local: bool,
    out: &Path,
    source: &str,
) -> Result<()> {
    if !dir.is_dir() {
        bail!("{} is not a directory", dir.display());
    }
    if out.exists() {
        bail!("refusing to overwrite existing zip: {}", out.display());
    }

    // Sign with the instance's own local key (`--local`) or a key file
    // (`--key`). clap enforces exactly one, but keep the source string honest:
    // `--local` always stamps `local` so the envelope matches the tier the
    // install path resolves it to.
    let (pkcs8, source): (Vec<u8>, &str) = if local {
        // The instance key is encrypted at rest under the master key, so the
        // keyring has to be initialised from the env first (the server binary
        // does this in main; the CLI does it lazily for the one command needing
        // it).
        backend::utils::encryption::init_keyring()
            .map_err(|e| anyhow!("initialising encryption keyring: {e}"))?;
        let mut conn = connect_db()?;
        let sk = backend::services::plugins::local_key::load_local_signing_pkcs8(&mut conn)
            .map_err(|e| anyhow!("loading instance local signing key: {e}"))?;
        (sk.to_vec(), "local")
    } else {
        let key_path = key_path.ok_or_else(|| anyhow!("--key or --local is required"))?;
        let sk = fs::read(key_path)
            .with_context(|| format!("reading signing key {}", key_path.display()))?;
        (sk, source)
    };
    let keypair =
        Ed25519KeyPair::from_pkcs8(&pkcs8).map_err(|e| anyhow!("parsing signing key: {e:?}"))?;

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
    let envelope_bytes =
        serde_json::to_vec_pretty(&envelope).with_context(|| "serialising signature envelope")?;

    let zip_bytes = build_zip(&entries, &envelope_bytes)?;
    fs::write(out, zip_bytes).with_context(|| format!("writing {}", out.display()))?;

    println!("signed {} entries", entries.len());
    println!("signer pubkey:  {}", envelope.signer_pubkey);
    println!(
        "fingerprint:    {}",
        signing::fingerprint(&base64_decode(&envelope.signer_pubkey)?)
    );
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
    println!("note: this checks the signature primitive only, not the trust chain.");
    println!("use `nosdesk-cli plugin install` to resolve the pubkey against the DB.");
    Ok(())
}

fn plugin_install(zip_path: &Path) -> Result<()> {
    let bytes = fs::read(zip_path).with_context(|| format!("reading {}", zip_path.display()))?;
    if bytes.len() > signing::MAX_ARCHIVE_SIZE {
        bail!("zip exceeds {} bytes", signing::MAX_ARCHIVE_SIZE);
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

    // Pin the bootstrap workspace: `plugins` is workspace-scoped with FORCE RLS
    // and its `workspace_id` defaults from `app.workspace_id`, so the INSERT
    // needs a workspace context to satisfy the RLS WITH CHECK (and the NOT NULL
    // default). `bootstrap` is the purpose-built helper for CLI-time install.
    let actor = backend::sync::actor::ActorContext::bootstrap("cli:plugin_install");
    let outcome = backend::sync::session::with_actor_context::<_, install::InstallError>(
        &mut conn,
        &actor,
        |conn| install::install_verified(conn, &verified.files, signer, tier, options),
    )
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
        AdminCommand::Create {
            name,
            email,
            password_stdin,
        } => admin_create(&name, &email, password_stdin),
        AdminCommand::ResetPassword { email } => admin_reset_password(&email),
        AdminCommand::ClearMfa { email } => admin_clear_mfa(&email),
        AdminCommand::ClearPasskeys { email } => admin_clear_passkeys(&email),
    }
}

fn admin_create(name: &str, email: &str, password_stdin: bool) -> Result<()> {
    let trimmed_name = name.trim();
    let trimmed_email = email.trim();
    if trimmed_name.is_empty() {
        bail!("--name is required and must not be blank");
    }
    if trimmed_name.len() > 255 {
        bail!("--name must be less than 255 characters");
    }
    if trimmed_email.is_empty() {
        bail!("--email is required and must not be blank");
    }
    if !trimmed_email.contains('@') || !trimmed_email.contains('.') {
        bail!("--email does not look like a valid address");
    }

    let password = if password_stdin {
        read_password_from_stdin()?
    } else {
        read_password_interactive()?
    };
    if password.len() < 8 {
        bail!("password must be at least 8 characters");
    }
    if password.len() > 128 {
        bail!("password must be less than 128 characters");
    }

    let hashed = hash(&password, DEFAULT_COST).with_context(|| "hashing password")?;

    let mut conn = connect_db()?;
    let (user, primary_email) = admin_setup::create_initial_admin(
        &mut conn,
        admin_setup::InitialAdminInput {
            name: trimmed_name,
            email: trimmed_email,
            password_hash: &hashed,
        },
    )
    .map_err(|e| match e {
        admin_setup::AdminSetupError::AlreadyComplete => anyhow!(
            "setup is already complete; use `nosdesk-cli admin reset-password` to recover credentials"
        ),
        admin_setup::AdminSetupError::DuplicateEmail => {
            anyhow!("email address already in use")
        }
        admin_setup::AdminSetupError::Db(db_err) => anyhow!("db error: {db_err:?}"),
    })?;

    // Shell access implies file access; clearing the bootstrap
    // token here makes the web setup endpoint inert immediately,
    // matching what the HTTP path does on its success branch.
    backend::utils::bootstrap_token::consume();

    println!(
        "created administrator: {} <{}>",
        user.name, primary_email.email
    );
    println!("uuid: {}", user.uuid);
    println!("the user can now log in via the web UI with the password you provided");
    Ok(())
}

// ---------------------------------------------------------------
// Secrets handlers
// ---------------------------------------------------------------

fn run_secrets(cmd: SecretsCommand) -> Result<()> {
    match cmd {
        SecretsCommand::BcryptHash { stdin } => secrets_bcrypt_hash(stdin),
    }
}

fn secrets_bcrypt_hash(from_stdin: bool) -> Result<()> {
    let password = if from_stdin {
        read_password_from_stdin()?
    } else {
        read_password_interactive()?
    };
    if password.is_empty() {
        bail!("password must not be empty");
    }
    let hashed = hash(&password, DEFAULT_COST).with_context(|| "hashing password")?;
    // Print to stdout exactly, no trailing newline-after-prefix
    // noise; this output is meant to be captured into env files
    // or pasted into a Kubernetes Secret.
    println!("{hashed}");
    Ok(())
}

fn read_password_interactive() -> Result<String> {
    let pw =
        rpassword::prompt_password("Password: ").with_context(|| "reading password from TTY")?;
    let confirm = rpassword::prompt_password("Confirm password: ")
        .with_context(|| "reading password confirmation from TTY")?;
    if pw != confirm {
        bail!("passwords do not match");
    }
    Ok(pw)
}

fn read_password_from_stdin() -> Result<String> {
    use std::io::Read as _;
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .with_context(|| "reading password from stdin")?;
    // Trim the trailing newline that pipes typically add. Don't
    // trim leading/internal whitespace — a leading space could be
    // intentional, however unusual.
    Ok(buf.trim_end_matches(['\n', '\r']).to_string())
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
    let revoked =
        backend::repository::active_sessions::revoke_other_sessions(&mut conn, &user.uuid, None)
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

    // Wrap the audited `users` write in a bootstrap actor context so
    // the audit trigger has a workspace pin (self-hosted == workspace 1).
    let rows = backend::sync::session::with_actor_context(
        &mut conn,
        &backend::sync::actor::ActorContext::bootstrap("cli:admin_clear_mfa"),
        |c| users_repo::clear_user_mfa(c, &user.uuid),
    )
    .with_context(|| "clearing MFA fields")?;
    if rows == 0 {
        bail!("user {email} not found (unexpected after lookup)");
    }

    println!("cleared MFA for {} ({})", user.name, user.uuid);
    println!("user will re-enrol on next login");
    Ok(())
}

fn admin_clear_passkeys(email: &str) -> Result<()> {
    let mut conn = connect_db()?;

    let user = user_helpers::get_user_by_email(email, &mut conn)
        .map_err(|e| anyhow!("no user found for {email}: {e}"))?;

    let removed = passkey_credentials::delete_all_for_user(&mut conn, &user.uuid)
        .with_context(|| "deleting passkey credentials")?;

    // Revoke sessions so a stale session can't keep the (now removed)
    // passkey association alive; matches reset-password's behaviour.
    let revoked =
        backend::repository::active_sessions::revoke_other_sessions(&mut conn, &user.uuid, None)
            .unwrap_or(0);

    println!(
        "removed {removed} passkey(s) for {} ({})",
        user.name, user.uuid
    );
    println!("revoked {revoked} active session(s)");
    if removed == 0 {
        println!("(the user had no passkeys registered)");
    } else {
        println!("the user can re-enrol a passkey from a secure origin (HTTPS or localhost)");
    }
    Ok(())
}

// ---------------------------------------------------------------
// Database handlers
// ---------------------------------------------------------------

fn run_db(cmd: DbCommand) -> Result<()> {
    match cmd {
        DbCommand::Restore {
            file,
            password,
            password_env,
            yes,
            force,
            ignore_schema_mismatch,
        } => db_restore(
            &file,
            password,
            password_env,
            yes,
            force,
            ignore_schema_mismatch,
        ),
    }
}

fn db_restore(
    file: &Path,
    password: Option<String>,
    password_env: Option<String>,
    yes: bool,
    force: bool,
    ignore_schema_mismatch: bool,
) -> Result<()> {
    if !file.exists() {
        bail!("backup file not found: {}", file.display());
    }

    let password = resolve_password(password, password_env)?;

    // preview_restore now drives both metadata-read AND password
    // verification — a successful preview means the archive
    // header parsed and (if encrypted) the password decrypted
    // the inner zip.
    let preview = backup_service::preview_restore(file, password.as_deref())
        .map_err(|e| anyhow!("preview failed: {e}"))?;

    let manifest = &preview.manifest;
    println!("Restore preview:");
    println!("  source:       {}", file.display());
    println!("  created:      {}", manifest.created_at);
    println!("  version:      {}", manifest.nosdesk_version);
    println!(
        "  files:        {} ({} bytes)",
        manifest.files.total_count, manifest.files.total_size_bytes
    );
    let mut tables: Vec<_> = manifest.tables.iter().collect();
    tables.sort_by_key(|(name, _)| name.as_str());
    println!("  tables:");
    for (name, info) in tables {
        println!("    - {name}: {} rows", info.count);
    }
    for warning in &preview.warnings {
        eprintln!("  warning: {warning}");
    }

    if !yes && !confirm_destructive("Replace all matching tables with the backup contents?")? {
        eprintln!("aborted");
        return Ok(());
    }

    let mut conn = connect_db()?;
    let stats = backup_service::restore_database(
        &mut conn,
        file,
        password.as_deref(),
        backup_service::RestoreOptions {
            force_non_empty: force,
            ignore_schema_mismatch,
        },
    )
    .map_err(|e| anyhow!("database restore failed: {e}"))?;
    let files_restored = backup_service::restore_backup_files(file, password.as_deref())
        .map_err(|e| anyhow!("file restore failed: {e}"))?;

    // Thumbnails aren't part of the backup payload (skipped as cheap to
    // regenerate), so rebuild them from the restored avatar originals.
    // This mirrors the admin HTTP restore path; without it a CLI restore
    // leaves every profile thumbnail missing. The image pipeline is
    // async, so drive it on a one-shot current-thread runtime.
    let thumb_stats = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build runtime for thumbnail backfill")?
        .block_on(avatar_thumbnails::backfill_thumbnails(
            &mut conn,
            avatar_thumbnails::BackfillMode::Force,
            "cli:db_restore",
        ));

    println!();
    println!("Restore complete:");
    println!("  tables restored:  {}", stats.tables_restored);
    println!("  records restored: {}", stats.records_restored);
    println!("  files restored:   {}", files_restored);
    println!(
        "  thumbnails:       {} regenerated, {} failed",
        thumb_stats.regenerated, thumb_stats.failed
    );
    if !stats.per_table.is_empty() {
        println!();
        println!("Per-table breakdown:");
        for r in &stats.per_table {
            if r.rows_attempted > 0 || r.rows_loaded > 0 {
                println!(
                    "  {:<32} {:>6} loaded / {:>6} attempted",
                    r.table, r.rows_loaded, r.rows_attempted
                );
            }
        }
    }
    Ok(())
}

fn resolve_password(
    password: Option<String>,
    password_env: Option<String>,
) -> Result<Option<String>> {
    match (password, password_env) {
        (Some(_), Some(_)) => bail!("pass --password or --password-env, not both"),
        (Some(p), None) => Ok(Some(p)),
        (None, Some(var)) => {
            Ok(Some(std::env::var(&var).map_err(|_| {
                anyhow!("environment variable {var} is not set")
            })?))
        }
        (None, None) => Ok(None),
    }
}

fn confirm_destructive(prompt: &str) -> Result<bool> {
    use std::io::{BufRead, Write as _};
    print!("{prompt} Type 'yes' to continue: ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .with_context(|| "reading confirmation")?;
    Ok(line.trim().eq_ignore_ascii_case("yes"))
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
    use zip::write::SimpleFileOptions;
    let mut buf = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
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
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789-_@#%!";
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
