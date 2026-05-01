//! Admin-only CRUD for `channels`, `channel_credentials`, plus a
//! `test-connection` probe that verifies an IMAP mailbox is reachable.
//!
//! All routes live under `/api/admin/channels` and require
//! `role = admin`. Credentials never ride back out to the client — the
//! response only reports whether a credential is stored.
//!
//! Each `pub async fn` is a thin shim around an `_impl` that returns
//! `Result<HttpResponse, HttpResponse>`, so the bodies stay linear
//! with `?` rather than a tower of `match` blocks. The `Err` branch is
//! the already-built error response; the shim just collapses the
//! `Result` back into a single `HttpResponse`.

use actix_web::{web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use tracing::{error, info, warn};

use crate::db::{DbConnection, Pool};
use crate::handlers::errors;
use crate::handlers::helpers;
use crate::models::{
    Channel, ChannelUpdate, NewChannel, CRED_TYPE_IMAP_PASSWORD,
};
use crate::repository::channels as channels_repo;
use crate::services::channels::email_imap::{test_imap_connection, ImapChannelConfig};
use crate::services::channels::supervisor::ChannelControl;

// ---------- Response / request DTOs ----------

/// Shape returned to the admin UI. Wraps [`Channel`] with a flag telling
/// the UI whether a password is already stored (so the form can hide
/// the password input or show a "replace" affordance).
#[derive(Debug, Serialize)]
pub struct ChannelResponse {
    #[serde(flatten)]
    pub channel: Channel,
    pub has_credential: bool,
}

impl ChannelResponse {
    fn build(channel: Channel, conn: &mut DbConnection) -> Result<Self, HttpResponse> {
        // For phase-1 `email_imap` the only credential type is the
        // password. When other providers arrive they'll need their own
        // flag shape.
        let has = channels_repo::get_credential(conn, channel.id, CRED_TYPE_IMAP_PASSWORD)
            .map_err(|e| {
                error!(error = %e, "failed to check credential presence");
                server_error("Failed to read channel credentials")
            })?
            .is_some();
        Ok(Self {
            channel,
            has_credential: has,
        })
    }
}

/// Body of `POST /api/admin/channels`. Password is stored in the
/// encrypted credentials table and stripped before we return the
/// channel row.
#[derive(Debug, Deserialize)]
pub struct CreateChannelRequest {
    pub provider: String,
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    pub config: JsonValue,
    /// Optional at create time — admins who want to verify the config
    /// via `POST /test-connection` before locking credentials in can
    /// omit this and set it afterwards via `PUT /credentials`.
    #[serde(default)]
    pub password: Option<String>,
}

/// Body of `PATCH /api/admin/channels/{id}`. All fields optional.
#[derive(Debug, Deserialize, Default)]
pub struct UpdateChannelRequest {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub config: Option<JsonValue>,
    /// When set, replaces the stored password. Clearing it is done via
    /// `DELETE /api/admin/channels/{id}/credentials`, never via an
    /// empty string — that would silently nuke a working secret.
    pub password: Option<String>,
}

/// Body of `POST /api/admin/channels/{id}/test-connection`. If the
/// stored password should be used, omit this field. If not, the
/// caller can pass a candidate password to try before committing.
#[derive(Debug, Deserialize, Default)]
pub struct TestConnectionRequest {
    #[serde(default)]
    pub password: Option<String>,
}

// ---------- Small response helpers ----------

fn server_error(msg: &str) -> HttpResponse {
    errors::internal(msg)
}

fn bad_request(msg: impl Into<String>) -> HttpResponse {
    errors::bad_request(msg)
}

/// `None` is treated as 404; other DB errors log + return 500. Callers
/// that already hold a connection use this to distinguish the "row
/// doesn't exist" path from transport errors.
fn load_channel(conn: &mut DbConnection, id: i32) -> Result<Channel, HttpResponse> {
    channels_repo::find(conn, id).map_err(|e| match e {
        diesel::result::Error::NotFound => HttpResponse::NotFound().finish(),
        other => {
            error!(error = %other, "failed to load channel");
            server_error("Failed to load channel")
        }
    })
}

fn collapse(r: Result<HttpResponse, HttpResponse>) -> HttpResponse {
    match r {
        Ok(r) | Err(r) => r,
    }
}

/// Validate provider-specific config JSON. Adding a new provider means
/// adding a branch here — keeps bad config from reaching the adapter
/// and blowing up at poll time.
fn validate_config(provider: &str, config: &JsonValue) -> Result<(), String> {
    match provider {
        "email_imap" => serde_json::from_value::<ImapChannelConfig>(config.clone())
            .map(|_| ())
            .map_err(|e| format!("invalid email_imap config: {e}")),
        other => Err(format!("unknown provider: {other}")),
    }
}

// ---------- Routes ----------

/// GET /api/admin/channels
pub async fn list_channels(pool: web::Data<Pool>, req: HttpRequest) -> HttpResponse {
    collapse(list_channels_impl(pool, req).await)
}

async fn list_channels_impl(
    pool: web::Data<Pool>,
    req: HttpRequest,
) -> Result<HttpResponse, HttpResponse> {
    let mut conn = helpers::admin_conn(&req, &pool)?;
    let rows = channels_repo::list_channels(&mut conn).map_err(|e| {
        error!(error = %e, "failed to list channels");
        server_error("Failed to list channels")
    })?;
    let out: Vec<ChannelResponse> = rows
        .into_iter()
        .map(|ch| ChannelResponse::build(ch, &mut conn))
        .collect::<Result<_, _>>()?;
    Ok(HttpResponse::Ok().json(out))
}

/// GET /api/admin/channels/{id}
pub async fn get_channel(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    req: HttpRequest,
) -> HttpResponse {
    collapse(get_channel_impl(pool, path, req).await)
}

async fn get_channel_impl(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    req: HttpRequest,
) -> Result<HttpResponse, HttpResponse> {
    let mut conn = helpers::admin_conn(&req, &pool)?;
    let channel = load_channel(&mut conn, path.into_inner())?;
    let body = ChannelResponse::build(channel, &mut conn)?;
    Ok(HttpResponse::Ok().json(body))
}

/// POST /api/admin/channels
pub async fn create_channel(
    pool: web::Data<Pool>,
    body: web::Json<CreateChannelRequest>,
    control: web::Data<ChannelControl>,
    req: HttpRequest,
) -> HttpResponse {
    collapse(create_channel_impl(pool, body, control, req).await)
}

async fn create_channel_impl(
    pool: web::Data<Pool>,
    body: web::Json<CreateChannelRequest>,
    control: web::Data<ChannelControl>,
    req: HttpRequest,
) -> Result<HttpResponse, HttpResponse> {
    let mut conn = helpers::admin_conn(&req, &pool)?;

    if body.name.trim().is_empty() {
        return Err(bad_request("Name is required"));
    }
    validate_config(&body.provider, &body.config).map_err(bad_request)?;

    let channel = channels_repo::create(
        &mut conn,
        NewChannel {
            provider: body.provider.clone(),
            name: body.name.trim().to_string(),
            enabled: body.enabled,
            config: body.config.clone(),
        },
    )
    .map_err(|e| {
        error!(error = %e, "failed to create channel");
        server_error("Failed to create channel")
    })?;

    if let Some(password) = body.password.as_deref() {
        if password.is_empty() {
            return Err(bad_request("Password, if provided, must not be empty"));
        }
        channels_repo::put_credential(
            &mut conn,
            channel.id,
            CRED_TYPE_IMAP_PASSWORD,
            password,
            None,
        )
        .map_err(|e| {
            error!(error = %e, "failed to store channel credential");
            server_error("Failed to store credential")
        })?;
    }

    info!(channel_id = channel.id, provider = %channel.provider, "channel created");
    // Ask the supervisor to spin up a worker. Upsert is idempotent —
    // if the channel is `enabled = false` (admin wanted to add creds
    // before going live) the supervisor will just leave it stopped.
    control.upsert(channel.id).await;
    let body = ChannelResponse::build(channel, &mut conn)?;
    Ok(HttpResponse::Created().json(body))
}

/// PATCH /api/admin/channels/{id}
pub async fn update_channel(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<UpdateChannelRequest>,
    control: web::Data<ChannelControl>,
    req: HttpRequest,
) -> HttpResponse {
    collapse(update_channel_impl(pool, path, body, control, req).await)
}

async fn update_channel_impl(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<UpdateChannelRequest>,
    control: web::Data<ChannelControl>,
    req: HttpRequest,
) -> Result<HttpResponse, HttpResponse> {
    let mut conn = helpers::admin_conn(&req, &pool)?;
    let channel_id = path.into_inner();
    let existing = load_channel(&mut conn, channel_id)?;

    if let Some(ref cfg) = body.config {
        validate_config(&existing.provider, cfg).map_err(bad_request)?;
    }
    if let Some(ref name) = body.name {
        if name.trim().is_empty() {
            return Err(bad_request("Name must not be empty"));
        }
    }

    let change = ChannelUpdate {
        name: body.name.clone().map(|n| n.trim().to_string()),
        enabled: body.enabled,
        config: body.config.clone(),
        ..Default::default()
    };
    let updated = channels_repo::update(&mut conn, channel_id, change).map_err(|e| {
        error!(error = %e, "failed to update channel");
        server_error("Failed to update channel")
    })?;

    if let Some(ref new_password) = body.password {
        if new_password.is_empty() {
            return Err(bad_request(
                "Password must not be empty — use DELETE credentials to clear",
            ));
        }
        channels_repo::put_credential(
            &mut conn,
            channel_id,
            CRED_TYPE_IMAP_PASSWORD,
            new_password,
            None,
        )
        .map_err(|e| {
            error!(error = %e, "failed to rotate channel credential");
            server_error("Failed to rotate credential")
        })?;
    }

    info!(channel_id, "channel updated");
    // Reconcile: stops any running worker and, if the row is still
    // enabled, starts a fresh one with the new config/credentials.
    // Disabling via this PATCH converges cleanly — the supervisor
    // stops the worker and leaves it stopped.
    control.upsert(channel_id).await;
    let body = ChannelResponse::build(updated, &mut conn)?;
    Ok(HttpResponse::Ok().json(body))
}

/// DELETE /api/admin/channels/{id}
///
/// Cascades to `channel_credentials` and `channel_messages` via the
/// migration's `ON DELETE CASCADE`. Tickets with `origin_channel_id`
/// pointing here remain (the FK is `ON DELETE SET NULL`); they stay
/// usable but future outbound relay is skipped.
pub async fn delete_channel(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    control: web::Data<ChannelControl>,
    req: HttpRequest,
) -> HttpResponse {
    collapse(delete_channel_impl(pool, path, control, req).await)
}

async fn delete_channel_impl(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    control: web::Data<ChannelControl>,
    req: HttpRequest,
) -> Result<HttpResponse, HttpResponse> {
    let mut conn = helpers::admin_conn(&req, &pool)?;
    let channel_id = path.into_inner();
    let rows = channels_repo::delete(&mut conn, channel_id).map_err(|e| {
        error!(error = %e, "failed to delete channel");
        server_error("Failed to delete channel")
    })?;
    if rows == 0 {
        return Ok(HttpResponse::NotFound().finish());
    }
    // Tell the supervisor to stop the worker. Delete is idempotent,
    // so ordering with the DB commit doesn't matter — in the worst
    // case the worker gets told to stop twice.
    control.delete(channel_id).await;
    info!(channel_id, "channel deleted");
    Ok(HttpResponse::NoContent().finish())
}

/// DELETE /api/admin/channels/{id}/credentials
pub async fn clear_credential(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    control: web::Data<ChannelControl>,
    req: HttpRequest,
) -> HttpResponse {
    collapse(clear_credential_impl(pool, path, control, req).await)
}

async fn clear_credential_impl(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    control: web::Data<ChannelControl>,
    req: HttpRequest,
) -> Result<HttpResponse, HttpResponse> {
    let mut conn = helpers::admin_conn(&req, &pool)?;
    let channel_id = path.into_inner();
    channels_repo::delete_credential(&mut conn, channel_id, CRED_TYPE_IMAP_PASSWORD)
        .map_err(|e| {
            error!(error = %e, "failed to clear channel credential");
            server_error("Failed to clear credential")
        })?;
    // Reconcile so the running worker (if any) observes the missing
    // credential on its next start attempt; it'll fail with a
    // Configuration error and settle into a stopped state rather than
    // continuing to hit auth failures with the cached password.
    control.upsert(channel_id).await;
    Ok(HttpResponse::NoContent().finish())
}

/// POST /api/admin/channels/{id}/test-connection
///
/// Opens an IMAP session against the channel's config using either the
/// caller-supplied candidate password or the stored one. Does not
/// persist anything. Body field `password` is optional.
pub async fn test_connection(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<TestConnectionRequest>,
    req: HttpRequest,
) -> HttpResponse {
    collapse(test_connection_impl(pool, path, body, req).await)
}

async fn test_connection_impl(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<TestConnectionRequest>,
    req: HttpRequest,
) -> Result<HttpResponse, HttpResponse> {
    let mut conn = helpers::admin_conn(&req, &pool)?;
    let channel_id = path.into_inner();
    let channel = load_channel(&mut conn, channel_id)?;

    if channel.provider != "email_imap" {
        return Err(bad_request("test-connection is only supported for email_imap"));
    }

    let config: ImapChannelConfig = serde_json::from_value(channel.config.clone())
        .map_err(|e| bad_request(format!("Invalid channel config: {e}")))?;

    // Prefer a candidate password from the body; fall back to the
    // stored one. An empty string in the body is treated as "use
    // stored" rather than "clear" — clearing goes through the
    // dedicated DELETE endpoint.
    let password = match body.password.as_deref().filter(|p| !p.is_empty()) {
        Some(p) => p.to_string(),
        None => channels_repo::get_credential(&mut conn, channel_id, CRED_TYPE_IMAP_PASSWORD)
            .map_err(|e| {
                error!(error = %e, "failed to read channel credential");
                server_error("Failed to read stored credential")
            })?
            .ok_or_else(|| {
                bad_request("No stored password — provide one in the request body")
            })?,
    };

    match test_imap_connection(&config, &password).await {
        Ok(()) => {
            info!(channel_id, "test-connection succeeded");
            Ok(HttpResponse::Ok().json(json!({ "ok": true })))
        }
        Err(e) => {
            warn!(channel_id, error = %e, "test-connection failed");
            Ok(HttpResponse::Ok().json(json!({ "ok": false, "error": e })))
        }
    }
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    //! HTTP-level tests use the `setup_test_pool` harness. They cover:
    //!
    //! - admin guard (non-admin → 403)
    //! - create with invalid provider / bad config → 400
    //! - create round-trip → GET returns channel + has_credential
    //! - update rotates the password without leaking it
    //! - delete cascades the credential row
    //! - test-connection is only allowed for `email_imap` and refuses
    //!   when no password is available
    //!
    //! The IMAP probe itself (`test_imap_connection` happy-path) is not
    //! unit-tested here — it needs Greenmail, which lives in
    //! `tests/channels_email_imap_integration.rs`.

    use super::*;
    use crate::models::{Claims, UserRole};
    use crate::test_helpers::{create_test_claims, setup_test_pool, TestFixtures};
    use actix_web::{test as actix_test, web, App, HttpMessage};

    /// Build a shared test pool, seed a user with the given role, and
    /// return the pool + claims. `setup_test_pool` uses `max_size(1)`
    /// so seed and handler share one transaction; the transaction
    /// rolls back on pool drop.
    ///
    /// Tests must drop the seed connection before the HTTP call to
    /// avoid a deadlock on the single-connection pool.
    fn seeded_env(role: UserRole, name: &str) -> (Pool, Claims) {
        let pool = setup_test_pool();
        let claims = {
            let mut conn = pool.get().unwrap();
            let user = TestFixtures::create_user(&mut conn, name, role);
            create_test_claims(&user)
        };
        (pool, claims)
    }

    fn build_app_with_pool(
        pool: Pool,
    ) -> App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Response = actix_web::dev::ServiceResponse,
            Config = (),
            InitError = (),
            Error = actix_web::Error,
        >,
    > {
        // Spin up a real supervisor against the test pool so the
        // handler's `control.upsert(..)` / `control.delete(..)` calls
        // are exercised too. The supervisor's hydration from an empty
        // DB is a no-op; subsequent commands run through the full
        // reconcile path against the test transaction.
        use crate::services::channels::registry::RegistryDeps;
        use crate::services::channels::supervisor;
        let deps = RegistryDeps {
            pool: pool.clone(),
            email: None,
            sse: None,
            search: None,
            storage: None,
            http: None,
        };
        let (control, _join) = supervisor::spawn(deps);

        App::new()
            .app_data(web::Data::new(pool))
            .app_data(web::Data::new(control))
            .route("/api/admin/channels", web::get().to(list_channels))
            .route("/api/admin/channels", web::post().to(create_channel))
            .route("/api/admin/channels/{id}", web::get().to(get_channel))
            .route("/api/admin/channels/{id}", web::patch().to(update_channel))
            .route("/api/admin/channels/{id}", web::delete().to(delete_channel))
            .route(
                "/api/admin/channels/{id}/credentials",
                web::delete().to(clear_credential),
            )
            .route(
                "/api/admin/channels/{id}/test-connection",
                web::post().to(test_connection),
            )
    }

    fn sample_imap_config() -> JsonValue {
        json!({
            "host": "imap.example.com",
            "port": 993,
            "username": "support@example.com",
            "mailbox": "INBOX",
            "use_tls": true,
            "reply_domain": "example.com",
        })
    }

    #[actix_web::test]
    async fn non_admin_cannot_list_channels() {
        let (pool, claims) = seeded_env(UserRole::User, "plain-user");
        let srv = actix_test::init_service(build_app_with_pool(pool)).await;

        let req = actix_test::TestRequest::get()
            .uri("/api/admin/channels")
            .to_request();
        req.extensions_mut().insert(claims);

        let resp = actix_test::call_service(&srv, req).await;
        assert_eq!(resp.status(), 403);
    }

    #[actix_web::test]
    async fn create_rejects_unknown_provider() {
        let (pool, claims) = seeded_env(UserRole::Admin, "admin-test");
        let srv = actix_test::init_service(build_app_with_pool(pool)).await;

        let req = actix_test::TestRequest::post()
            .uri("/api/admin/channels")
            .set_json(json!({
                "provider": "quantum-entanglement-mail",
                "name": "nonsense",
                "enabled": true,
                "config": {},
            }))
            .to_request();
        req.extensions_mut().insert(claims);

        let resp = actix_test::call_service(&srv, req).await;
        assert_eq!(resp.status(), 400);
    }

    #[actix_web::test]
    async fn create_rejects_missing_config_fields() {
        let (pool, claims) = seeded_env(UserRole::Admin, "admin-test");
        let srv = actix_test::init_service(build_app_with_pool(pool)).await;

        let req = actix_test::TestRequest::post()
            .uri("/api/admin/channels")
            .set_json(json!({
                "provider": "email_imap",
                "name": "Support mailbox",
                "enabled": true,
                "config": { "host": "imap.example.com" }, // missing username + reply_domain
            }))
            .to_request();
        req.extensions_mut().insert(claims);

        let resp = actix_test::call_service(&srv, req).await;
        assert_eq!(resp.status(), 400);
    }

    #[actix_web::test]
    async fn create_then_fetch_roundtrips_and_marks_credential_stored() {
        let (pool, claims) = seeded_env(UserRole::Admin, "admin-test");
        let srv = actix_test::init_service(build_app_with_pool(pool)).await;

        let req = actix_test::TestRequest::post()
            .uri("/api/admin/channels")
            .set_json(json!({
                "provider": "email_imap",
                "name": "Support mailbox",
                "enabled": true,
                "config": sample_imap_config(),
                "password": "hunter2"
            }))
            .to_request();
        req.extensions_mut().insert(claims.clone());

        let resp = actix_test::call_service(&srv, req).await;
        assert_eq!(resp.status(), 201);
        let created: serde_json::Value = actix_test::read_body_json(resp).await;
        let id = created["id"].as_i64().unwrap() as i32;
        assert_eq!(created["has_credential"], json!(true));
        // Password must not echo back.
        assert!(created.get("password").is_none());

        let req = actix_test::TestRequest::get()
            .uri(&format!("/api/admin/channels/{id}"))
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&srv, req).await;
        assert_eq!(resp.status(), 200);
        let fetched: serde_json::Value = actix_test::read_body_json(resp).await;
        assert_eq!(fetched["has_credential"], json!(true));
        assert_eq!(fetched["name"], json!("Support mailbox"));
    }

    #[actix_web::test]
    async fn update_rotates_password_without_echoing_it() {
        let (pool, claims) = seeded_env(UserRole::Admin, "admin-test");
        let srv = actix_test::init_service(build_app_with_pool(pool)).await;

        // Create without password first.
        let req = actix_test::TestRequest::post()
            .uri("/api/admin/channels")
            .set_json(json!({
                "provider": "email_imap",
                "name": "Support mailbox",
                "enabled": false,
                "config": sample_imap_config(),
            }))
            .to_request();
        req.extensions_mut().insert(claims.clone());
        let resp = actix_test::call_service(&srv, req).await;
        let created: serde_json::Value = actix_test::read_body_json(resp).await;
        let id = created["id"].as_i64().unwrap() as i32;
        assert_eq!(created["has_credential"], json!(false));

        // Patch in a password.
        let req = actix_test::TestRequest::patch()
            .uri(&format!("/api/admin/channels/{id}"))
            .set_json(json!({
                "enabled": true,
                "password": "new-secret"
            }))
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&srv, req).await;
        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = actix_test::read_body_json(resp).await;
        assert_eq!(body["has_credential"], json!(true));
        assert_eq!(body["enabled"], json!(true));
    }

    #[actix_web::test]
    async fn delete_channel_cascades_credential() {
        let (pool, claims) = seeded_env(UserRole::Admin, "admin-test");
        let srv = actix_test::init_service(build_app_with_pool(pool)).await;

        // Create with password.
        let req = actix_test::TestRequest::post()
            .uri("/api/admin/channels")
            .set_json(json!({
                "provider": "email_imap",
                "name": "doomed",
                "enabled": true,
                "config": sample_imap_config(),
                "password": "will-be-wiped"
            }))
            .to_request();
        req.extensions_mut().insert(claims.clone());
        let resp = actix_test::call_service(&srv, req).await;
        let created: serde_json::Value = actix_test::read_body_json(resp).await;
        let id = created["id"].as_i64().unwrap() as i32;

        // Delete.
        let req = actix_test::TestRequest::delete()
            .uri(&format!("/api/admin/channels/{id}"))
            .to_request();
        req.extensions_mut().insert(claims.clone());
        let resp = actix_test::call_service(&srv, req).await;
        assert_eq!(resp.status(), 204);

        // Follow-up GET is 404.
        let req = actix_test::TestRequest::get()
            .uri(&format!("/api/admin/channels/{id}"))
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&srv, req).await;
        assert_eq!(resp.status(), 404);
    }

    #[actix_web::test]
    async fn test_connection_refuses_when_no_password() {
        let (pool, claims) = seeded_env(UserRole::Admin, "admin-test");
        let srv = actix_test::init_service(build_app_with_pool(pool)).await;

        let req = actix_test::TestRequest::post()
            .uri("/api/admin/channels")
            .set_json(json!({
                "provider": "email_imap",
                "name": "sans-creds",
                "enabled": false,
                "config": sample_imap_config(),
            }))
            .to_request();
        req.extensions_mut().insert(claims.clone());
        let resp = actix_test::call_service(&srv, req).await;
        let created: serde_json::Value = actix_test::read_body_json(resp).await;
        let id = created["id"].as_i64().unwrap() as i32;

        let req = actix_test::TestRequest::post()
            .uri(&format!("/api/admin/channels/{id}/test-connection"))
            .set_json(json!({}))
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&srv, req).await;
        assert_eq!(resp.status(), 400);
    }
}
