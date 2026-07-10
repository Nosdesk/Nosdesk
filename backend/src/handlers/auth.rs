use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use bcrypt::verify;
use serde_json::json;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::handlers::errors;
use crate::handlers::helpers;
use crate::models::{LoginRequest, PasswordChangeRequest};
use crate::repository::{self, user_auth_identities::get_local_password_hash};
use crate::utils::auth::hash_password;
use crate::utils::mfa;
use crate::utils::rate_limit::{get_redis_url, RateLimiter};
use crate::utils::{parse_uuid, ValidationError};

// Import JWT utilities
use crate::utils::jwt::{helpers as jwt_helpers, JwtUtils};

/// Authentication routes, mounted inside the rate-limited `/api/auth` scope in
/// main.rs (paths are scope-relative). The `/setup` sub-scope stays in main.rs
/// because it's built conditionally on the deployment mode; everything else
/// lives here. Per-route `cookie_auth_middleware` wraps and the nested
/// `/sessions`, `/mfa`, `/passkeys` scopes are preserved verbatim.
pub fn config(cfg: &mut web::ServiceConfig) {
    use crate::middleware::cookie_auth_middleware;
    use actix_web::middleware::from_fn;
    cfg.route("/login", web::post().to(crate::handlers::login))
        .route("/logout", web::post().to(crate::handlers::logout))
        .route("/mfa-login", web::post().to(crate::handlers::mfa_login))
        .route(
            "/recovery-login",
            web::post().to(crate::handlers::recovery_login),
        )
        .route(
            "/mfa-setup-login",
            web::post().to(crate::handlers::mfa_setup_login),
        )
        .route(
            "/mfa-enable-login",
            web::post().to(crate::handlers::mfa_enable_login),
        )
        .route(
            "/passkey-setup-login/start",
            web::post().to(crate::handlers::start_passkey_setup_login),
        )
        .route(
            "/passkey-setup-login/finish",
            web::post().to(crate::handlers::finish_passkey_setup_login),
        )
        .route("/refresh", web::post().to(crate::handlers::refresh_token))
        .route(
            "/password-reset/request",
            web::post().to(crate::handlers::password_reset::request_password_reset),
        )
        .route(
            "/password-reset/complete",
            web::post().to(crate::handlers::password_reset::reset_password_with_token),
        )
        .route(
            "/invitation/validate",
            web::post().to(crate::handlers::invitation::validate_invitation),
        )
        .route(
            "/invitation/accept",
            web::post().to(crate::handlers::invitation::accept_invitation),
        )
        .route(
            "/providers",
            web::get().to(crate::handlers::get_enabled_auth_providers),
        )
        .route(
            "/oauth/authorize",
            web::post().to(crate::handlers::oauth_authorize),
        )
        .route(
            "/oauth/callback",
            web::get().to(crate::handlers::oauth_callback),
        )
        .route(
            "/oauth/logout",
            web::post().to(crate::handlers::oauth_logout),
        )
        .route(
            "/native-oidc-config",
            web::get().to(crate::handlers::native_oidc_config),
        )
        .route(
            "/oidc/native-login",
            web::post().to(crate::handlers::native_oidc_login),
        )
        .route(
            "/me",
            web::get()
                .to(crate::handlers::get_current_user)
                .wrap(from_fn(cookie_auth_middleware)),
        )
        .route(
            "/change-password",
            web::post()
                .to(crate::handlers::change_password)
                .wrap(from_fn(cookie_auth_middleware)),
        )
        .route(
            "/oauth/connect",
            web::post()
                .to(crate::handlers::oauth_connect)
                .wrap(from_fn(cookie_auth_middleware)),
        )
        .service(
            web::scope("/sessions")
                .wrap(from_fn(cookie_auth_middleware))
                .route("", web::get().to(crate::handlers::get_user_sessions))
                .route(
                    "/others",
                    web::delete().to(crate::handlers::revoke_all_other_sessions),
                )
                .route("/{id}", web::delete().to(crate::handlers::revoke_session)),
        )
        .service(
            web::scope("/mfa")
                .wrap(from_fn(cookie_auth_middleware))
                .route("/setup", web::post().to(crate::handlers::mfa_setup))
                .route(
                    "/verify-setup",
                    web::post().to(crate::handlers::mfa_verify_setup),
                )
                .route("/enable", web::post().to(crate::handlers::mfa_enable))
                .route("/disable", web::post().to(crate::handlers::mfa_disable))
                .route(
                    "/regenerate-backup-codes",
                    web::post().to(crate::handlers::mfa_regenerate_backup_codes),
                )
                .route("/status", web::get().to(crate::handlers::mfa_status)),
        )
        .route(
            "/passkeys/login/start",
            web::post().to(crate::handlers::start_passkey_login),
        )
        .route(
            "/passkeys/login/finish",
            web::post().to(crate::handlers::finish_passkey_login),
        )
        .service(
            web::scope("/passkeys")
                .wrap(from_fn(cookie_auth_middleware))
                .route(
                    "/register/start",
                    web::post().to(crate::handlers::start_passkey_registration),
                )
                .route(
                    "/register/finish",
                    web::post().to(crate::handlers::finish_passkey_registration),
                )
                .route("", web::get().to(crate::handlers::list_passkeys))
                .route(
                    "/{credential_id}",
                    web::patch().to(crate::handlers::rename_passkey),
                )
                .route(
                    "/{credential_id}",
                    web::delete().to(crate::handlers::delete_passkey),
                ),
        );
}

/// Convert ValidationError to HTTP response
impl From<ValidationError> for HttpResponse {
    fn from(error: ValidationError) -> Self {
        match error {
            ValidationError::InvalidUuid(_) => HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": error.to_string()
            })),
            ValidationError::InvalidRole(_) => HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": error.to_string()
            })),
            ValidationError::ValidationFailed(msg) => {
                HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": msg
                }))
            }
        }
    }
}

// JWT token creation functions moved to jwt utils module

/// Helper function to log password change security event
async fn log_password_change_event(
    user_uuid: &Uuid,
    conn: &mut DbConnection,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::utils::security_events::{record_security_event, SecurityEventInput};

    record_security_event(
        conn,
        SecurityEventInput {
            user_uuid: Some(*user_uuid),
            event_type: "password_changed",
            severity: "info",
            details: Some(json!({
                "action": "password_change",
                "success": true
            })),
            // Handler doesn't have the request in scope; this call site
            // pre-dates the shared helper and keeps its previous "no IP"
            // behavior. If we want IP here later, wire the request in.
            request: None,
            session_id: None,
        },
    )?;

    Ok(())
}

/// Parse a device name from a user-agent string.
fn parse_device_name(ua: &str) -> &'static str {
    if ua.contains("iPhone") {
        "iPhone"
    } else if ua.contains("iPad") {
        "iPad"
    } else if ua.contains("Android") {
        "Android Asset"
    } else if ua.contains("Macintosh") || ua.contains("Mac OS") {
        "Mac"
    } else if ua.contains("Windows") {
        "Windows PC"
    } else if ua.contains("Linux") {
        "Linux"
    } else {
        "Unknown Asset"
    }
}

/// Create a session record from an HTTP request. Returns the ActiveSession with its DB-generated session_id.
pub fn create_session_record(
    user_uuid: &Uuid,
    request: &HttpRequest,
    conn: &mut DbConnection,
) -> Result<crate::models::ActiveSession, diesel::result::Error> {
    // Resolve the client IP once: stored as INET, and (when a GeoIP
    // database is configured) used for a coarse "City, Country" label.
    let client_ip = crate::utils::client_ip::from_http_request(request);
    let ip_address = client_ip.and_then(|ip| ip.to_string().parse().ok());
    let location = client_ip.and_then(crate::utils::geoip::lookup);

    let user_agent = request
        .headers()
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let device_name = user_agent
        .as_deref()
        .map(|ua| parse_device_name(ua).to_string());

    let new_session = crate::models::NewActiveSession {
        user_uuid: *user_uuid,
        device_name,
        ip_address,
        user_agent,
        location,
        expires_at: chrono::Utc::now().naive_utc() + chrono::Duration::days(7),
        is_current: true,
    };

    crate::repository::active_sessions::create_session(conn, new_session)
}

/// Create a session + token pair, returning an HttpResponse with auth cookies set.
/// Shared by all login flows that use `create_login_response`.
pub(crate) fn complete_login(
    user: crate::models::User,
    request: &HttpRequest,
    conn: &mut DbConnection,
) -> HttpResponse {
    match establish_login_session(user, request, conn) {
        Ok((response, tokens)) => build_auth_response(request, response, &tokens),
        Err(error_response) => error_response,
    }
}

/// Finish a login by setting the auth cookies and **redirecting** the
/// browser to `location` (302 Found), instead of returning JSON.
///
/// The OAuth/OIDC browser flow lands the user agent directly on the backend
/// callback, so it must redirect onward into the app; a JSON body would
/// dead-end the user on an API URL. (The XHR login path uses
/// [`complete_login`], which returns JSON for the SPA to consume.)
pub(crate) fn complete_login_redirect(
    user: crate::models::User,
    request: &HttpRequest,
    conn: &mut DbConnection,
    location: &str,
) -> HttpResponse {
    match establish_login_session(user, request, conn) {
        Ok((_response, tokens)) => build_auth_cookie_redirect(&tokens, location),
        Err(error_response) => error_response,
    }
}

/// Create the session record, log the success event, and mint the login
/// tokens. Shared by the JSON and redirect login finishers.
pub(crate) fn establish_login_session(
    user: crate::models::User,
    request: &HttpRequest,
    conn: &mut DbConnection,
) -> Result<(crate::models::LoginResponse, jwt_helpers::LoginTokens), HttpResponse> {
    let user_uuid = user.uuid;

    // Pin the request's workspace so the login response's workspace_role
    // resolves under RLS (workspace_members is workspace-isolated).
    helpers::pin_request_workspace(request, conn);

    let session = match create_session_record(&user_uuid, request, conn) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to create session for user {}: {}", user_uuid, e);
            return Err(errors::internal("Failed to create authentication session"));
        }
    };
    let family_id = Uuid::new_v4();

    // W2: record the successful authentication. Covers the no-MFA
    // local-login path and the OAuth/OIDC path (both finish here);
    // the MFA path records via complete_mfa_login.
    let _ = crate::utils::security_events::record_security_event(
        conn,
        crate::utils::security_events::SecurityEventInput {
            user_uuid: Some(user_uuid),
            event_type: "login_success",
            severity: "info",
            details: None,
            request: Some(request),
            session_id: None,
        },
    );

    jwt_helpers::create_login_response(user, &session.session_id, &family_id, conn)
}

/// Create a session + MFA token pair, returning an HttpResponse with auth cookies set.
/// Shared by MFA and recovery login flows.
fn complete_mfa_login(
    user: crate::models::User,
    backup_code_used: bool,
    requires_regeneration: bool,
    request: &HttpRequest,
    conn: &mut DbConnection,
) -> HttpResponse {
    let user_uuid = user.uuid;

    // Pin the request's workspace so the login response's workspace_role
    // resolves under RLS (workspace_members is workspace-isolated).
    helpers::pin_request_workspace(request, conn);

    let session = match create_session_record(&user_uuid, request, conn) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to create session for user {}: {}", user_uuid, e);
            return errors::internal("Failed to create authentication session");
        }
    };
    let family_id = Uuid::new_v4();

    // W2: the MFA path's successful-authentication record (the no-MFA
    // path records in complete_login).
    let _ = crate::utils::security_events::record_security_event(
        conn,
        crate::utils::security_events::SecurityEventInput {
            user_uuid: Some(user_uuid),
            event_type: "login_success",
            severity: "info",
            details: Some(json!({ "via": "mfa" })),
            request: Some(request),
            session_id: None,
        },
    );

    match jwt_helpers::create_mfa_login_response(
        user,
        backup_code_used,
        requires_regeneration,
        &session.session_id,
        &family_id,
        conn,
    ) {
        Ok((response, tokens)) => build_auth_response(request, response, &tokens),
        Err(error_response) => error_response,
    }
}

/// Finish a login response, delivering the session tokens the way the client
/// asked for (see `utils::auth_mode`).
///
/// - Cookie mode (web, the default): set the three httpOnly auth cookies and
///   return `body` unchanged. Byte-identical to the historical behaviour.
/// - Bearer mode (native, `X-Auth-Mode: bearer`): set NO cookies and inject the
///   `access_token` + `refresh_token` into the JSON body, marked `no-store`.
///
/// The body is injected as a `serde_json::Value` rather than via typed fields so
/// the same helper serves both the `LoginResponse` callers and the passkey
/// finishers, which pass an ad-hoc `json!` value.
pub(crate) fn build_auth_response(
    request: &HttpRequest,
    body: impl serde::Serialize,
    tokens: &jwt_helpers::LoginTokens,
) -> HttpResponse {
    use crate::utils::auth_mode::{auth_mode_from_request, AuthMode};

    match auth_mode_from_request(request) {
        AuthMode::Cookie => HttpResponse::Ok()
            .cookie(crate::utils::cookies::create_access_token_cookie(
                &tokens.access_token,
            ))
            .cookie(crate::utils::cookies::create_refresh_token_cookie(
                &tokens.refresh_token,
            ))
            .cookie(crate::utils::cookies::create_csrf_token_cookie(
                &tokens.csrf_token,
            ))
            .json(body),
        AuthMode::Bearer => {
            // Native clients can't use the cookie jar, so hand them the tokens
            // in the body and set no cookies. Inject into the serialized value
            // so both LoginResponse and the passkey json! bodies pick them up.
            let mut value = match serde_json::to_value(&body) {
                Ok(v) => v,
                Err(e) => {
                    tracing::error!("Failed to serialize bearer login body: {}", e);
                    return errors::internal("Failed to build authentication response");
                }
            };
            if let Some(obj) = value.as_object_mut() {
                obj.insert("access_token".to_string(), json!(tokens.access_token));
                obj.insert("refresh_token".to_string(), json!(tokens.refresh_token));
            }
            HttpResponse::Ok()
                // The body now carries session secrets; keep them out of caches.
                .insert_header(("Cache-Control", "no-store"))
                .json(value)
        }
    }
}

/// Set the auth cookies and redirect (302) to `location`. The browser
/// OAuth/OIDC callback finishes here so the user lands in the app.
pub(crate) fn build_auth_cookie_redirect(
    tokens: &jwt_helpers::LoginTokens,
    location: &str,
) -> HttpResponse {
    HttpResponse::Found()
        .cookie(crate::utils::cookies::create_access_token_cookie(
            &tokens.access_token,
        ))
        .cookie(crate::utils::cookies::create_refresh_token_cookie(
            &tokens.refresh_token,
        ))
        .cookie(crate::utils::cookies::create_csrf_token_cookie(
            &tokens.csrf_token,
        ))
        .append_header(("Location", location))
        .finish()
}

// Account lockout configuration (IP rate limiting handled by middleware)
const MAX_LOGIN_ATTEMPTS: u32 = 5;
const LOCKOUT_DURATION_SECONDS: u64 = 900; // 15 minutes

/// True when local credential auth (password + passkey) must be refused.
///
/// In hosted mode the platform OIDC is the only identity source for tenant
/// users; the static-RP passkey model also only fits self-hosted's single
/// origin. Login, recovery, and passkey-login handlers short-circuit on
/// this so the local paths can't be driven directly.
pub(crate) fn hosted_local_auth_disabled() -> bool {
    !crate::middleware::workspace_context::local_credentials_permitted()
}

// Authentication handlers
/// Try LDAP authentication for the request's workspace when local password auth
/// missed. Returns the resolved user on a successful directory bind, or `None`
/// to fall through to the standard failed-login handling (the caller's lockout
/// already bounds how many binds reach the directory, since this runs after the
/// lockout gate). Self-hosted only by construction: the hosted login endpoint is
/// disabled above, and cloud directory sync is SCIM, not LDAP login.
async fn try_ldap_login(
    conn: &mut DbConnection,
    request: &HttpRequest,
    login_data: &LoginRequest,
) -> Option<crate::models::User> {
    let workspace_id = helpers::request_workspace_id(request)?;
    let settings = match repository::workspace_ldap_settings::get_for_workspace(conn, workspace_id)
    {
        Ok(Some(s)) if s.enabled => s,
        Ok(_) => return None, // not configured or disabled
        Err(e) => {
            error!(error = %e, "load ldap settings during login failed");
            return None;
        }
    };
    let bind_password = repository::workspace_ldap_settings::decrypt_bind_password(&settings)
        .ok()
        .flatten()
        .unwrap_or_default();

    use crate::services::ldap::auth::{authenticate, LdapAuthError};
    match authenticate(
        &settings,
        &bind_password,
        &login_data.email,
        &login_data.password,
    )
    .await
    {
        Ok(result) => {
            let input = crate::services::oauth_provisioning::ProjectedUserInput {
                iss: "ldap".to_string(),
                sub: result.external_id,
                identity_workspace_id: Some(workspace_id),
                email: result
                    .email
                    .clone()
                    .unwrap_or_else(|| login_data.email.clone()),
                // The directory is authoritative for the address, so the
                // email-fallback link to a pre-existing local/SSO account is
                // authorised (the "operator created the user, LDAP now signs in"
                // migration case).
                //
                // INTERLOCK (review finding): because email_verified is always
                // true here, a directory bind whose `mail` matches an EXISTING
                // workspace member links onto that seat, and ensure_membership's
                // ON CONFLICT DO NOTHING PRESERVES that member's current role. So
                // `member` below applies only to brand-new users; an existing
                // admin/owner keeps their role. The risk is bounded (the bind
                // needs a valid AD credential, and `mail` is directory-admin-
                // controlled), but when the P4 group->role mapping lands it
                // should scope the email link to in-workspace users and/or audit
                // a directory login that resolves onto an above-baseline role.
                email_verified: true,
                name: result.display_name.clone(),
                // Group->role mapping lands in P4; until then a new LDAP user
                // gets the baseline member role.
                role: "member".to_string(),
                workspace_id,
                password_hash: None,
                metadata: None,
            };
            match crate::services::oauth_provisioning::find_or_create_projected_user(conn, input) {
                Ok(outcome) => Some(outcome.into_user()),
                Err(e) => {
                    error!(error = %e, workspace_id, "ldap user provisioning failed");
                    None
                }
            }
        }
        // Authentication misses fall through silently to the standard failure
        // path (which records the attempt + counts toward the lockout).
        Err(LdapAuthError::EmptyPassword)
        | Err(LdapAuthError::InvalidCredentials)
        | Err(LdapAuthError::UserNotFound)
        | Err(LdapAuthError::AmbiguousUser(_)) => None,
        // A config / connectivity / service-bind problem is logged + recorded as
        // a security event so a broken LDAP setup is visible, then treated as a
        // miss rather than a hard login error.
        Err(e) => {
            error!(error = %e, workspace_id, "ldap login error");
            let _ = crate::utils::security_events::record_security_event(
                conn,
                crate::utils::security_events::SecurityEventInput {
                    user_uuid: None,
                    event_type: "ldap_login_error",
                    severity: "warning",
                    details: Some(json!({ "error": e.to_string() })),
                    request: Some(request),
                    session_id: None,
                },
            );
            None
        }
    }
}

pub async fn login(
    db_pool: web::Data<crate::db::Pool>,
    login_data: web::Json<LoginRequest>,
    request: HttpRequest,
) -> impl Responder {
    // Hosted mode authenticates tenant users exclusively through the
    // platform OIDC; there is no local product password. Reject local
    // sign-in even if the endpoint is reached directly.
    if hosted_local_auth_disabled() {
        return errors::forbidden(
            "Password sign-in is disabled. Sign in with your organisation account.",
        );
    }

    let redis_url = get_redis_url();
    let client_ip = crate::utils::client_ip::from_http_request(&request);
    let lockout_key = RateLimiter::login_attempt_key(&login_data.email, client_ip);

    // Check if account is locked before any validation. Fail-closed detection:
    // an unset/non-canonical ENVIRONMENT still denies on a Redis error below.
    let is_production = crate::config_utils::assume_production();

    match RateLimiter::check_lockout(&redis_url, &lockout_key, MAX_LOGIN_ATTEMPTS).await {
        Ok(Some(remaining_seconds)) => {
            warn!(email = %login_data.email, remaining_seconds, "Login attempt on locked account");
            return errors::too_many_requests(
                format!(
                    "Account temporarily locked. Try again in {} minutes.",
                    (remaining_seconds / 60) + 1
                ),
                remaining_seconds,
            );
        }
        Ok(None) => {} // Not locked, continue
        Err(e) => {
            error!(error = %e, "Redis error checking account lockout");
            if is_production {
                // Fail closed in production - deny login if we can't verify lockout status
                return errors::service_unavailable(
                    "Authentication service temporarily unavailable. Please try again.",
                );
            }
            // Fail open in development for convenience
        }
    }

    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    // Pin the request's workspace up front so every role-dependent check on
    // this connection (the MFA policy gate below, the login response builder)
    // resolves the caller's role under RLS. In production the pool clears
    // app.workspace_id on checkout, so without this the role reads as None
    // and the MFA policy would not see an admin as elevated.
    helpers::pin_request_workspace(&request, &mut conn);

    // AUD-007: lookup + bcrypt verify happen as one equal-work
    // call. Missing users, SSO-only users, and wrong passwords
    // are indistinguishable in wall-clock time.
    //
    // Local password first; if it misses and LDAP is enabled for this workspace,
    // try the directory before declaring failure. LDAP runs only on a local
    // miss, so it's a fallback (a user with a working local password never hits
    // the directory), and it's past the lockout gate so the rate limit bounds
    // how many binds reach the DC.
    let resolved_user = match crate::utils::login_timing::verify_credentials(
        &mut conn,
        &login_data.email,
        &login_data.password,
    ) {
        Some(u) => Some(u),
        None => try_ldap_login(&mut conn, &request, &login_data).await,
    };
    let user = match resolved_user {
        Some(u) => u,
        None => {
            // W2: persist the failed attempt. user_uuid is None — the
            // AUD-007 equal-work verify deliberately can't tell us
            // whether the email matched an account, so we attribute by
            // the attempted identifier in `details` instead. PCI 10.2.4
            // / NIST AU-2(3) want invalid access attempts recorded
            // regardless of whether the account exists.
            let mut locked = false;
            match RateLimiter::record_failed_attempt(
                &redis_url,
                &lockout_key,
                LOCKOUT_DURATION_SECONDS,
            )
            .await
            {
                Ok(attempts) => {
                    let remaining = MAX_LOGIN_ATTEMPTS.saturating_sub(attempts);
                    if remaining == 0 {
                        warn!(email = %login_data.email, "Account locked after {} failed attempts", MAX_LOGIN_ATTEMPTS);
                        locked = true;
                    } else {
                        debug!(email = %login_data.email, attempts, remaining, "Failed login attempt");
                    }
                }
                Err(e) => warn!(error = %e, "Failed to record login attempt"),
            }
            let _ = crate::utils::security_events::record_security_event(
                &mut conn,
                crate::utils::security_events::SecurityEventInput {
                    user_uuid: None,
                    event_type: if locked {
                        "account_locked"
                    } else {
                        "login_failed"
                    },
                    severity: if locked { "warning" } else { "info" },
                    details: Some(json!({
                        "attempted_email": login_data.email,
                        "reason": "invalid_credentials",
                    })),
                    request: Some(&request),
                    session_id: None,
                },
            );
            if locked {
                return errors::too_many_requests(
                    format!(
                        "Account locked after too many failed attempts. Try again in {} minutes.",
                        LOCKOUT_DURATION_SECONDS / 60
                    ),
                    LOCKOUT_DURATION_SECONDS,
                );
            }
            return errors::unauthorized("Invalid email or password");
        }
    };

    if let Err(e) = RateLimiter::clear_attempts(&redis_url, &lockout_key).await {
        warn!(error = %e, "Failed to clear login attempts after successful auth");
    }

    // Check if user has TOTP MFA enabled - if so, require TOTP verification
    if mfa::user_has_mfa_enabled(&user) {
        let response = jwt_helpers::create_mfa_required_response(user.uuid);
        return HttpResponse::Ok().json(response);
    }

    // Check if user has passkeys registered — require passkey verification.
    // Propagate the lookup error rather than silently treating it as
    // "no passkeys" (the F2C.1 C1 fails-OPEN finding). A transient DB
    // error here used to downgrade a passkey-only user to a password-
    // only session; now it surfaces as a 5xx and the user retries.
    match mfa::user_has_passkeys(&mut conn, &user.uuid) {
        Ok(true) => {
            let response = jwt_helpers::create_passkey_mfa_required_response(user.uuid);
            return HttpResponse::Ok().json(response);
        }
        Ok(false) => {}
        Err(e) => {
            error!(error = ?e, user_uuid = %user.uuid, "Failed to check passkey registration during login");
            return errors::internal("Login could not complete; please retry");
        }
    }

    // Check MFA policy enforcement (for users without MFA enabled)
    if let Err(_policy_error) = mfa::validate_mfa_policy(&user, &mut conn).await {
        // Instead of blocking, offer MFA setup for users who need it
        let response = jwt_helpers::create_mfa_setup_required_response(user.uuid);
        return HttpResponse::Ok().json(response);
    }

    complete_login(user, &request, &mut conn)
}

/// MFA Login - Verify MFA token and complete login
pub async fn mfa_login(
    db_pool: web::Data<crate::db::Pool>,
    login_data: web::Json<crate::models::MfaLoginRequest>,
    request: actix_web::HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let user = match crate::utils::login_timing::verify_credentials(
        &mut conn,
        &login_data.email,
        &login_data.password,
    ) {
        Some(u) => u,
        None => return errors::unauthorized("Invalid email or password"),
    };

    // Check that user actually has MFA enabled
    if !mfa::user_has_mfa_enabled(&user) {
        return errors::bad_request("MFA is not enabled for this account");
    }

    // Check rate limiting for MFA attempts
    if !mfa::check_mfa_rate_limit(&user.uuid).await {
        return errors::too_many_requests("Too many MFA attempts. Please try again later.", 60);
    }

    // W2: helper to persist an MFA outcome to security_events. The
    // existing mfa::log_mfa_attempt only emits tracing; this is the
    // durable record (event types match SecurityEventType::MfaFailed /
    // MfaSuccess).
    let record_mfa = |conn: &mut DbConnection, success: bool| {
        let _ = crate::utils::security_events::record_security_event(
            conn,
            crate::utils::security_events::SecurityEventInput {
                user_uuid: Some(user.uuid),
                event_type: if success { "mfa_success" } else { "mfa_failed" },
                severity: if success { "info" } else { "warning" },
                details: Some(json!({ "method": "totp_or_backup", "context": "login" })),
                request: Some(&request),
                session_id: None,
            },
        );
    };

    // Verify MFA token (TOTP or backup code)
    let mfa_result = match mfa::verify_mfa_token(&user.uuid, &login_data.mfa_token, &mut conn).await
    {
        Ok(result) => result,
        Err(e) => {
            mfa::log_mfa_attempt(&user.uuid, false, "login", &request).await;
            record_mfa(&mut conn, false);
            return errors::bad_request(format!("MFA verification failed: {}", e));
        }
    };

    if !mfa_result.is_valid {
        mfa::log_mfa_attempt(&user.uuid, false, "login", &request).await;
        record_mfa(&mut conn, false);
        return errors::bad_request("Invalid MFA token");
    }

    // Log successful MFA attempt
    mfa::log_mfa_attempt(&user.uuid, true, "login", &request).await;
    record_mfa(&mut conn, true);

    complete_mfa_login(
        user,
        mfa_result.backup_code_used.is_some(),
        mfa_result.requires_backup_code_regeneration,
        &request,
        &mut conn,
    )
}

/// Recovery code login - for users with passkey MFA who can't use their passkey
/// Accepts email + password + recovery_code, verifies backup code directly
pub async fn recovery_login(
    db_pool: web::Data<crate::db::Pool>,
    login_data: web::Json<crate::models::RecoveryLoginRequest>,
    request: actix_web::HttpRequest,
) -> impl Responder {
    let redis_url = get_redis_url();
    let client_ip = crate::utils::client_ip::from_http_request(&request);
    let lockout_key = RateLimiter::login_attempt_key(&login_data.email, client_ip);

    // Check if account is locked before any validation
    match RateLimiter::check_lockout(&redis_url, &lockout_key, MAX_LOGIN_ATTEMPTS).await {
        Ok(Some(remaining_seconds)) => {
            warn!(email = %login_data.email, remaining_seconds, "Recovery login attempt on locked account");
            return errors::too_many_requests(
                format!(
                    "Account temporarily locked. Try again in {} minutes.",
                    (remaining_seconds / 60) + 1
                ),
                remaining_seconds,
            );
        }
        Ok(None) => {} // Not locked, continue
        Err(e) => {
            error!(error = %e, "Redis error checking account lockout for recovery login");
            let is_production = crate::config_utils::assume_production();
            if is_production {
                return errors::service_unavailable(
                    "Authentication service temporarily unavailable. Please try again.",
                );
            }
        }
    }

    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let user = match crate::utils::login_timing::verify_credentials(
        &mut conn,
        &login_data.email,
        &login_data.password,
    ) {
        Some(u) => u,
        None => {
            let _ = RateLimiter::record_failed_attempt(
                &redis_url,
                &lockout_key,
                LOCKOUT_DURATION_SECONDS,
            )
            .await;
            return errors::unauthorized("Invalid email or password");
        }
    };

    if let Err(e) = RateLimiter::clear_attempts(&redis_url, &lockout_key).await {
        warn!(error = %e, "Failed to clear login attempts after successful recovery auth");
    }

    // Verify the user actually has passkeys or MFA (this endpoint
    // is for recovery). Same fail-CLOSED rule as the main login
    // gate: propagate the DB lookup error instead of treating it
    // as "no passkeys", otherwise the recovery endpoint could be
    // tricked into accepting credentials on a transient DB blip.
    let has_passkeys = match mfa::user_has_passkeys(&mut conn, &user.uuid) {
        Ok(b) => b,
        Err(e) => {
            error!(error = ?e, user_uuid = %user.uuid, "Failed to check passkey registration during recovery login");
            return errors::internal("Recovery could not complete; please retry");
        }
    };
    if !has_passkeys && !mfa::user_has_mfa_enabled(&user) {
        return errors::bad_request("No MFA configured for this account");
    }

    // Check rate limiting for MFA/recovery attempts
    if !mfa::check_mfa_rate_limit(&user.uuid).await {
        return errors::too_many_requests(
            "Too many recovery attempts. Please try again later.",
            60,
        );
    }

    // Verify recovery code directly (bypasses TOTP check)
    let result =
        match mfa::verify_backup_code(&user.uuid, login_data.recovery_code.trim(), &mut conn).await
        {
            Ok(result) => result,
            Err(_) => {
                mfa::log_mfa_attempt(&user.uuid, false, "recovery_login", &request).await;
                return errors::bad_request("Invalid recovery code");
            }
        };

    if !result.is_valid {
        mfa::log_mfa_attempt(&user.uuid, false, "recovery_login", &request).await;
        return errors::bad_request("Invalid recovery code");
    }

    // Log successful recovery
    mfa::log_mfa_attempt(&user.uuid, true, "recovery_login", &request).await;

    info!(user_uuid = %user.uuid, "Recovery code login successful");

    complete_mfa_login(
        user,
        result.backup_code_used.is_some(),
        result.requires_backup_code_regeneration,
        &request,
        &mut conn,
    )
}

/// Logout endpoint - revokes session from DB and clears cookies
pub async fn logout(db_pool: web::Data<crate::db::Pool>, req: HttpRequest) -> impl Responder {
    use crate::utils::cookies::{
        delete_access_token_cookie, delete_csrf_token_cookie, delete_refresh_token_cookie,
    };

    // Best-effort session revocation — CASCADE handles linked refresh_tokens
    if let (Ok(claims), Ok(mut conn)) = (JwtUtils::extract_claims(&req), helpers::db_conn(&db_pool))
    {
        if let Some(sid) = claims.session_uuid() {
            match crate::repository::active_sessions::revoke_session_by_uuid(&mut conn, &sid) {
                Ok(n) => tracing::info!("Logout: revoked {n} session(s) for sid {sid}"),
                Err(e) => tracing::warn!("Logout: failed to revoke session {sid}: {e}"),
            }
            // W2: record the logout / session revocation. user_uuid
            // resolves from the JWT subject when it parses.
            let user_uuid = Uuid::parse_str(&claims.sub).ok();
            let _ = crate::utils::security_events::record_security_event(
                &mut conn,
                crate::utils::security_events::SecurityEventInput {
                    user_uuid,
                    event_type: "session_revoked",
                    severity: "info",
                    details: Some(json!({ "reason": "logout" })),
                    request: Some(&req),
                    session_id: None,
                },
            );
        }
    }

    HttpResponse::Ok()
        .cookie(delete_access_token_cookie())
        .cookie(delete_refresh_token_cookie())
        .cookie(delete_csrf_token_cookie())
        .json(json!({
            "success": true,
            "message": "Logged out successfully"
        }))
}

pub async fn change_password(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    password_data: web::Json<PasswordChangeRequest>,
) -> impl Responder {
    // Validate new password first
    if password_data.new_password.len() < 8 {
        return errors::bad_request(
            "New password validation failed: Password must be at least 8 characters long",
        );
    } else if password_data.new_password.len() > 128 {
        return errors::bad_request(
            "New password validation failed: Password must be less than 128 characters",
        );
    }

    // Get database connection
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Parse UUID from claims
    let user_uuid = match parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid user UUID in token"),
    };

    match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => {
            // Get current password hash from user_auth_identities
            let current_password_hash = match get_local_password_hash(&user.uuid, &mut conn) {
                Ok(hash) => hash,
                Err(_) => {
                    warn!(user_uuid = %user.uuid, "No local password found for user");
                    return errors::bad_request("No local password found for this user");
                }
            };

            // Verify current password
            let password_matches =
                match verify(&password_data.current_password, &current_password_hash) {
                    Ok(matches) => matches,
                    Err(_) => {
                        error!("Error verifying current password during password change");
                        return errors::internal("Error verifying password");
                    }
                };

            if !password_matches {
                return errors::unauthorized("Current password is incorrect");
            }

            // Check if new password is the same as current password
            if verify(&password_data.new_password, &current_password_hash).unwrap_or(false) {
                return errors::bad_request("New password must be different from current password");
            }

            // Hash the new password
            let new_password_hash = match hash_password(&password_data.new_password) {
                Ok(hash) => hash,
                Err(_) => return errors::internal("Error hashing new password"),
            };

            // Update the user's password hash in user_auth_identities and password_changed_at timestamp in users
            let now = chrono::Utc::now().naive_utc();

            // Update password hash in user_auth_identities
            if let Err(e) = repository::user_auth_identities::update_local_password_hash(
                &mut conn,
                &user.uuid,
                &new_password_hash,
            ) {
                error!(error = ?e, "Error updating password hash");
                return errors::internal("Error updating password");
            }

            // Update password_changed_at timestamp in the audited
            // users table, inside the request's actor context so the
            // audit trigger has a workspace pin.
            let actor = helpers::actor_for(&req, "handler:change_password");
            let update_result = crate::sync::session::with_actor_context(&mut conn, &actor, |c| {
                repository::users::set_password_changed_at(c, &user.uuid, now)
            });
            match update_result {
                Ok(_) => {
                    info!(user_name = %user.name, "Password changed successfully");

                    // Log security event for password change
                    if let Err(e) = log_password_change_event(&user.uuid, &mut conn).await {
                        tracing::warn!("Failed to log password change event: {}", e);
                        // Don't fail the password change if logging fails
                    }

                    // Revoke all other sessions for security (defense in depth)
                    if let Some(claims) = req.extensions().get::<crate::models::Claims>() {
                        if let Some(sid) = claims.session_uuid() {
                            match crate::repository::active_sessions::revoke_other_sessions_by_uuid(
                                &mut conn, &user.uuid, &sid,
                            ) {
                                Ok(n) if n > 0 => info!(revoked_count = n, user_name = %user.name, "Revoked other sessions after password change"),
                                Ok(_) => {},
                                Err(e) => tracing::warn!("Failed to revoke other sessions after password change for user {}: {e}", user.uuid),
                            }
                        }
                    }

                    HttpResponse::Ok().json(json!({
                        "status": "success",
                        "message": "Password changed successfully"
                    }))
                }
                Err(e) => {
                    error!(error = ?e, "Error updating password");
                    errors::internal("Error updating password")
                }
            }
        }
        Err(_) => errors::not_found_msg("User not found"),
    }
}

// Handler to get current authenticated user
pub async fn get_current_user(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return errors::unauthorized("Authentication required"),
    };

    // Parse UUID from claims
    let user_uuid = match uuid::Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid user UUID"),
    };

    // Pin the request's workspace so the user's workspace_role resolves
    // under RLS (workspace_members is workspace-isolated). Without this the
    // /me response carries a null workspace_role in hosted multi-tenant mode.
    helpers::pin_request_workspace(&req, &mut conn);

    // Get user from database using claims
    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => return errors::not_found_msg("User not found"),
    };

    // Flatten user_preferences + primary_email into the response so
    // /me carries the same shape it did before the preferences
    // refactor.
    let mut response =
        crate::repository::user_helpers::get_user_with_primary_email(user, &mut conn);

    // Populate effective_locale / effective_timezone by walking the
    // chain: user pref -> site default -> hardcoded fallback. A
    // failed site_settings read shouldn't sink the response; the
    // resolver treats an empty string as "no site default" and
    // falls through cleanly. site_settings is RLS-enabled
    // (Phase 3c.2); the /me handler is authenticated (auth conn
    // is in scope) but uses helpers::auth_conn which returns a
    // raw pool conn without setting the workspace GUC. Until
    // /me is migrated to TenantConn (a bigger refactor — many
    // other repo calls in this handler share the same conn),
    // read site_settings through with_actor_context with a
    // workspace pin from the request's RequestContext.
    let (default_locale, default_timezone) = {
        let actor = crate::handlers::helpers::actor_for(&req, "handler:auth_me");
        match crate::sync::session::with_actor_context(&mut conn, &actor, |c| {
            crate::repository::site_settings::get_site_settings(c)
        }) {
            Ok(s) => (s.default_locale, s.default_timezone),
            Err(_) => (String::new(), String::new()),
        }
    };
    let eff_locale =
        crate::utils::locale::effective_locale(response.locale.as_deref(), &default_locale);
    let eff_timezone =
        crate::utils::locale::effective_timezone(response.timezone.as_deref(), &default_timezone);
    response.effective_locale = Some(eff_locale.to_string());
    response.effective_timezone = Some(eff_timezone.name().to_string());

    HttpResponse::Ok().json(response)
}

/// Check if system requires initial setup
pub async fn check_setup_status(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
) -> impl Responder {
    // Log access for audit purposes
    let client_ip = crate::utils::client_ip::from_http_request(&req)
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    debug!("Setup status check from IP: {}", client_ip);

    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::count_users(&mut conn) {
        Ok(user_count) => {
            // Check if Microsoft auth is configured via environment variables
            let microsoft_auth_enabled = std::env::var("MICROSOFT_CLIENT_ID").is_ok()
                && std::env::var("MICROSOFT_TENANT_ID").is_ok()
                && std::env::var("MICROSOFT_CLIENT_SECRET").is_ok();

            // Check if OIDC is configured
            let oidc_enabled = crate::config_utils::is_oidc_enabled();
            let oidc_display_name = if oidc_enabled {
                Some(crate::oidc::get_display_name_cached())
            } else {
                None
            };

            // Hosted has no self-serve bootstrap: the control plane
            // provisions admins, so this surface never reports setup as
            // required (and the /setup/admin route isn't mounted). The
            // auth-provider flags are still reported for the login UI.
            let requires_setup = match crate::middleware::DeploymentMode::current() {
                crate::middleware::DeploymentMode::Hosted => false,
                crate::middleware::DeploymentMode::SelfHosted => user_count == 0,
            };

            let response = crate::models::OnboardingStatus {
                requires_setup,
                user_count,
                microsoft_auth_enabled,
                oidc_enabled,
                oidc_display_name,
                local_auth_disabled: hosted_local_auth_disabled(),
            };

            // Security headers now applied globally by SecurityHeaders middleware
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            error!("Error counting users: {:?}", e);
            errors::internal("Failed to check setup status")
        }
    }
}

pub async fn setup_initial_admin(
    req: HttpRequest,
    db_pool: web::Data<crate::db::Pool>,
    search_service: web::Data<std::sync::Arc<crate::services::search::SearchService>>,
    admin_data: web::Json<crate::models::AdminSetupRequest>,
) -> impl Responder {
    // Bootstrap writes a local admin credential via a raw insert that bypasses
    // the repository gate, so refuse here in hosted mode. This is defensive:
    // hosted mints no bootstrap token (bootstrap_token::reconcile), so the
    // token gate below already blocks it.
    if hosted_local_auth_disabled() {
        return errors::forbidden("Initial admin setup is not available in hosted mode");
    }

    // AUD-005: bootstrap-token gate. Network attackers reaching
    // the listener on first boot cannot proceed without the token
    // written to disk at startup. Accept the token via either
    // `Authorization: Bearer <token>` (CLI / scripts) or
    // `X-Bootstrap-Token: <token>` (frontend setup form, which
    // sends the token alongside the eventual user-session bearer
    // and needs a separate header to avoid collision).
    let bearer = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let x_bootstrap = req
        .headers()
        .get("X-Bootstrap-Token")
        .and_then(|h| h.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(provided) = bearer.or(x_bootstrap) else {
        return errors::unauthorized_with_code(
            "Bootstrap token required. Check the server startup logs for the setup URL, or print it with `docker compose exec nosdesk nosdesk-cli setup-token`.",
            "BOOTSTRAP_TOKEN_REQUIRED",
        );
    };
    if let Err(e) = crate::utils::bootstrap_token::verify(provided) {
        warn!(error = %e, "setup_initial_admin: bootstrap token verify failed");
        // Map the verify failure to a machine-readable code the
        // frontend localises (the human strings stay non-enumerable).
        let reason = e.to_string();
        let code = if reason.contains("expired") {
            "BOOTSTRAP_TOKEN_EXPIRED"
        } else if reason.contains("not present") {
            "BOOTSTRAP_TOKEN_NOT_PRESENT"
        } else {
            "BOOTSTRAP_TOKEN_MISMATCH"
        };
        return errors::unauthorized_with_code("Invalid bootstrap token", code);
    }

    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let mut validation_errors = Vec::new();
    let trimmed_name = admin_data.name.trim();
    if trimmed_name.is_empty() {
        validation_errors.push("name: Name is required".to_string());
    } else if trimmed_name.len() > 255 {
        validation_errors.push("name: Name must be less than 255 characters".to_string());
    }
    let trimmed_email = admin_data.email.trim();
    if trimmed_email.is_empty() {
        validation_errors.push("email: Email is required".to_string());
    } else if trimmed_email.len() > 255 {
        validation_errors.push("email: Email must be less than 255 characters".to_string());
    } else if !trimmed_email.contains('@') || !trimmed_email.contains('.') {
        validation_errors.push("email: Invalid email format".to_string());
    }
    if admin_data.password.len() < 8 {
        validation_errors.push("password: Password must be at least 8 characters long".to_string());
    } else if admin_data.password.len() > 128 {
        validation_errors.push("password: Password must be less than 128 characters".to_string());
    }
    if !validation_errors.is_empty() {
        return HttpResponse::BadRequest().json(json!({
            "error": "Validation failed",
            "code": "VALIDATION_FAILED",
            "fields": validation_errors,
        }));
    }

    let password_hash = match hash_password(&admin_data.password) {
        Ok(hash) => hash,
        Err(e) => {
            error!(error = ?e, "Error hashing password");
            return errors::internal("Error processing password");
        }
    };

    let (created_user, user_email) = match crate::services::admin_setup::create_initial_admin(
        &mut conn,
        crate::services::admin_setup::InitialAdminInput {
            name: &admin_data.name,
            email: &admin_data.email,
            password_hash: &password_hash,
        },
    ) {
        Ok(v) => v,
        Err(crate::services::admin_setup::AdminSetupError::AlreadyComplete) => {
            // 410 Gone, not 400: setup is a one-shot endpoint that has
            // been consumed. Gone tells the client this is permanent and
            // not worth retrying (the frontend must not auto-redirect on
            // it).
            return errors::gone_with_code(
                "Setup has already been completed; this endpoint is no longer accepting requests.",
                "SETUP_COMPLETE",
            );
        }
        Err(crate::services::admin_setup::AdminSetupError::DuplicateEmail) => {
            // 409 Conflict, not 500: a taken email is a client-correctable
            // state, not a server fault.
            return errors::conflict_with_code("Email already in use", "EMAIL_TAKEN");
        }
        Err(crate::services::admin_setup::AdminSetupError::Db(e)) => {
            error!(error = ?e, "Error creating admin user");
            return errors::internal_with_code("Internal error during setup", "INTERNAL_ERROR");
        }
    };

    info!(user_name = %created_user.name, "Initial admin user created successfully");

    // Bootstrap token has done its job; consuming the file is the
    // belt to the count check's braces.
    crate::utils::bootstrap_token::consume();

    // First-admin provenance (workspace-local usage log, not phone-home
    // telemetry): record who created the bootstrap admin and from where
    // so a reviewer can later answer "who set this instance up?".
    let client_ip = crate::utils::client_ip::from_http_request(&req)
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let user_agent = req
        .headers()
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");
    info!(
        event = "bootstrap_admin_created",
        user_uuid = %created_user.uuid,
        email = %user_email.email,
        client_ip = %client_ip,
        user_agent = %user_agent,
        "bootstrap admin created"
    );

    // Search indexing happens post-commit so a rolled-back insert
    // never leaves an orphan document in tantivy.
    search_service
        .index_user(&created_user, Some(user_email.email.as_str()))
        .ok();

    // Default ticket categories are seeded inside create_initial_admin's
    // transaction (so they inherit the bootstrap actor context); nothing
    // to do here.

    // Pin the bootstrap workspace so the response's workspace_role resolves
    // under RLS. create_initial_admin set the membership inside its own
    // transaction, which reset the connection GUC on commit; this is always
    // the bootstrap workspace (the only one that exists at setup time).
    {
        use diesel::prelude::*;
        let _ = diesel::sql_query("SELECT set_config('app.workspace_id', $1, false) AS set_config")
            .bind::<diesel::sql_types::Text, _>(
                crate::sync::actor::BOOTSTRAP_WORKSPACE_ID.to_string(),
            )
            .execute(&mut conn);
    }

    let response = crate::models::AdminSetupResponse {
        success: true,
        message: "Initial admin user created successfully".to_string(),
        user: Some(repository::user_helpers::get_user_with_primary_email(
            created_user,
            &mut conn,
        )),
    };
    HttpResponse::Created().json(response)
}

// === MFA (Multi-Factor Authentication) Handlers ===

/// MFA Setup - Generate secret and QR code
pub async fn mfa_setup(db_pool: web::Data<crate::db::Pool>, req: HttpRequest) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    let user_uuid = match parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid user UUID"),
    };

    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => return errors::not_found_msg("User not found"),
    };

    // (Encryption-key configuration is now a boot-time invariant —
    // `init_keyring()` in main.rs refuses to start without MFA_KEK_V1
    // so there's no per-request check to do here.)

    // Generate new TOTP secret (backup codes will be generated after successful verification)
    let secret = mfa::generate_totp_secret();

    // Pin server-side so the matching mfa_enable call refuses to
    // honour a substituted secret. See utils::mfa::stash_setup_secret
    // for the threat model (attacker-with-password substitutes their
    // own TOTP secret + matching code).
    if let Err(e) = mfa::stash_setup_secret(&user.uuid, secret.as_str()).await {
        tracing::error!(user_uuid = %user.uuid, error = %e, "Failed to stash MFA setup secret");
        return errors::internal("Failed to initialise MFA setup");
    }

    // Get user's primary email for QR code
    let user_email = repository::user_helpers::get_primary_email(&user.uuid, &mut conn)
        .unwrap_or_else(|| format!("user-{}", user.uuid));

    // Generate QR code
    let qr_result = match mfa::generate_qr_code(secret.as_str(), &user_email, "Nosdesk") {
        Ok(result) => result,
        Err(e) => {
            tracing::error!("Failed to generate QR code: {}", e);
            return errors::internal("Failed to generate QR code");
        }
    };

    tracing::info!("MFA setup initiated for user: {}", user.uuid);

    let response = crate::models::MfaSetupResponse {
        secret: secret.as_str().to_string(),
        qr_code: qr_result.svg_data_url,
        // Do not generate or return backup codes until after verification completes
        backup_codes: vec![],
        qr_matrix: Some(qr_result.matrix),
    };

    // The body carries the TOTP secret; keep it out of any shared or
    // disk cache (proxies, the browser bfcache, devtools history).
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(response)
}

/// MFA Verify Setup - Verify the TOTP token during setup
pub async fn mfa_verify_setup(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    request: web::Json<crate::models::MfaVerifySetupRequest>,
) -> impl Responder {
    let _conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Verify the TOTP token (timing-safe verification)
    if !mfa::verify_totp_token(&request.secret, &request.token) {
        tracing::warn!("MFA setup verification failed for user: {}", claims.sub);
        return errors::bad_request("Invalid verification code");
    }

    tracing::info!("MFA setup verification successful for user: {}", claims.sub);

    // Return success - backup codes will be generated after enabling
    let response = crate::models::MfaVerifySetupResponse {
        success: true,
        backup_codes: vec![],
    };

    HttpResponse::Ok().json(response)
}

/// MFA Enable - Enable MFA for the user
pub async fn mfa_enable(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    request: web::Json<crate::models::MfaEnableRequest>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Parse UUID from claims
    let user_uuid = match parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid user UUID in token"),
    };

    tracing::info!("Enabling MFA for user: {}", user_uuid);

    // Pull the secret from the server-side setup cache. See
    // utils::mfa::stash_setup_secret for the threat model — request
    // bodies on this endpoint are not trusted to carry the TOTP
    // secret. An absent cache entry means setup expired (TTL ~10min)
    // or was never initiated; user is sent back to mfa_setup.
    let stashed_secret = match mfa::fetch_setup_secret(&user_uuid).await {
        Some(s) => s,
        None => {
            return errors::bad_request(
                "MFA setup expired or was not initiated. Please start setup again.",
            )
        }
    };
    let mfa_secret = stashed_secret.as_str();

    // Per-account rate limit on enrollment TOTP attempts (shares the
    // bucket with login MFA attempts). Five tries in the window, then
    // the user must wait, so brute-forcing the 6-digit code during
    // enrollment isn't viable.
    if !mfa::check_mfa_rate_limit(&user_uuid).await {
        return errors::too_many_requests("Too many MFA attempts. Please try again later.", 60);
    }

    // Generate backup codes on the server after successful verification

    // Final TOTP verification before enabling
    if !mfa::verify_totp_token(mfa_secret, &request.token) {
        tracing::warn!("MFA enable verification failed for user: {}", user_uuid);
        return errors::bad_request("Invalid verification code");
    }

    // Encrypt the MFA secret before storage. Returns the framed blob
    // plus the sidecar kek_id we mirror onto `mfa_secret_kek_id`.
    let (encrypted_secret, kek_id) = match mfa::encrypt_mfa_secret(mfa_secret, &user_uuid) {
        Ok(pair) => pair,
        Err(e) => {
            tracing::error!("Failed to encrypt MFA secret: {}", e);
            return errors::internal("Failed to secure MFA data");
        }
    };

    // Generate backup codes now that verification succeeded.
    // bcrypt hashing happens off the request thread via
    // spawn_blocking; the plaintext set is returned to the client
    // once for them to record.
    let (backup_codes_plaintext, backup_codes_hashed) = mfa::generate_backup_codes_async().await;

    let mfa_update = crate::models::UserMfaUpdate {
        mfa_enabled: Some(true),
        mfa_secret: Some(Some(encrypted_secret)),
        mfa_secret_kek_id: Some(Some(kek_id)),
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    // Two writes in one actor-scoped transaction: replace the
    // recovery-codes set in `user_recovery_codes`, then enable MFA on
    // the audited `users` table. The actor pins `app.workspace_id`
    // (from the request's resolved WorkspaceContext) so the users
    // audit trigger has a workspace; the shared transaction makes the
    // pair atomic, so a failed users update rolls the codes back too
    // (no MFA-active-with-zero-codes lockout window, no manual
    // rollback needed).
    let actor = helpers::actor_for(&req, "handler:mfa_enable");
    let result = crate::sync::session::with_actor_context(&mut conn, &actor, |c| {
        repository::user_recovery_codes::replace_all(c, &user_uuid, backup_codes_hashed)?;
        repository::update_user_mfa(&user_uuid, mfa_update, c)?;
        Ok::<(), diesel::result::Error>(())
    });

    match result {
        Ok(()) => {
            tracing::info!("MFA enabled successfully for user: {}", user_uuid);
            // Clear the rate-limit bucket so any fumbled enrollment
            // attempts don't follow the user into their next login.
            mfa::clear_mfa_rate_limit(&user_uuid).await;
            // Drop the setup secret cache entry — its job is done
            // and the persisted users.mfa_secret is authoritative.
            mfa::consume_setup_secret(&user_uuid).await;
            // Return plaintext backup codes so the client can display them once
            HttpResponse::Ok().json(json!({
                "status": "success",
                "message": "MFA enabled successfully",
                // &* derefs through `Zeroizing<Vec<String>>` to
                // `&Vec<String>` for serde. The Zeroizing wrapper
                // wipes the source allocation when this handler
                // returns/unwinds.
                "backup_codes": &*backup_codes_plaintext,
            }))
        }
        Err(e) => {
            tracing::error!("Failed to enable MFA in database: {:?}", e);
            errors::internal("Failed to enable MFA")
        }
    }
}

/// MFA Disable - Disable MFA for the user (requires full scope)
pub async fn mfa_disable(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    request: web::Json<crate::models::MfaDisableRequest>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Verify scope - must be "full" (MFA disable requires being fully authenticated)
    if claims.scope != "full" {
        return errors::forbidden("This action requires a full session");
    }

    // Parse UUID from claims
    let user_uuid = match parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid user UUID in token"),
    };

    // Get user
    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => return errors::not_found_msg("User not found"),
    };

    // Always verify password for MFA disable (even with recovery token)
    // This prevents account takeover if recovery email is compromised
    let password_hash_str = match get_local_password_hash(&user.uuid, &mut conn) {
        Ok(hash) => hash,
        Err(_) => return errors::internal("Error reading password hash"),
    };

    if !verify(&request.password, &password_hash_str).unwrap_or(false) {
        return errors::bad_request("Invalid password");
    }

    // Both columns are cleared together; the CHECK constraint
    // `(mfa_secret IS NULL) = (mfa_secret_kek_id IS NULL)` requires
    // they move in lockstep. `Some(None)` is the AsChangeset idiom for
    // "set this nullable column to NULL"; `None` would mean "leave it".
    let mfa_update = crate::models::UserMfaUpdate {
        mfa_enabled: Some(false),
        mfa_secret: Some(None),
        mfa_secret_kek_id: Some(None),
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    // Disable MFA: wipe recovery codes, then clear the secret + flip
    // the flag on the audited `users` table, both in one actor-scoped
    // transaction. The actor pins `app.workspace_id` for the users
    // audit trigger; the shared transaction makes the pair atomic.
    let actor = helpers::actor_for(&req, "handler:mfa_disable");
    let result = crate::sync::session::with_actor_context(&mut conn, &actor, |c| {
        repository::user_recovery_codes::delete_all_for_user(c, &user_uuid)?;
        repository::update_user_mfa(&user_uuid, mfa_update, c)?;
        Ok::<(), diesel::result::Error>(())
    });

    match result {
        Ok(()) => {
            tracing::info!(
                "MFA disabled for user: {} (scope: {})",
                user_uuid,
                claims.scope
            );
            HttpResponse::Ok().json(json!({
                "status": "success",
                "message": "MFA disabled successfully"
            }))
        }
        Err(e) => {
            error!(error = ?e, "Error disabling MFA");
            errors::internal("Failed to disable MFA")
        }
    }
}

/// MFA Regenerate Backup Codes - Generate new backup codes
pub async fn mfa_regenerate_backup_codes(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    request: web::Json<crate::models::MfaRegenerateBackupCodesRequest>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Parse UUID from claims
    let user_uuid = match parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid user UUID in token"),
    };

    // Get user to verify password
    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => return errors::not_found_msg("User not found"),
    };

    // Verify password
    let password_hash_str = match get_local_password_hash(&user.uuid, &mut conn) {
        Ok(hash) => hash,
        Err(_) => return errors::internal("Error reading password hash"),
    };

    if !verify(&request.password, &password_hash_str).unwrap_or(false) {
        return errors::bad_request("Invalid password");
    }

    // Generate new backup codes. The replace_all repo call is
    // a single transaction (delete-all + bulk-insert), so the
    // user is never left mid-rotation with a partial set.
    let (backup_codes_plaintext, backup_codes_hashed) = mfa::generate_backup_codes_async().await;

    // `user_recovery_codes` isn't audited today, but wrap the write in
    // the actor context anyway so it stays correct if a trigger is
    // ever attached (and so the sync_actions attribution is right).
    let actor = helpers::actor_for(&req, "handler:mfa_regenerate_backup_codes");
    let result = crate::sync::session::with_actor_context(&mut conn, &actor, |c| {
        repository::user_recovery_codes::replace_all(c, &user_uuid, backup_codes_hashed)
    });

    match result {
        Ok(_) => {
            // Inline the response rather than building the typed
            // struct so we can pass `&*backup_codes_plaintext`
            // through serde without first cloning into a
            // non-Zeroizing Vec — the source allocation gets
            // wiped on handler exit. Wire shape identical to
            // `MfaRegenerateBackupCodesResponse { backup_codes }`.
            HttpResponse::Ok().json(json!({
                "backup_codes": &*backup_codes_plaintext,
            }))
        }
        Err(e) => {
            error!(error = ?e, "Error regenerating backup codes");
            errors::internal("Failed to regenerate backup codes")
        }
    }
}

/// MFA Status - Get current MFA status for the user
pub async fn mfa_status(db_pool: web::Data<crate::db::Pool>, req: HttpRequest) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return errors::unauthorized("Authentication required"),
    };

    // Parse UUID from claims
    let user_uuid = match parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid user UUID in token"),
    };

    // Get user MFA status
    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => return errors::not_found_msg("User not found"),
    };

    // Check if user has any unused recovery codes left. Indexed
    // count via the partial index on (user_uuid) WHERE used_at
    // IS NULL.
    let has_backup_codes = repository::user_recovery_codes::count_unused(&mut conn, &user_uuid)
        .map(|n| n > 0)
        .unwrap_or(false);

    let response = crate::models::MfaStatusResponse {
        enabled: user.mfa_enabled,
        has_backup_codes,
    };

    HttpResponse::Ok().json(response)
}

/// MFA Setup for Login (Unauthenticated) - For users who need MFA to login but haven't set it up yet
pub async fn mfa_setup_login(
    db_pool: web::Data<crate::db::Pool>,
    body: web::Json<crate::models::MfaSetupLoginRequest>,
    http_request: HttpRequest,
) -> impl Responder {
    let redis_url = get_redis_url();
    let email_lower = body.email.to_lowercase();
    let client_ip = crate::utils::client_ip::from_http_request(&http_request);
    let lockout_key = RateLimiter::login_attempt_key(&email_lower, client_ip);

    // Check if account is locked before any validation
    match RateLimiter::check_lockout(&redis_url, &lockout_key, MAX_LOGIN_ATTEMPTS).await {
        Ok(Some(remaining_seconds)) => {
            warn!(email = %email_lower, remaining_seconds, "MFA setup attempt on locked account");
            return errors::too_many_requests(
                format!(
                    "Account temporarily locked. Try again in {} seconds.",
                    remaining_seconds
                ),
                remaining_seconds,
            );
        }
        Ok(None) => {} // Not locked, continue
        Err(e) => {
            warn!("Failed to check account lockout: {:?}", e);
        }
    }

    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Pin the request's workspace so the MFA policy gate resolves the
    // caller's role under RLS (the pool clears app.workspace_id on checkout).
    helpers::pin_request_workspace(&http_request, &mut conn);

    let user = match crate::utils::login_timing::verify_credentials(
        &mut conn,
        &email_lower,
        &body.password,
    ) {
        Some(u) => u,
        None => {
            match RateLimiter::record_failed_attempt(
                &redis_url,
                &lockout_key,
                LOCKOUT_DURATION_SECONDS,
            )
            .await
            {
                Ok(attempts) => {
                    let remaining = MAX_LOGIN_ATTEMPTS.saturating_sub(attempts);
                    if remaining == 0 {
                        warn!(email = %email_lower, "Account locked after failed MFA setup attempts");
                    }
                }
                Err(e) => warn!("Failed to record failed attempt: {:?}", e),
            }
            return errors::unauthorized("Invalid email or password");
        }
    };

    if let Err(e) = RateLimiter::clear_attempts(&redis_url, &lockout_key).await {
        warn!(error = %e, "Failed to clear login attempts after successful MFA setup auth");
    }

    // Verify that user actually needs MFA setup (security check)
    if mfa::user_has_mfa_enabled(&user) {
        return errors::bad_request("MFA is already enabled for this account");
    }

    // Verify that MFA is required for this user
    if mfa::validate_mfa_policy(&user, &mut conn).await.is_ok() {
        return errors::bad_request("MFA is not required for this account");
    }

    // Encryption-key configuration is a boot-time invariant; nothing to
    // check here.

    // Generate new TOTP secret (backup codes will be generated after verification)
    let secret = mfa::generate_totp_secret();

    // Pin the secret server-side for the matching mfa_enable_login
    // call. The client still receives the secret in the response so
    // it can render the QR; the server-side stash is what makes the
    // enroll path refuse to honour a secret the user didn't actually
    // see, closing the attacker-with-password substitution vector.
    if let Err(e) = mfa::stash_setup_secret(&user.uuid, secret.as_str()).await {
        tracing::error!(user_uuid = %user.uuid, error = %e, "Failed to stash MFA setup secret");
        return errors::internal("Failed to initialise MFA setup");
    }

    // Get user's primary email for QR code
    let user_email = repository::user_helpers::get_primary_email(&user.uuid, &mut conn)
        .unwrap_or_else(|| format!("user-{}", user.uuid));

    // Generate QR code
    let qr_result = match mfa::generate_qr_code(secret.as_str(), &user_email, "Nosdesk") {
        Ok(result) => result,
        Err(e) => {
            tracing::error!("Failed to generate QR code: {}", e);
            return errors::internal("Failed to generate QR code");
        }
    };

    tracing::info!("MFA setup initiated for user during login: {}", user.uuid);

    let response = crate::models::MfaSetupResponse {
        secret: secret.as_str().to_string(),
        qr_code: qr_result.svg_data_url,
        backup_codes: vec![],
        qr_matrix: Some(qr_result.matrix),
    };

    // The body carries the TOTP secret; keep it out of any cache.
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(response)
}

/// MFA Enable for Login (Unauthenticated) - Complete MFA setup and login
pub async fn mfa_enable_login(
    db_pool: web::Data<crate::db::Pool>,
    request: web::Json<crate::models::MfaEnableLoginRequest>,
    http_request: HttpRequest,
) -> impl Responder {
    let redis_url = get_redis_url();
    let email_lower = request.email.to_lowercase();
    let client_ip = crate::utils::client_ip::from_http_request(&http_request);
    let lockout_key = RateLimiter::login_attempt_key(&email_lower, client_ip);

    // Check if account is locked before any validation
    match RateLimiter::check_lockout(&redis_url, &lockout_key, MAX_LOGIN_ATTEMPTS).await {
        Ok(Some(remaining_seconds)) => {
            warn!(email = %email_lower, remaining_seconds, "MFA enable attempt on locked account");
            return errors::too_many_requests(
                format!(
                    "Account temporarily locked. Try again in {} seconds.",
                    remaining_seconds
                ),
                remaining_seconds,
            );
        }
        Ok(None) => {} // Not locked, continue
        Err(e) => {
            warn!("Failed to check account lockout: {:?}", e);
        }
    }

    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    // Pin the request's workspace so the MFA policy gate resolves the
    // caller's role under RLS (the pool clears app.workspace_id on checkout).
    helpers::pin_request_workspace(&http_request, &mut conn);

    let user = match crate::utils::login_timing::verify_credentials(
        &mut conn,
        &email_lower,
        &request.password,
    ) {
        Some(u) => u,
        None => {
            match RateLimiter::record_failed_attempt(
                &redis_url,
                &lockout_key,
                LOCKOUT_DURATION_SECONDS,
            )
            .await
            {
                Ok(attempts) => {
                    let remaining = MAX_LOGIN_ATTEMPTS.saturating_sub(attempts);
                    if remaining == 0 {
                        warn!(email = %email_lower, "Account locked after failed MFA enable attempts");
                    }
                }
                Err(e) => warn!("Failed to record failed attempt: {:?}", e),
            }
            return errors::unauthorized("Invalid email or password");
        }
    };

    if let Err(e) = RateLimiter::clear_attempts(&redis_url, &lockout_key).await {
        warn!(error = %e, "Failed to clear login attempts after successful MFA enable auth");
    }

    // Security checks - same as setup
    if mfa::user_has_mfa_enabled(&user) {
        return errors::bad_request("MFA is already enabled for this account");
    }

    if mfa::validate_mfa_policy(&user, &mut conn).await.is_ok() {
        return errors::bad_request("MFA is not required for this account");
    }

    // Pull the secret from the server-side setup cache, NOT from
    // the request body. The previous design honoured request.secret
    // and let an attacker who knew the victim's password substitute
    // their own attacker-controlled TOTP secret, enrolling the
    // attacker's authenticator on the victim's account. The cache
    // entry was minted by `mfa_setup_login` and is bound to this
    // user uuid; if it's absent the user is asked to restart setup.
    let stashed_secret = match mfa::fetch_setup_secret(&user.uuid).await {
        Some(s) => s,
        None => {
            return errors::bad_request(
                "MFA setup expired or was not initiated. Please start setup again.",
            )
        }
    };
    let mfa_secret = stashed_secret.as_str();

    // Per-account rate limit on enrollment TOTP attempts (same bucket
    // as login MFA), so the 6-digit code can't be brute-forced here
    // either.
    if !mfa::check_mfa_rate_limit(&user.uuid).await {
        return errors::too_many_requests("Too many MFA attempts. Please try again later.", 60);
    }

    // Backup codes are generated after verification, not required in request

    // Final TOTP verification before enabling
    if !mfa::verify_totp_token(mfa_secret, &request.token) {
        tracing::warn!(
            "MFA enable verification failed for user during login: {}",
            user.uuid
        );
        return errors::bad_request("Invalid verification code");
    }

    // Encrypt the MFA secret before storage
    let (encrypted_secret, kek_id) = match mfa::encrypt_mfa_secret(mfa_secret, &user.uuid) {
        Ok(pair) => pair,
        Err(e) => {
            tracing::error!("Failed to encrypt MFA secret: {}", e);
            return errors::internal("Failed to secure MFA data");
        }
    };

    // Generate backup codes now that verification succeeded
    let (backup_codes_plaintext, backup_codes_hashed) = mfa::generate_backup_codes_async().await;

    // Enable MFA in database. This is a pre-session flow (the user
    // has verified credentials but has no JWT / RequestContext yet),
    // so resolve the audit workspace from the user's primary
    // membership and build the actor explicitly.
    let user_uuid = user.uuid;

    let mfa_update = crate::models::UserMfaUpdate {
        mfa_enabled: Some(true),
        mfa_secret: Some(Some(encrypted_secret)),
        mfa_secret_kek_id: Some(Some(kek_id)),
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    let workspace_id =
        match crate::repository::workspaces::primary_workspace_for_user(&mut conn, user_uuid) {
            Ok(ws) => ws,
            Err(e) => {
                tracing::error!(
                    "Failed to resolve primary workspace for MFA enable: {:?}",
                    e
                );
                return errors::internal("Failed to enable MFA");
            }
        };
    let actor = crate::sync::actor::ActorContext::user_at_workspace(user_uuid, workspace_id);
    // Recovery codes write goes first, then the audited `users`
    // update, both in one actor-scoped transaction so the pair is
    // atomic and the users audit trigger sees the workspace GUC.
    let result = crate::sync::session::with_actor_context(&mut conn, &actor, |c| {
        repository::user_recovery_codes::replace_all(c, &user_uuid, backup_codes_hashed)?;
        repository::update_user_mfa(&user_uuid, mfa_update, c)?;
        Ok::<(), diesel::result::Error>(())
    });

    match result {
        Ok(()) => {
            tracing::info!(
                "MFA enabled successfully for user during login: {}",
                user_uuid
            );
            // Drop the enrollment rate-limit bucket — a successful
            // enrol shouldn't penalise the user's next login
            // attempt with attempts they've already cleared.
            mfa::clear_mfa_rate_limit(&user_uuid).await;
            // The setup secret has done its job; remove the cache
            // entry so a subsequent stale enable_login attempt
            // can't reuse it (defence in depth — the user.mfa_secret
            // column is now the authoritative copy).
            mfa::consume_setup_secret(&user_uuid).await;

            // Create session + tokens (same as login, but attach backup codes).
            // Pin the workspace so the response's workspace_role resolves
            // under RLS (workspace_members is workspace-isolated).
            helpers::pin_request_workspace(&http_request, &mut conn);
            let session = match create_session_record(&user_uuid, &http_request, &mut conn) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!(
                        "Failed to create session for MFA enable login {}: {}",
                        user_uuid,
                        e
                    );
                    return errors::internal("Failed to create authentication session");
                }
            };
            let family_id = Uuid::new_v4();

            match jwt_helpers::create_login_response(
                user,
                &session.session_id,
                &family_id,
                &mut conn,
            ) {
                Ok((mut response, tokens)) => {
                    // Clone the inner Vec out of the Zeroizing
                    // wrapper to satisfy the response struct's
                    // field type. The clone is no worse than
                    // serde's implicit clone during serialisation;
                    // the source allocation still gets wiped via
                    // the wrapper's Drop on handler exit.
                    response.backup_codes = Some((*backup_codes_plaintext).clone());
                    build_auth_response(&http_request, response, &tokens)
                }
                Err(error_response) => error_response,
            }
        }
        Err(e) => {
            tracing::error!("Failed to enable MFA in database: {:?}", e);
            errors::internal("Failed to enable MFA")
        }
    }
}

// === SESSION MANAGEMENT HANDLERS ===

/// Get all active sessions for the current user
pub async fn get_user_sessions(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return errors::unauthorized("Authentication required"),
    };

    // Parse UUID from claims
    let user_uuid = match parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid user UUID in token"),
    };

    let current_sid = claims.session_uuid();

    // Get all sessions for the user
    let sessions =
        match crate::repository::active_sessions::get_user_sessions(&mut conn, &user_uuid) {
            Ok(sessions) => sessions,
            Err(e) => {
                tracing::error!("Failed to get user sessions: {}", e);
                return errors::internal("Failed to retrieve sessions");
            }
        };

    // Convert sessions to response format
    let session_responses: Vec<serde_json::Value> = sessions
        .into_iter()
        .map(|session| {
            let is_current = current_sid == Some(session.session_id);
            json!({
                "id": session.id,
                "session_id": session.session_id.to_string(),
                "device_name": session.device_name,
                "ip_address": session.ip_address.map(|ip| ip.to_string()),
                "user_agent": session.user_agent,
                "location": session.location,
                "created_at": session.created_at,
                "last_active": session.last_active,
                "expires_at": session.expires_at,
                "is_current": is_current
            })
        })
        .collect();

    HttpResponse::Ok().json(json!({
        "status": "success",
        "sessions": session_responses
    }))
}

/// Revoke a specific session
pub async fn revoke_session(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<i32>,
) -> impl Responder {
    let session_id = path.into_inner();

    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return errors::unauthorized("Authentication required"),
    };

    // Parse UUID from claims
    let user_uuid = match parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid user UUID in token"),
    };

    // Verify the session belongs to this user before revoking. Return
    // 404 (not 403) when the row exists but isn't theirs, so the response
    // can't be used to probe for another user's session ids.
    match crate::repository::active_sessions::get_session_by_id(&mut conn, session_id) {
        Ok(session) if session.user_uuid == user_uuid => {
            // Session belongs to user, proceed with revocation
        }
        _ => return errors::not_found_msg("Session not found"),
    }

    // Revoke the session. A count of 0 means it was already gone (e.g. a
    // double-click race); that's the desired end state, so the delete is
    // idempotent and still reports success.
    match crate::repository::active_sessions::revoke_session(&mut conn, session_id) {
        Ok(_) => {
            tracing::info!("Session {} revoked for user {}", session_id, user_uuid);
            let _ = crate::utils::security_events::record_security_event(
                &mut conn,
                crate::utils::security_events::SecurityEventInput {
                    user_uuid: Some(user_uuid),
                    event_type: "session_revoked",
                    severity: "info",
                    details: Some(json!({
                        "reason": "manual_revoke",
                        "revoked_session_id": session_id
                    })),
                    request: Some(&req),
                    session_id: None,
                },
            );
            HttpResponse::Ok().json(json!({
                "status": "success",
                "message": "Session revoked successfully"
            }))
        }
        Err(e) => {
            tracing::error!("Failed to revoke session: {}", e);
            errors::internal("Failed to revoke session")
        }
    }
}

/// Revoke all other sessions (keep current session active)
pub async fn revoke_all_other_sessions(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    body: web::Json<crate::models::RevokeOtherSessionsRequest>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let claims = match JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return errors::unauthorized("Authentication required"),
    };

    // Parse UUID from claims
    let user_uuid = match parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid user UUID in token"),
    };

    // Step-up re-auth. Signing out every other device is a high-blast-
    // radius, classic post-compromise action, so require a full (non-MFA-
    // pending) session plus a fresh credential, mirroring mfa_disable.
    if claims.scope != "full" {
        return errors::forbidden("This action requires a full session");
    }

    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(u) => u,
        Err(_) => return errors::not_found_msg("User not found"),
    };
    // Fetch the local password hash once (Err for OAuth-only accounts).
    let local_hash = get_local_password_hash(&user.uuid, &mut conn).ok();

    let reauthenticated = if let Some(pw) = body.password.as_deref().filter(|p| !p.is_empty()) {
        // Verify the local password (the mfa_disable precedent).
        local_hash
            .as_deref()
            .map(|hash| verify(pw, hash).unwrap_or(false))
            .unwrap_or(false)
    } else if let Some(code) = body
        .mfa_code
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        // Verify a TOTP or backup code against the stored secret.
        user.mfa_enabled
            && mfa::verify_mfa_token(&user.uuid, code, &mut conn)
                .await
                .map(|r| r.is_valid)
                .unwrap_or(false)
    } else {
        // No credential supplied: only acceptable when the account has
        // nothing to step up with (no local password and no MFA, e.g. an
        // OAuth-only account without MFA). Otherwise it's required.
        local_hash.is_none() && !user.mfa_enabled
    };

    if !reauthenticated {
        return errors::bad_request("Re-authentication required to sign out all other sessions");
    }

    // Revoke all other sessions (if we can't identify current session, revoke everything)
    let revoke_result = match claims.session_uuid() {
        Some(sid) => crate::repository::active_sessions::revoke_other_sessions_by_uuid(
            &mut conn, &user_uuid, &sid,
        ),
        None => {
            crate::repository::active_sessions::revoke_other_sessions(&mut conn, &user_uuid, None)
        }
    };

    match revoke_result {
        Ok(revoked_count) => {
            tracing::info!(
                "Revoked {} other session(s) for user {}",
                revoked_count,
                user_uuid
            );
            let _ = crate::utils::security_events::record_security_event(
                &mut conn,
                crate::utils::security_events::SecurityEventInput {
                    user_uuid: Some(user_uuid),
                    event_type: "session_revoked",
                    severity: "info",
                    details: Some(json!({
                        "reason": "revoke_all_others",
                        "revoked_count": revoked_count
                    })),
                    request: Some(&req),
                    session_id: None,
                },
            );
            HttpResponse::Ok().json(json!({
                "status": "success",
                "message": format!("Successfully revoked {} session(s)", revoked_count),
                "revoked_count": revoked_count
            }))
        }
        Err(e) => {
            tracing::error!("Failed to revoke other sessions: {}", e);
            errors::internal("Failed to revoke sessions")
        }
    }
}

// === TOKEN REFRESH HANDLER ===

/// Refresh access token using refresh token (with reuse detection + grace period)
pub async fn refresh_token(
    db_pool: web::Data<crate::db::Pool>,
    // Optional so web clients, which POST an empty body and carry the refresh
    // token in the httpOnly cookie, still bind.
    body: Option<web::Json<crate::models::RefreshRequest>>,
    request: HttpRequest,
) -> impl Responder {
    use crate::utils::auth_mode::{auth_mode_from_request, AuthMode};

    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // 1. Source the refresh token: native clients send it in the body, web
    //    clients in the httpOnly cookie. Reply in body-token (bearer) mode when
    //    the token came from the body or the client asked via X-Auth-Mode, so a
    //    native caller never gets Set-Cookie back.
    let from_body = body
        .as_ref()
        .and_then(|b| b.refresh_token.clone())
        .filter(|t| !t.is_empty());
    let bearer_mode = from_body.is_some() || auth_mode_from_request(&request) == AuthMode::Bearer;
    let refresh_raw = match from_body.or_else(|| {
        request
            .cookie(crate::utils::cookies::REFRESH_TOKEN_COOKIE)
            .map(|c| c.value().to_string())
    }) {
        Some(token) => token,
        None => {
            return errors::unauthorized("Refresh token not found");
        }
    };

    let token_hash = JwtUtils::hash_refresh_token(&refresh_raw);

    let old_token = match crate::repository::refresh_tokens::get_refresh_token_by_hash(
        &mut conn,
        &token_hash,
    ) {
        Ok(token) => token,
        Err(_) => {
            return errors::unauthorized("Invalid or expired refresh token");
        }
    };

    // 2. Check if revoked
    if old_token.revoked_at.is_some() {
        tracing::warn!(
            "Revoked refresh token presented, family={}",
            old_token.family_id
        );
        return errors::unauthorized("Refresh token has been revoked");
    }

    // 3. Reuse detection
    if old_token.is_used {
        let now = chrono::Utc::now().naive_utc();
        let within_grace = old_token.grace_expires_at.is_some_and(|grace| grace > now);

        if !within_grace {
            // Token reuse outside grace period — potential theft!
            tracing::warn!(
                "Refresh token reuse detected outside grace period! Revoking family={}",
                old_token.family_id
            );
            let _ = crate::repository::refresh_tokens::revoke_token_family(
                &mut conn,
                &old_token.family_id,
            );
            if let Some(sid) = old_token.session_id {
                let _ = crate::repository::active_sessions::revoke_session_by_uuid(&mut conn, &sid);
            }
            return errors::unauthorized("Token reuse detected — session revoked for security");
        }
        // Within grace period — allow (concurrent tab scenario)
        tracing::debug!(
            "Refresh token reuse within grace period, family={}",
            old_token.family_id
        );
    }

    // 4. Get user
    let user = match repository::get_user_by_uuid(&old_token.user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            return errors::unauthorized("User not found");
        }
    };

    // 5. Determine session_id (from token, or create new session for tokens without one)
    let session_id = match old_token.session_id {
        Some(sid) => sid,
        None => {
            // Token created before this migration — create a new session
            match create_session_record(&user.uuid, &request, &mut conn) {
                Ok(session) => session.session_id,
                Err(e) => {
                    tracing::error!("Failed to create session during refresh: {}", e);
                    return errors::internal("Failed to create session");
                }
            }
        }
    };

    // 6. Generate new access JWT with same sid
    let new_access_token = match JwtUtils::create_token(&user, &session_id) {
        Ok(token) => token,
        Err(_) => {
            return errors::internal("Failed to create access token");
        }
    };

    // 7. Generate new refresh token
    let new_refresh_raw = JwtUtils::generate_refresh_token();
    let new_refresh_hash = JwtUtils::hash_refresh_token(&new_refresh_raw);

    // 8. Mark old token used (if not already)
    if !old_token.is_used {
        let grace_until = chrono::Utc::now().naive_utc() + chrono::Duration::seconds(5);
        if let Err(e) = crate::repository::refresh_tokens::mark_token_used(
            &mut conn,
            &token_hash,
            &new_refresh_hash,
            grace_until,
        ) {
            tracing::error!("Failed to mark old refresh token as used: {}", e);
        }
    }

    // 9. Create new refresh token with same family_id and session_id
    let new_refresh_expires = chrono::Utc::now().naive_utc() + chrono::Duration::days(7);
    let new_refresh_record = crate::models::NewRefreshToken {
        token_hash: new_refresh_hash,
        user_uuid: user.uuid,
        expires_at: new_refresh_expires,
        session_id: Some(session_id),
        family_id: old_token.family_id,
    };

    if let Err(e) =
        crate::repository::refresh_tokens::create_refresh_token(&mut conn, new_refresh_record)
    {
        tracing::error!("Failed to store new refresh token: {}", e);
        return errors::internal("Failed to create refresh token");
    }

    // 10. Update session activity
    let new_session_expires = chrono::Utc::now().naive_utc() + chrono::Duration::days(7);
    if let Err(e) = crate::repository::active_sessions::update_session_activity(
        &mut conn,
        &session_id,
        new_session_expires,
    ) {
        tracing::warn!("Failed to update session activity: {}", e);
    }

    // 11. Return new tokens, the way the client asked for them.
    let new_csrf_token = crate::utils::csrf::generate_csrf_token();

    if bearer_mode {
        // Native: rotated tokens in the body, no cookies, kept out of caches.
        let response = crate::models::RefreshTokenResponse {
            success: true,
            csrf_token: new_csrf_token,
            access_token: Some(new_access_token),
            refresh_token: Some(new_refresh_raw),
        };
        return HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .json(response);
    }

    // Web: rotated tokens in httpOnly cookies, only CSRF in the body (unchanged).
    let response = crate::models::RefreshTokenResponse {
        success: true,
        csrf_token: new_csrf_token.clone(),
        access_token: None,
        refresh_token: None,
    };

    HttpResponse::Ok()
        .cookie(crate::utils::cookies::create_access_token_cookie(
            &new_access_token,
        ))
        .cookie(crate::utils::cookies::create_refresh_token_cookie(
            &new_refresh_raw,
        ))
        .cookie(crate::utils::cookies::create_csrf_token_cookie(
            &new_csrf_token,
        ))
        .json(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{create_test_claims, setup_test_pool, TestFixtures};
    use actix_web::{http::StatusCode, test, App};

    /// Helper to create a test app with the public auth routes used by
    /// the tests below.
    fn test_app(
        pool: crate::db::Pool,
    ) -> App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        App::new()
            .app_data(web::Data::new(pool))
            .route("/setup/status", web::get().to(check_setup_status))
            .route("/login", web::post().to(login))
            .route("/me", web::get().to(get_current_user))
    }

    // =========================================================================
    // PUBLIC ENDPOINT TESTS (no auth required)
    // =========================================================================

    #[actix_web::test]
    async fn check_setup_status_returns_ok() {
        let pool = setup_test_pool();
        let app = test::init_service(test_app(pool)).await;

        let req = test::TestRequest::get().uri("/setup/status").to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::OK);

        // Verify response structure
        let body = test::read_body(resp).await;
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json.get("requires_setup").is_some());
        assert!(json.get("user_count").is_some());
        assert!(json.get("microsoft_auth_enabled").is_some());
        assert!(json.get("oidc_enabled").is_some());
    }

    #[actix_web::test]
    async fn login_with_invalid_credentials_fails() {
        let pool = setup_test_pool();
        let app = test::init_service(test_app(pool)).await;

        // Use a unique email per run to avoid triggering the Redis rate limiter
        // across repeated test invocations.
        let unique_email = format!("nonexistent_{}@example.com", uuid::Uuid::new_v4());

        let login_request = serde_json::json!({
            "email": unique_email,
            "password": "wrongpassword"
        });

        let req = test::TestRequest::post()
            .uri("/login")
            .set_json(&login_request)
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let body = test::read_body(resp).await;
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json.get("error").and_then(|v| v.as_str()).is_some());
        assert_eq!(
            json.get("code").and_then(|v| v.as_str()),
            Some("AUTH_REQUIRED")
        );
    }

    // =========================================================================
    // PROTECTED ENDPOINT TESTS (auth required)
    // =========================================================================

    #[actix_web::test]
    async fn get_current_user_requires_auth() {
        let pool = setup_test_pool();
        let app = test::init_service(test_app(pool)).await;

        // Request without authentication
        let req = test::TestRequest::get().uri("/me").to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let body = test::read_body(resp).await;
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json.get("error").and_then(|v| v.as_str()),
            Some("Authentication required")
        );
        assert_eq!(
            json.get("code").and_then(|v| v.as_str()),
            Some("AUTH_REQUIRED")
        );
    }

    #[actix_web::test]
    async fn get_current_user_with_auth_succeeds() {
        let pool = setup_test_pool();
        let (user_uuid, claims) = {
            let mut conn = pool.get().unwrap();
            let user = TestFixtures::create_user(&mut conn, "authuser", "user");
            let claims = create_test_claims(&user);
            (user.uuid, claims)
        }; // conn dropped here

        let app = test::init_service(test_app(pool.clone())).await;

        let req = test::TestRequest::get().uri("/me").to_request();
        req.extensions_mut().insert(claims);

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = test::read_body(resp).await;
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json.get("uuid").and_then(|v| v.as_str()),
            Some(user_uuid.to_string().as_str())
        );
        assert_eq!(json.get("name").and_then(|v| v.as_str()), Some("authuser"));
    }
}

/// Serde invariants for bearer-mode tokens. Kept in a separate module so the
/// standard `#[test]` attribute isn't shadowed by the `actix_web::test` import
/// in the main `tests` module above. No DB needed.
#[cfg(test)]
mod bearer_serde_tests {
    /// Web (cookie) flow leaves the token fields `None`; they must then be
    /// omitted from the JSON entirely so the response is byte-identical to
    /// before bearer support. A regression here leaks session tokens into the
    /// web body, defeating the httpOnly cookies.
    #[test]
    fn login_response_omits_token_fields_when_none() {
        let response = crate::models::LoginResponse {
            success: true,
            mfa_required: Some(false),
            mfa_setup_required: Some(false),
            passkey_mfa_required: None,
            user_uuid: Some("u".into()),
            csrf_token: Some("csrf".into()),
            user: None,
            message: None,
            mfa_backup_code_used: None,
            requires_backup_code_regeneration: None,
            backup_codes: None,
            access_token: None,
            refresh_token: None,
        };
        let json = serde_json::to_value(&response).unwrap();
        assert!(
            json.get("access_token").is_none(),
            "web login leaked access_token"
        );
        assert!(
            json.get("refresh_token").is_none(),
            "web login leaked refresh_token"
        );
        assert_eq!(
            json.get("csrf_token").and_then(|v| v.as_str()),
            Some("csrf")
        );
    }

    /// Bearer mode surfaces both rotated tokens in the refresh body.
    #[test]
    fn refresh_response_includes_tokens_in_bearer_mode() {
        let web = crate::models::RefreshTokenResponse {
            success: true,
            csrf_token: "csrf".into(),
            access_token: None,
            refresh_token: None,
        };
        let web_json = serde_json::to_value(&web).unwrap();
        assert!(web_json.get("access_token").is_none());
        assert!(web_json.get("refresh_token").is_none());

        let native = crate::models::RefreshTokenResponse {
            success: true,
            csrf_token: "csrf".into(),
            access_token: Some("at".into()),
            refresh_token: Some("rt".into()),
        };
        let native_json = serde_json::to_value(&native).unwrap();
        assert_eq!(
            native_json.get("access_token").and_then(|v| v.as_str()),
            Some("at")
        );
        assert_eq!(
            native_json.get("refresh_token").and_then(|v| v.as_str()),
            Some("rt")
        );
    }
}
