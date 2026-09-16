//! Customer-portal session primitive: establishment + the authorization gate.
//!
//! The portal is a separate principal realm. These tests exercise the two
//! security-critical primitives directly (the way `workspace_selection_resolution`
//! exercises the agent gate), independent of the actix middleware that will
//! later wrap them:
//!
//! - `establish_portal_session` sets the three portal cookies.
//! - `authorize_portal_request` admits a portal token only when it is
//!   portal-scoped, bound to the origin's workspace, and the subject is a
//!   member; every other case is a uniform 403.

#![allow(clippy::expect_used)]

use actix_web::test::TestRequest;
use actix_web::HttpMessage as _;
use actix_web::ResponseError as _;

use backend::extractors::WorkspaceContext;
use backend::handlers::portal::{authorize_portal_request, establish_portal_session};
use backend::middleware::cookie_auth::PORTAL_SCOPE;
use backend::models::Claims;
use backend::repository::workspaces::{add_membership, find_by_id, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

mod common;

fn context_of(conn: &mut backend::db::DbConnection, workspace_id: i32) -> WorkspaceContext {
    let ws = find_by_id(conn, workspace_id)
        .expect("workspace lookup")
        .expect("workspace exists");
    WorkspaceContext {
        workspace_id: ws.id,
        workspace_uuid: ws.uuid,
        slug: ws.slug,
        name: ws.name,
        custom_domain: ws.custom_domain,
        organisation_id: ws.organisation_id,
    }
}

/// Portal-scope claims bound to `workspace_uuid` (or `None` to omit the
/// binding), with the given scope so we can also exercise a non-portal token.
fn portal_claims(user_uuid: uuid::Uuid, scope: &str, workspace_uuid: Option<uuid::Uuid>) -> Claims {
    Claims {
        sub: user_uuid.to_string(),
        name: "Customer".to_string(),
        email: String::new(),
        platform_role: "user".to_string(),
        scope: scope.to_string(),
        sid: Some(uuid::Uuid::new_v4().to_string()),
        workspace_uuid,
        exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    }
}

fn status_of(err: &actix_web::Error) -> u16 {
    err.as_response_error().status_code().as_u16()
}

#[test]
fn portal_session_establishment_and_gate() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(2);

    // acme (the customer's workspace) and other (a different tenant). The
    // customer is a baseline Member of acme only.
    let (acme_id, other_id, customer, stranger_uuid) = {
        let mut conn = pool.get().expect("conn");
        let acme = common::mint_workspace(&mut conn, "acme-portal", "Acme Portal");
        let other = common::mint_workspace(&mut conn, "other-portal", "Other Portal");
        let customer = common::insert_user(&mut conn, "Portal Customer");
        let stranger = common::insert_user(&mut conn, "Portal Stranger");
        (acme, other, customer, stranger.uuid)
    };
    {
        let mut conn = pool.get().expect("conn");
        let actor = ActorContext::user(customer.uuid, None).with_workspace(acme_id);
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            add_membership(
                c,
                acme_id,
                customer.uuid,
                "member",
                SeatWriteAuthority::ControlPlane,
            )?;
            Ok(())
        })
        .expect("add acme membership");
    }

    let acme = {
        let mut conn = pool.get().expect("conn");
        context_of(&mut conn, acme_id)
    };

    // --- establish_portal_session sets the three portal cookies ---
    {
        let mut conn = pool.get().expect("conn");
        let http_req = TestRequest::default().to_http_request();
        let resp = establish_portal_session(&customer, acme.workspace_uuid, &http_req, &mut conn)
            .expect("establishing a portal session succeeds");
        let names: Vec<String> = resp.cookies().map(|c| c.name().to_string()).collect();
        for expected in ["portal_access", "portal_refresh", "portal_csrf"] {
            assert!(
                names.iter().any(|n| n == expected),
                "missing portal cookie {expected}; got {names:?}"
            );
        }
    }

    // --- authorize: member with a portal token bound to the origin workspace ---
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default().to_srv_request();
        req.extensions_mut().insert(acme.clone());
        let claims = portal_claims(customer.uuid, PORTAL_SCOPE, Some(acme.workspace_uuid));
        let ctx = authorize_portal_request(&req, &mut conn, &claims)
            .expect("member on a matching origin must be authorized");
        assert_eq!(ctx.user_uuid, customer.uuid);
        assert_eq!(ctx.workspace_id, acme_id);
    }

    // --- binding mismatch: token bound to acme, origin serves other ---
    {
        let mut conn = pool.get().expect("conn");
        let other = context_of(&mut pool.get().expect("conn"), other_id);
        let req = TestRequest::default().to_srv_request();
        req.extensions_mut().insert(other);
        let claims = portal_claims(customer.uuid, PORTAL_SCOPE, Some(acme.workspace_uuid));
        let err = authorize_portal_request(&req, &mut conn, &claims)
            .expect_err("a token bound to another tenant must be denied");
        assert_eq!(status_of(&err), 403, "binding mismatch must be 403");
    }

    // --- non-member: portal token for the right origin, but not a member ---
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default().to_srv_request();
        req.extensions_mut().insert(acme.clone());
        let claims = portal_claims(stranger_uuid, PORTAL_SCOPE, Some(acme.workspace_uuid));
        let err = authorize_portal_request(&req, &mut conn, &claims)
            .expect_err("a non-member must be denied");
        assert_eq!(status_of(&err), 403, "non-member must be 403");
    }

    // --- non-portal token is refused by the portal gate ---
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default().to_srv_request();
        req.extensions_mut().insert(acme.clone());
        let claims = portal_claims(customer.uuid, "full", Some(acme.workspace_uuid));
        let err = authorize_portal_request(&req, &mut conn, &claims)
            .expect_err("a non-portal token must not act as a customer");
        assert_eq!(status_of(&err), 403, "non-portal scope must be 403");
    }

    // --- portal token with no workspace binding is refused ---
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default().to_srv_request();
        req.extensions_mut().insert(acme.clone());
        let claims = portal_claims(customer.uuid, PORTAL_SCOPE, None);
        let err = authorize_portal_request(&req, &mut conn, &claims)
            .expect_err("an unbound portal token must be denied");
        assert_eq!(status_of(&err), 403, "unbound portal token must be 403");
    }

    // --- magic-link token is single-use and resolves to the customer ---
    {
        use backend::utils::reset_tokens::{ResetTokenUtils, TokenType};
        let mut conn = pool.get().expect("conn");
        let token = ResetTokenUtils::create_reset_token(customer.uuid, TokenType::PortalMagicLink);
        backend::repository::reset_tokens::create_reset_token(
            &mut conn,
            &token.token_hash,
            customer.uuid,
            TokenType::PortalMagicLink.as_str(),
            None,
            None,
            token.expires_at,
            None,
        )
        .expect("issue magic-link token");

        let resolved = backend::repository::reset_tokens::validate_and_consume_token(
            &mut conn,
            &token.raw_token,
            TokenType::PortalMagicLink.as_str(),
        )
        .expect("a fresh magic-link token resolves to its user");
        assert_eq!(resolved, customer.uuid, "token resolves to the customer");

        // Single-use: a second consume of the same token fails.
        backend::repository::reset_tokens::validate_and_consume_token(
            &mut conn,
            &token.raw_token,
            TokenType::PortalMagicLink.as_str(),
        )
        .expect_err("a consumed magic-link token cannot be reused");
    }
}

/// A portal refresh token must not be exchangeable for an agent session.
///
/// Portal sign-in mints a portal-scoped ACCESS token but a generic refresh
/// token, stored in the same `refresh_tokens` table the agent app uses. Before
/// the `audience` column existed the two were byte-identical, so
/// `POST /api/auth/refresh` would look a portal token up by hash and mint a
/// full staff session. The refresh cookie is Path-scoped to the portal endpoint,
/// but that handler also reads the token from the request body, so a customer
/// could copy their own cookie out of devtools and escalate.
///
/// This pins the property the handler's equality check depends on: what portal
/// sign-in writes is not what the agent endpoint accepts.
#[test]
fn portal_refresh_tokens_are_not_agent_credentials() {
    use backend::models::{REFRESH_AUDIENCE_AGENT, REFRESH_AUDIENCE_PORTAL};
    use backend::schema::refresh_tokens;
    use diesel::prelude::*;

    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(2);

    let (workspace_id, customer) = {
        let mut conn = pool.get().expect("conn");
        let ws = common::mint_workspace(&mut conn, "acme-realm", "Acme Realm");
        let customer = common::insert_user(&mut conn, "Realm Customer");
        (ws, customer)
    };
    {
        let mut conn = pool.get().expect("conn");
        let actor = ActorContext::user(customer.uuid, None).with_workspace(workspace_id);
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            add_membership(
                c,
                workspace_id,
                customer.uuid,
                "member",
                SeatWriteAuthority::ControlPlane,
            )?;
            Ok(())
        })
        .expect("add membership");
    }

    let workspace_uuid = {
        let mut conn = pool.get().expect("conn");
        context_of(&mut conn, workspace_id).workspace_uuid
    };

    {
        let mut conn = pool.get().expect("conn");
        let http_req = TestRequest::default().to_http_request();
        establish_portal_session(&customer, workspace_uuid, &http_req, &mut conn)
            .expect("establishing a portal session succeeds");
    }

    let stored: Vec<String> = {
        let mut conn = pool.get().expect("conn");
        refresh_tokens::table
            .filter(refresh_tokens::user_uuid.eq(customer.uuid))
            .select(refresh_tokens::audience)
            .load(&mut conn)
            .expect("load refresh tokens")
    };

    assert!(
        !stored.is_empty(),
        "portal sign-in should store a refresh token"
    );
    for audience in &stored {
        assert_eq!(
            audience, REFRESH_AUDIENCE_PORTAL,
            "portal sign-in must mark its refresh token as portal-realm"
        );
        // The agent refresh endpoint accepts only an exact match on the agent
        // realm, so this inequality is what stops the exchange.
        assert_ne!(
            audience, REFRESH_AUDIENCE_AGENT,
            "a portal token must never satisfy the agent endpoint's realm check"
        );
    }
}

/// Both refresh endpoints share one rotation core, so the realm check is
/// symmetric: neither realm's token works at the other's endpoint.
///
/// The portal route exists now (its cookie was always `Path`-scoped to it), so
/// portal sessions rotate instead of dying at the 15-minute access cookie. This
/// asserts the property that makes adding that route safe: the audience stored
/// at sign-in is the portal realm, and the agent endpoint accepts only its own.
#[test]
fn the_two_realms_mint_distinguishable_refresh_tokens() {
    use backend::models::{REFRESH_AUDIENCE_AGENT, REFRESH_AUDIENCE_PORTAL};
    use backend::schema::refresh_tokens;
    use diesel::prelude::*;

    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(2);

    let (workspace_id, customer) = {
        let mut conn = pool.get().expect("conn");
        let ws = common::mint_workspace(&mut conn, "acme-symmetry", "Acme Symmetry");
        let customer = common::insert_user(&mut conn, "Symmetry Customer");
        (ws, customer)
    };
    {
        let mut conn = pool.get().expect("conn");
        let actor = ActorContext::user(customer.uuid, None).with_workspace(workspace_id);
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            add_membership(
                c,
                workspace_id,
                customer.uuid,
                "member",
                SeatWriteAuthority::ControlPlane,
            )?;
            Ok(())
        })
        .expect("add membership");
    }

    let workspace_uuid = {
        let mut conn = pool.get().expect("conn");
        context_of(&mut conn, workspace_id).workspace_uuid
    };
    {
        let mut conn = pool.get().expect("conn");
        let http_req = TestRequest::default().to_http_request();
        establish_portal_session(&customer, workspace_uuid, &http_req, &mut conn)
            .expect("portal session");
    }

    let audiences: Vec<String> = {
        let mut conn = pool.get().expect("conn");
        refresh_tokens::table
            .filter(refresh_tokens::user_uuid.eq(customer.uuid))
            .select(refresh_tokens::audience)
            .load(&mut conn)
            .expect("load")
    };

    assert!(!audiences.is_empty());
    assert!(
        audiences.iter().all(|a| a == REFRESH_AUDIENCE_PORTAL),
        "portal sign-in must mint portal-realm refresh tokens, got {audiences:?}"
    );
    assert!(
        audiences.iter().all(|a| a != REFRESH_AUDIENCE_AGENT),
        "and none of them may satisfy the agent endpoint"
    );
}

/// A portal refresh cookie is a bearer credential. Replayed at another
/// tenant's origin it must not mint a session there, and it must not be spent
/// in the attempt: the refusal has to land before the family rotates, or a
/// customer's own cookie could be burned by anyone who copies it.
#[test]
fn a_portal_refresh_cookie_is_refused_at_another_tenants_origin() {
    use actix_web::cookie::Cookie;
    use backend::handlers::portal::refresh_portal_session;
    use backend::schema::refresh_tokens;
    use backend::utils::cookies::PORTAL_REFRESH_TOKEN_COOKIE;
    use diesel::prelude::*;

    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(2);

    let (home, foreign, customer) = {
        let mut conn = pool.get().expect("conn");
        let home = common::mint_workspace(&mut conn, "acme-home", "Acme Home");
        let foreign = common::mint_workspace(&mut conn, "acme-foreign", "Acme Foreign");
        let customer = common::insert_user(&mut conn, "Replay Customer");
        (home, foreign, customer)
    };
    {
        let mut conn = pool.get().expect("conn");
        let actor = ActorContext::user(customer.uuid, None).with_workspace(home);
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            add_membership(
                c,
                home,
                customer.uuid,
                "member",
                SeatWriteAuthority::ControlPlane,
            )?;
            Ok(())
        })
        .expect("add membership");
    }

    // Sign in at the home workspace and keep the refresh cookie it hands back.
    let refresh_raw = {
        let mut conn = pool.get().expect("conn");
        let home_uuid = context_of(&mut conn, home).workspace_uuid;
        let http_req = TestRequest::default().to_http_request();
        let response = establish_portal_session(&customer, home_uuid, &http_req, &mut conn)
            .expect("portal session");
        response
            .cookies()
            .find(|c| c.name() == PORTAL_REFRESH_TOKEN_COOKIE)
            .expect("refresh cookie")
            .value()
            .to_string()
    };

    // Present it at the foreign workspace's origin, where the customer is not
    // a member.
    let foreign_ctx = {
        let mut conn = pool.get().expect("conn");
        context_of(&mut conn, foreign)
    };
    let request = TestRequest::default()
        .cookie(Cookie::new(
            backend::utils::cookies::cookie_name(PORTAL_REFRESH_TOKEN_COOKIE),
            refresh_raw.clone(),
        ))
        .to_http_request();
    request.extensions_mut().insert(foreign_ctx);

    let response = actix_web::rt::System::new()
        .block_on(refresh_portal_session(
            actix_web::web::Data::new(pool.clone()),
            request,
        ))
        .unwrap_or_else(|e| e.error_response());
    assert_eq!(
        response.status().as_u16(),
        401,
        "a non-member must not be handed a session for this workspace"
    );

    let spent: Vec<bool> = {
        let mut conn = pool.get().expect("conn");
        refresh_tokens::table
            .filter(refresh_tokens::user_uuid.eq(customer.uuid))
            .select(refresh_tokens::is_used)
            .load(&mut conn)
            .expect("load")
    };
    assert_eq!(spent.len(), 1, "the refusal must not have minted anything");
    assert!(
        !spent[0],
        "and it must not have consumed the customer's own credential"
    );
}
