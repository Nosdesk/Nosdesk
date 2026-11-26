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
//! On Cloud, run them on the product machine (`fly ssh console`).
//!
//! Command groups:
//!
//!   - `admin`: create the first administrator, reset a password,
//!     clear MFA or passkeys.
//!   - `db restore`: restore a whole-instance backup.
//!   - `workspace`: export a workspace to a file, import an export
//!     into an untouched workspace (`--dry-run` previews it), and
//!     check, retry the files of, or reset an import
//!     (`import-status`, `import-retry-files`, `import-reset`).
//!     Imports run as workspace import jobs, as the app's do.
//!   - `plugin`: signing keys, signing, verifying and installing.
//!   - `secrets`, `license`, `setup-token`.
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
use backend::services::workspace_transfer as transfer;

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

    /// Export a workspace to a file, or import an export into a
    /// workspace. Needs DATABASE_URL and the storage env, as the
    /// server does.
    #[command(subcommand)]
    Workspace(WorkspaceCommand),

    /// Print the first-run setup token and onboarding URL. Handy
    /// after a detached `docker compose up -d`, where the startup
    /// banner isn't streamed to your terminal.
    SetupToken,

    /// Enterprise license operations. Signing requires the private
    /// signing key (held by Nosdesk only); the matching public key is
    /// compiled into the server, which verifies NOSDESK_LICENSE_KEY.
    #[command(subcommand)]
    License(LicenseCommand),

    /// Bring another helpdesk's tickets, conversations, people and files
    /// into a workspace. Needs DATABASE_URL and the storage env, as the
    /// server does.
    #[command(subcommand)]
    Import(ImportCommand),
}

#[derive(Subcommand)]
enum ImportCommand {
    /// Import from another helpdesk. Fetches into a bundle directory
    /// (resumable), prints what the import will do, asks, then writes in
    /// batches. Imported history notifies nobody and fires no rules or
    /// webhooks. Safe to run again: it resumes, and picks up new replies.
    Helpdesk(HelpdeskImportArgs),
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum HelpdeskSourceArg {
    Zendesk,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum NumbersArg {
    /// Keep the source's numbers when the workspace has none of its own.
    Auto,
    /// Keep them, or refuse.
    Keep,
    /// Number after the workspace's existing tickets.
    After,
}

#[derive(clap::Args, Debug)]
struct HelpdeskImportArgs {
    #[arg(value_enum)]
    source: HelpdeskSourceArg,

    /// The workspace to import into: its id or slug.
    #[arg(long)]
    workspace: String,

    /// Email of the workspace owner or admin the import runs as. Imported
    /// rows are recorded as written by them.
    #[arg(long = "as", value_name = "EMAIL")]
    run_as: String,

    /// The Zendesk subdomain (`acme` for acme.zendesk.com).
    #[arg(long)]
    subdomain: String,

    /// Environment variable holding a Zendesk OAuth access token.
    #[arg(long, value_name = "VAR", conflicts_with = "api_token_env")]
    oauth_token_env: Option<String>,

    /// Environment variable holding a legacy Zendesk API token (with
    /// --email). Zendesk revokes these on 2027-04-30.
    #[arg(long, value_name = "VAR", requires = "email")]
    api_token_env: Option<String>,

    /// The Zendesk admin email the API token belongs to.
    #[arg(long)]
    email: Option<String>,

    /// Read a Zendesk JSON data export instead of the API. A credential is
    /// optional; without one, custom field titles and sign-in-only files
    /// can't be fetched.
    #[arg(long, value_name = "ZIP")]
    export_zip: Option<PathBuf>,

    /// Where the fetched copy is kept. Reused (and a fetch resumed) when it
    /// exists. Defaults to ./helpdesk-import-<source>-<subdomain>.
    #[arg(long, value_name = "DIR")]
    bundle: Option<PathBuf>,

    /// Fetch again from the start even if the bundle is complete, to pick
    /// up activity since the last fetch.
    #[arg(long)]
    refetch: bool,

    #[arg(long, value_enum, default_value = "auto")]
    numbers: NumbersArg,

    /// Map a source agent to a member here: `zendesk-email=nosdesk-email`.
    /// Repeatable. On Cloud, unmapped agents become former staff.
    #[arg(long = "map-agent", value_name = "FROM=TO")]
    map_agent: Vec<String>,

    /// Create unmatched, active source agents as agents (self-hosted only).
    #[arg(long)]
    create_agents: bool,

    /// Import even though the tickets name people the bundle doesn't hold
    /// (a tickets-only export). The tickets they requested and the replies
    /// they wrote are credited to the placeholder author for good, tickets
    /// assigned to them are unassigned, their watching is dropped, and each
    /// ticket's import note names its requester and assignee by Zendesk id.
    /// Not needed for people a fetch from the API found gone at Zendesk.
    #[arg(long)]
    allow_missing_people: bool,

    /// Fetch and print the plan; write nothing to the workspace.
    #[arg(long)]
    dry_run: bool,

    /// Don't ask before writing.
    #[arg(long)]
    yes: bool,

    /// Point at a Zendesk stand-in (a local mock). Its host must be in
    /// NOSDESK_OUTBOUND_ALLOWED_HOSTS.
    #[arg(long, hide = true)]
    base_url: Option<String>,
}

#[derive(Subcommand)]
enum LicenseCommand {
    /// Mint a signed license token. Generate the keypair once with:
    ///   openssl genpkey -algorithm ed25519 -out license_private.pem
    ///   openssl pkey -in license_private.pem -pubout -out license_keys/<name>.pem
    /// Commit/embed the public key; keep the private key offline.
    Sign {
        /// Path to the Ed25519 private key (PKCS8 PEM).
        #[arg(long)]
        key: PathBuf,
        /// Stable customer id (UUID). Required; reissues MUST pass the same
        /// id or the usage meter starts over. Never defaulted.
        #[arg(long)]
        customer_id: String,
        /// Display name of the licensee organisation.
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
        /// Feature keys to stamp (comma-separated). Unknown keys are
        /// accepted at mint and dropped at verify against the binary's
        /// known-key set. Empty in v1.1.
        #[arg(long, value_delimiter = ',')]
        features: Vec<String>,
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
    /// Requires DATABASE_URL and the encryption env. A backup from an
    /// earlier version is upgraded as it restores, which needs a superuser
    /// role, as a fresh install does.
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
    },
}

// ---------------------------------------------------------------
// Workspace subcommands
// ---------------------------------------------------------------

#[derive(Subcommand)]
enum WorkspaceCommand {
    /// Export a workspace to a file: its rows, files, people's
    /// avatars and branding. With a password (`--seal` prompts for
    /// one) the archive is encrypted and also carries the secret
    /// columns a plaintext zip leaves out. Either way, credentials
    /// encrypted under this server's keys (integration passwords,
    /// plugin secrets) don't decrypt elsewhere: an import asks for
    /// them again. No size cap: the archive is built in memory, so
    /// allow about its size free.
    Export {
        #[arg(value_name = "WORKSPACE", help = "Workspace id or slug")]
        workspace: String,
        #[arg(
            long,
            short = 'o',
            value_name = "FILE",
            help = "Where to write the export"
        )]
        out: PathBuf,
        #[arg(long, help = "Seal the export with a password entered at a prompt")]
        seal: bool,
        #[arg(
            long,
            conflicts_with_all = ["seal", "password_env"],
            help = "Seal with a password read from stdin (one line)"
        )]
        password_stdin: bool,
        #[arg(
            long,
            value_name = "VAR",
            conflicts_with = "seal",
            help = "Seal with the password in this env var"
        )]
        password_env: Option<String>,
    },

    /// Import an export into an untouched workspace, through a
    /// workspace import job (it shows in Recent imports and resets
    /// like one started in the app). Prints the preview, asks to
    /// continue (unless --yes), follows the import and prints the
    /// report. Refuses a workspace that holds data, an export from
    /// another Nosdesk version, and a workspace with an import under
    /// way. A sealed export asks for its password. The archive is
    /// read into memory (about three times its size while it is
    /// unsealed and parsed) beside the running server.
    Import {
        #[arg(value_name = "FILE", help = "The workspace export (.nosdesk)")]
        archive: PathBuf,
        #[arg(
            long,
            value_name = "WORKSPACE",
            help = "Workspace id or slug to import into"
        )]
        into: String,
        #[arg(long, help = "Print the preview and stop, writing nothing")]
        dry_run: bool,
        #[arg(long, short = 'y', help = "Skip the confirmation prompt")]
        yes: bool,
        #[arg(
            long,
            help = "Give every imported row a new uuid: for a copy of a workspace this server still holds"
        )]
        regenerate_uuids: bool,
        #[arg(
            long,
            conflicts_with = "password_env",
            help = "Read a sealed export's password from stdin (one line)"
        )]
        password_stdin: bool,
        #[arg(
            long,
            value_name = "VAR",
            help = "Read a sealed export's password from this env var"
        )]
        password_env: Option<String>,
    },

    /// Show a workspace import: its state, and once it imported, the
    /// rows, the people it created and the files it skipped.
    ImportStatus {
        #[arg(value_name = "JOB", help = "Import id")]
        job: uuid::Uuid,
    },

    /// Restore the files of an import whose rows imported and whose
    /// files stopped part way. Files already in place are left alone.
    ImportRetryFiles {
        #[arg(value_name = "JOB", help = "Import id")]
        job: uuid::Uuid,
        #[arg(
            long,
            conflicts_with = "password_env",
            help = "Read a sealed export's password from stdin (one line)"
        )]
        password_stdin: bool,
        #[arg(
            long,
            value_name = "VAR",
            help = "Read a sealed export's password from this env var"
        )]
        password_env: Option<String>,
    },

    /// Reset a workspace import. One that imported takes the
    /// workspace back to how it was provisioned and removes the
    /// people it created; one that didn't is discarded. Prompts
    /// unless --yes.
    ImportReset {
        #[arg(value_name = "JOB", help = "Import id")]
        job: uuid::Uuid,
        #[arg(long, short = 'y', help = "Skip the confirmation prompt")]
        yes: bool,
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

    /// Replace the user's local auth password with one you enter:
    /// from a TTY prompt by default (no echo), or one line from
    /// stdin with `--password-stdin`. Nothing is printed, so the
    /// password never lands in terminal scrollback or a job log.
    /// All of the user's sessions are revoked as a side-effect so
    /// the old credentials stop working everywhere.
    ResetPassword {
        #[arg(value_name = "EMAIL", help = "User email")]
        email: String,
        #[arg(
            long,
            help = "Read the new password from stdin (one line). Use for scripts; omit for an interactive prompt."
        )]
        password_stdin: bool,
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
        Commands::Workspace(cmd) => run_workspace(cmd),
        Commands::SetupToken => run_setup_token(),
        Commands::License(cmd) => run_license(cmd),
        Commands::Import(ImportCommand::Helpdesk(args)) => run_helpdesk_import(args),
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
            customer_id,
            licensee,
            max_workspaces,
            days,
            license_id,
            features,
        } => license_sign(
            &key,
            &customer_id,
            &licensee,
            max_workspaces,
            days,
            license_id,
            features,
        ),
    }
}

fn license_sign(
    key_path: &Path,
    customer_id: &str,
    licensee: &str,
    max_workspaces: u32,
    days: i64,
    license_id: Option<String>,
    features: Vec<String>,
) -> Result<()> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    let customer_id = uuid::Uuid::parse_str(customer_id.trim()).with_context(|| {
        format!(
            "--customer-id must be a UUID (got {customer_id:?}); \
             reissues must pass the same id, never omit it"
        )
    })?;
    if licensee.trim().is_empty() {
        bail!("--licensee must not be empty");
    }

    let pem = fs::read_to_string(key_path)
        .with_context(|| format!("reading private key from {}", key_path.display()))?;
    let enc = EncodingKey::from_ed_pem(pem.as_bytes())
        .context("parsing Ed25519 private key (expected PKCS8 PEM)")?;

    let now = chrono::Utc::now().timestamp();
    let exp = now + days.max(1) * 86_400;
    let jti = license_id.unwrap_or_else(|| format!("lic_{}", uuid::Uuid::now_v7()));

    let claims = backend::license::LicenseClaims {
        iss: backend::license::LICENSE_ISSUER.to_string(),
        sub: customer_id.to_string(),
        licensee: licensee.trim().to_string(),
        jti,
        iat: now,
        exp,
        max_workspaces,
        features,
    };

    let token = encode(&Header::new(Algorithm::EdDSA), &claims, &enc).context("signing license")?;

    // Stderr carries the human-readable summary; stdout is just the token so
    // it can be piped straight into an env file or secret store.
    eprintln!(
        "Signed license for {licensee} (customer_id={customer_id}): \
         max_workspaces={max_workspaces}, expires in {days} day(s)."
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
        AdminCommand::ResetPassword {
            email,
            password_stdin,
        } => admin_reset_password(&email, password_stdin),
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

    let password = read_new_password(password_stdin)?;

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

/// A new account password, from stdin or the TTY prompt, held to the
/// same 8 to 128 characters as the web forms.
fn read_new_password(password_stdin: bool) -> Result<String> {
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
    Ok(password)
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

fn admin_reset_password(email: &str, password_stdin: bool) -> Result<()> {
    let mut conn = connect_db()?;

    let user = user_helpers::get_user_by_email(email, &mut conn)
        .map_err(|e| anyhow!("no user found for {email}: {e}"))?;

    let new_password = read_new_password(password_stdin)?;
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
    let revoked = backend::repository::active_sessions::revoke_all_sessions(&mut conn, &user.uuid)
        .unwrap_or(0);

    println!("reset password for {} ({})", user.name, user.uuid);
    println!("revoked {revoked} active session(s)");
    println!("the user can now log in via the web UI with the password you provided");
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
    let revoked = backend::repository::active_sessions::revoke_all_sessions(&mut conn, &user.uuid)
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
        } => db_restore(&file, password, password_env, yes, force),
    }
}

fn db_restore(
    file: &Path,
    password: Option<String>,
    password_env: Option<String>,
    yes: bool,
    force: bool,
) -> Result<()> {
    if !file.exists() {
        bail!("backup file not found: {}", file.display());
    }

    let password = resolve_password(password, password_env)?;

    // preview_restore now drives both metadata-read AND password
    // verification — a successful preview means the archive
    // header parsed and (if encrypted) the password decrypted
    // the inner zip.
    let mut preview = backup_service::preview_restore(file, password.as_deref())
        .map_err(|e| anyhow!("preview failed: {e}"))?;
    let pool = db::establish_connection_pool();
    backup_service::list_replaced_tables(&pool, &mut preview);

    let manifest = &preview.manifest;
    println!("Restore preview:");
    println!("  source:       {}", file.display());
    println!("  created:      {}", manifest.created_at);
    println!("  version:      {}", manifest.nosdesk_version);
    if let Some(upgrade) = &preview.upgrade {
        println!(
            "  upgrade:      from {} to {}, as the backup restores",
            upgrade.from_version, upgrade.to_version
        );
    }
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
    if let Some(upgrade) = preview
        .upgrade
        .as_ref()
        .filter(|u| !u.replaced_tables.is_empty())
    {
        eprintln!(
            "  warning: the backup predates these tables; their current rows are replaced \
             (settings return to their defaults): {}",
            upgrade.replaced_tables.join(", ")
        );
    }
    if !preview.encrypted {
        eprintln!(
            "  warning: this backup holds no passwords or MFA; restoring it clears them for \
             everyone. Set a new one afterwards with `nosdesk-cli admin reset-password`."
        );
    }
    for warning in &preview.warnings {
        eprintln!("  warning: {warning}");
    }

    if !yes && !confirm_destructive("Replace all matching tables with the backup contents?")? {
        eprintln!("aborted");
        return Ok(());
    }

    // Runs as the migration role when MIGRATION_DATABASE_URL is set, like
    // the admin restore: the app role can't truncate and reload tables.
    let (stats, mut conn) = backup_service::restore_instance(
        &pool,
        file,
        password.as_deref(),
        backup_service::RestoreOptions {
            force_non_empty: force,
            ..Default::default()
        },
    )
    .map_err(|e| match e.connection_cause() {
        // This is the operator's own terminal, so the cause is shown.
        Some(cause) => anyhow!("database restore failed: {e}: {cause}"),
        None => anyhow!("database restore failed: {e}"),
    })?;
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

// ---------------------------------------------------------------
// Helpdesk import
// ---------------------------------------------------------------

fn run_helpdesk_import(args: HelpdeskImportArgs) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build runtime")?;
    runtime.block_on(helpdesk_import(args))
}

/// `FROM=TO` pairs, lower-cased.
fn parse_agent_map(pairs: &[String]) -> Result<std::collections::HashMap<String, String>> {
    pairs
        .iter()
        .map(|p| {
            let (from, to) = p
                .split_once('=')
                .ok_or_else(|| anyhow!("--map-agent takes FROM=TO, got {p:?}"))?;
            let (from, to) = (from.trim().to_lowercase(), to.trim().to_lowercase());
            if from.is_empty() || to.is_empty() {
                bail!("--map-agent takes FROM=TO, got {p:?}");
            }
            Ok((from, to))
        })
        .collect()
}

/// The owner or admin of `workspace_id` whose email is `email`: the import
/// runs as them.
async fn resolve_import_actor(
    pool: &db::Pool,
    workspace_id: i32,
    email: &str,
) -> Result<uuid::Uuid> {
    use backend::repository::user_helpers::PrimaryEmailHolder;
    let email = email.trim().to_lowercase();
    let (pool, key) = (pool.clone(), email.clone());
    let holders = tokio::task::spawn_blocking(move || {
        backend::sync::session::run_in_workspace(&pool, "cli:import_helpdesk", workspace_id, |c| {
            user_helpers::primary_email_holders(c, workspace_id, &[key])
        })
    })
    .await
    .context("worker")?
    .map_err(|e| anyhow!("{e}"))?;
    match holders.get(&email) {
        Some(PrimaryEmailHolder::Member { user, role }) if role == "owner" || role == "admin" => {
            Ok(user.uuid)
        }
        Some(PrimaryEmailHolder::Member { role, .. }) => {
            bail!("{email} is a {role} of this workspace; the import must run as an owner or admin")
        }
        _ => bail!("{email} isn't a member of this workspace"),
    }
}

async fn helpdesk_import(args: HelpdeskImportArgs) -> Result<()> {
    use backend::services::imports::helpdesk::bundle::{self, Bundle, FetchState};
    use backend::services::imports::helpdesk::loader::{self, LoadOptions, NumberPolicy};
    use backend::services::imports::helpdesk::report;
    use backend::services::imports::helpdesk::source::{FetchProgress, HelpdeskSource};
    use backend::services::imports::helpdesk::zendesk::{self, ZendeskAuth, ZendeskSource};
    use std::io::IsTerminal;
    use tokio_util::sync::CancellationToken;

    let HelpdeskSourceArg::Zendesk = args.source;
    if !args.dry_run && !args.yes && !std::io::stdin().is_terminal() {
        bail!("no terminal to confirm on: pass --yes, or --dry-run to preview");
    }

    let pool = db::establish_connection_pool();
    let storage = backend::utils::storage::process_storage();
    let ws = transfer::resolve_workspace(&pool, &args.workspace).await?;
    let actor = resolve_import_actor(&pool, ws.id, &args.run_as).await?;
    let agent_map = parse_agent_map(&args.map_agent)?;

    let env = |var: &str| {
        std::env::var(var).map_err(|_| anyhow!("environment variable {var} is not set"))
    };
    let auth = match (&args.oauth_token_env, &args.api_token_env) {
        (Some(var), _) => Some(ZendeskAuth::Bearer(env(var)?)),
        (None, Some(var)) => Some(ZendeskAuth::Basic {
            email: args.email.clone().unwrap_or_default(),
            token: env(var)?,
        }),
        (None, None) => None,
    };
    if auth.is_none() && args.export_zip.is_none() {
        bail!("give --oauth-token-env, --api-token-env with --email, or --export-zip");
    }
    let mut source = match (&args.base_url, auth) {
        (Some(url), Some(a)) => ZendeskSource::with_base_url(url, a)?,
        (Some(url), None) => ZendeskSource::for_export_with_base_url(url)?,
        (None, Some(a)) => ZendeskSource::new(&args.subdomain, a)?,
        (None, None) => ZendeskSource::for_export(&args.subdomain)?,
    };

    let dir = args.bundle.clone().unwrap_or_else(|| {
        PathBuf::from(format!(
            "helpdesk-import-zendesk-{}",
            args.subdomain.trim().to_lowercase()
        ))
    });
    // A copy of a customer's helpdesk: created readable by its owner only.
    let bundle = Bundle::create(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let complete = bundle.read_manifest().is_ok();

    if !complete || args.refetch {
        if args.refetch && complete {
            bundle.write_state(&FetchState::default())?;
            fs::remove_file(dir.join(bundle::MANIFEST)).ok();
        }
        let cancel = CancellationToken::new();
        {
            let cancel = cancel.clone();
            tokio::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    eprintln!("\nStopping after the current page; run the same command to resume.");
                    cancel.cancel();
                }
            });
        }
        let progress = |p: &FetchProgress| {
            eprint!(
                "\r  {} people, {} tickets, {} replies, {} files{}",
                p.people,
                p.tickets,
                p.comments,
                p.attachments,
                if p.throttled > 0 {
                    format!(" (waited on Zendesk {}x)", p.throttled)
                } else {
                    String::new()
                }
            );
        };
        eprintln!("Fetching into {} ...", dir.display());
        let fetched = match &args.export_zip {
            Some(zip) => zendesk::read_export(&source, zip, &bundle, &progress, cancel).await,
            None => source.fetch(&bundle, &progress, cancel).await,
        };
        let manifest = match fetched {
            Ok(m) => m,
            Err(backend::services::imports::helpdesk::source::SourceError::Unauthorized(why)) => {
                eprintln!();
                bail!(
                    "Zendesk refused the credential ({why}). OAuth tokens expire (30 minutes by \
                     default): give a fresh one and run the same command; the fetch resumes \
                     where it stopped"
                );
            }
            Err(backend::services::imports::helpdesk::source::SourceError::Cancelled) => {
                eprintln!();
                bail!("stopped; run the same command to resume the fetch");
            }
            Err(e) => {
                eprintln!();
                return Err(anyhow!("{e}. Run the same command to resume the fetch"));
            }
        };
        eprintln!();
        for note in &manifest.notes {
            eprintln!("  note: {note}");
        }
    } else {
        eprintln!(
            "Using the complete copy in {} (--refetch fetches again).",
            dir.display()
        );
        // Files that went missing for a reason that may have passed are
        // tried again, without fetching anything else; not on a dry run,
        // which changes nothing.
        if !args.dry_run {
            use backend::services::imports::helpdesk::zendesk::Retried;
            match source
                .retry_missing_files(&bundle, CancellationToken::new())
                .await
            {
                Ok(Retried::Done { recovered, .. }) if recovered > 0 => {
                    eprintln!("  fetched {recovered} files that were missing")
                }
                Ok(Retried::Busy) => eprintln!(
                    "  another run is using {}, so missing files weren't tried again",
                    dir.display()
                ),
                Ok(_) => {}
                Err(e) => eprintln!("warning: couldn't try the missing files again: {e}"),
            }
        }
    }

    let opts = LoadOptions {
        job_id: uuid::Uuid::now_v7(),
        workspace_id: ws.id,
        actor,
        numbers: match args.numbers {
            NumbersArg::Auto => NumberPolicy::Auto,
            NumbersArg::Keep => NumberPolicy::Keep,
            NumbersArg::After => NumberPolicy::AfterExisting,
        },
        agent_map,
        create_agents: args.create_agents,
        allow_missing_people: args.allow_missing_people,
        batch_size: loader::DEFAULT_BATCH,
        cancel: CancellationToken::new(),
    };
    println!();
    println!(
        "Into workspace {} \"{}\" ({}), as {}",
        ws.id, ws.slug, ws.name, args.run_as
    );
    println!();
    let plan = loader::plan(&pool, &bundle, &opts).await?;
    let plan_text = report::render_plan(&plan);
    print!("{plan_text}");
    if args.dry_run {
        println!();
        if let Some(why) = &plan.refused {
            println!("A run would refuse: {why}");
            println!();
        }
        println!("Dry run: nothing was written.");
        return Ok(());
    }
    println!();
    if let Some(why) = plan.refused {
        bail!("{why} Nothing was written.");
    }
    if !args.yes && !confirm_on_tty(&format!("Import into workspace \"{}\"?", ws.slug))? {
        eprintln!("Nothing was written.");
        return Ok(());
    }

    let started = std::time::Instant::now();
    {
        // Stop between tickets and files, so nothing is left half written.
        let cancel = opts.cancel.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                eprintln!("\nStopping after the current ticket; run the same command to carry on.");
                cancel.cancel();
            }
        });
    }
    let result = loader::load(&pool, storage, &bundle, &opts, &|p| {
        eprint!(
            "\r  {}/{} people, {}/{} tickets, {} replies, {} files",
            p.people_done,
            p.people_total,
            p.tickets_done,
            p.tickets_total,
            p.comments_written,
            p.attachments_written
        );
    })
    .await;
    eprintln!();

    // Whatever landed should be searchable, even after a failure: ask the
    // server (which holds the index) to rebuild this workspace's.
    {
        let (pool, ws_id) = (pool.clone(), ws.id);
        let asked = tokio::task::spawn_blocking(move || {
            backend::sync::session::run_in_workspace(&pool, "cli:import_helpdesk", ws_id, |c| {
                backend::repository::search_reindex::request(c, ws_id)
            })
        })
        .await;
        if !matches!(asked, Ok(Ok(_))) {
            eprintln!("warning: couldn't ask the server to reindex search; imported tickets appear in search after its next reindex");
        }
    }

    // The report is written whether the run finished or not.
    let (rep, failure) = match result {
        Ok(rep) => (rep, None),
        Err(f) => (*f.report, Some(f.error)),
    };
    let mut text = report::render_report(&rep);
    if let Some(e) = &failure {
        text.push_str(&format!("\nThe run stopped: {e}\n"));
    }
    print!("{text}");
    let path = dir.join(format!("report-{}.txt", opts.job_id));
    fs::write(
        &path,
        format!("Import run {}\n\n{plan_text}\n{text}", opts.job_id),
    )
    .with_context(|| format!("writing {}", path.display()))?;
    if let Some(e) = failure {
        return Err(anyhow!("{e}. Report: {}", path.display()));
    }
    println!();
    println!(
        "Import run {} finished in {}s. Report: {}",
        opts.job_id,
        started.elapsed().as_secs(),
        path.display()
    );
    println!("To bring across replies added since, run it again with --refetch.");
    println!(
        "{} holds a copy of the customer's helpdesk. Delete it once the customer has signed off.",
        dir.display()
    );
    Ok(())
}

// ---------------------------------------------------------------
// Workspace export and import
// ---------------------------------------------------------------

fn run_workspace(cmd: WorkspaceCommand) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build runtime")?;
    runtime.block_on(async move {
        let pool = db::establish_connection_pool();
        let storage = backend::utils::storage::process_storage();
        match cmd {
            WorkspaceCommand::Export {
                workspace,
                out,
                seal,
                password_stdin,
                password_env,
            } => {
                let password = if seal {
                    Some(read_password_interactive()?)
                } else if password_stdin {
                    Some(read_password_from_stdin()?)
                } else {
                    resolve_password(None, password_env)?
                };
                workspace_export(&pool, storage, &workspace, password, &out).await
            }
            WorkspaceCommand::Import {
                archive,
                into,
                dry_run,
                yes,
                regenerate_uuids,
                password_stdin,
                password_env,
            } => {
                let password = if password_stdin {
                    Some(read_password_from_stdin()?)
                } else {
                    resolve_password(None, password_env)?
                };
                workspace_import(
                    transfer::runtime(pool.clone(), storage),
                    &into,
                    archive,
                    password,
                    regenerate_uuids,
                    dry_run,
                    yes,
                )
                .await
            }
            WorkspaceCommand::ImportStatus { job } => {
                let job = transfer::find_job(&pool, job).await?;
                print!("{}", transfer::render_job(&job));
                Ok(())
            }
            WorkspaceCommand::ImportRetryFiles {
                job,
                password_stdin,
                password_env,
            } => {
                let found = transfer::find_job(&pool, job).await?;
                let password = if password_stdin {
                    Some(read_password_from_stdin()?)
                } else {
                    resolve_password(None, password_env)?
                };
                let password = match password {
                    None if found.sealed => Some(
                        rpassword::prompt_password("Export password: ")
                            .with_context(|| "reading password from TTY")?,
                    ),
                    password => password,
                };
                let rt = transfer::runtime(pool.clone(), storage);
                let job = transfer::retry_files(
                    &rt,
                    &transfer::operator(),
                    found.workspace_id,
                    job,
                    password,
                    |_| eprintln!("Restoring files..."),
                )
                .await?;
                finish_import(&job)
            }
            WorkspaceCommand::ImportReset { job, yes } => {
                let found = transfer::find_job(&pool, job).await?;
                print!("{}", transfer::render_job(&found));
                let prompt = if found.applied_at.is_some() {
                    "Reset this import? The workspace goes back to how it was provisioned and the \
                     people the import created are removed."
                } else {
                    "Discard this import?"
                };
                if !yes && !confirm_on_tty(prompt)? {
                    eprintln!("aborted");
                    return Ok(());
                }
                let rt = transfer::runtime(pool.clone(), storage);
                let job =
                    transfer::reset(&rt, &transfer::operator(), found.workspace_id, job).await?;
                println!();
                print!("{}", transfer::render_job(&job));
                Ok(())
            }
        }
    })
}

async fn workspace_export(
    pool: &db::Pool,
    storage: std::sync::Arc<dyn backend::utils::storage::Storage>,
    target: &str,
    password: Option<String>,
    out: &Path,
) -> Result<()> {
    let ws = transfer::resolve_workspace(pool, target).await?;
    eprintln!(
        "Exporting workspace {} \"{}\" ({})...",
        ws.id,
        ws.slug,
        if password.is_some() {
            "sealed, with integration secrets"
        } else {
            "plaintext, without integration secrets"
        }
    );
    let size = transfer::export(pool, storage, ws.id, password.as_deref(), out).await?;
    println!("Wrote {} ({size} bytes).", out.display());
    Ok(())
}

async fn workspace_import(
    rt: backend::services::workspace_import_jobs::Runtime,
    into: &str,
    archive: PathBuf,
    mut password: Option<String>,
    regenerate_uuids: bool,
    dry_run: bool,
    yes: bool,
) -> Result<()> {
    use std::io::IsTerminal;
    let origin = transfer::operator();
    let ws = transfer::resolve_workspace(&rt.pool, into).await?;
    if password.is_none() && is_sealed_file(&archive) {
        if !std::io::stdin().is_terminal() {
            bail!("the export is sealed: pass --password-stdin or --password-env");
        }
        password = Some(
            rpassword::prompt_password("Export password: ")
                .with_context(|| "reading password from TTY")?,
        );
    }
    let req = transfer::ImportRequest {
        workspace_id: ws.id,
        archive,
        password: password.clone(),
        regenerate_uuids,
    };
    println!("Into workspace {} \"{}\" ({})", ws.id, ws.slug, ws.name);
    println!();

    if dry_run {
        let preview = transfer::dry_run(&rt, &origin, &req).await?;
        print!("{}", transfer::render_preview(&preview.report));
        if let Some(refused) = preview.refused {
            return Err(refused.into());
        }
        println!();
        println!("Dry run: nothing was written.");
        return Ok(());
    }

    if !yes && !std::io::stdin().is_terminal() {
        bail!("no terminal to confirm on: pass --yes, or --dry-run to preview");
    }
    eprintln!("Checking the export...");
    let (job, preview) = transfer::check(&rt, &origin, &req).await?;
    print!("{}", transfer::render_preview(&preview.report));
    if let Some(refused) = preview.refused {
        return Err(refused.into());
    }
    println!();
    if !yes && !confirm_on_tty(&format!("Import into workspace \"{}\"?", ws.slug))? {
        transfer::reset(&rt, &origin, ws.id, job.id).await?;
        eprintln!("Discarded import {}.", job.id);
        return Ok(());
    }
    let job = transfer::import(&rt, &origin, ws.id, job.id, password, |job| {
        match job.stage.as_deref() {
            Some("rows") => eprintln!("Importing rows..."),
            Some("files") => eprintln!("Restoring files..."),
            Some(other) => eprintln!("{other}..."),
            None => {}
        }
    })
    .await?;
    finish_import(&job)
}

/// Print how an import ended; an error unless it imported.
fn finish_import(job: &backend::models::WorkspaceImportJob) -> Result<()> {
    use backend::services::workspace_import_jobs as jobs;
    println!();
    print!("{}", transfer::render_job(job));
    if job.state == jobs::IMPORTED {
        return Ok(());
    }
    if jobs::actions(job)["retry_files"] == true {
        eprintln!(
            "Retry the files with: nosdesk-cli workspace import-retry-files {}",
            job.id
        );
    }
    let error = job
        .error
        .as_ref()
        .map(transfer::Refusal::from_value)
        .unwrap_or_else(|| transfer::Refusal::new("import_failed", "The import didn't finish."));
    Err(error.into())
}

fn is_sealed_file(path: &Path) -> bool {
    use std::io::Read as _;
    let mut head = [0u8; 4];
    fs::File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .map(|n| backend::services::workspace_import_jobs::is_sealed(&head[..n]))
        .unwrap_or(false)
}

/// [`confirm_destructive`], refusing rather than reading an empty
/// line as "no" when there's no terminal to ask on.
fn confirm_on_tty(prompt: &str) -> Result<bool> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        bail!("no terminal to confirm on: pass --yes");
    }
    confirm_destructive(prompt)
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

fn base64_decode(s: &str) -> Result<Vec<u8>> {
    BASE64
        .decode(s.as_bytes())
        .map_err(|e| anyhow!("base64 decode failed: {e}"))
}
