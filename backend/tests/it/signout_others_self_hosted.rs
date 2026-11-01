//! Self-hosted: "sign out all other sessions" ends the helpdesk's own sessions
//! and makes no control-plane call; the response carries no
//! `remembered_sign_ins_ended`. Hosted is `tests/hosted_signout_others.rs`.

use actix_web::dev::Service;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::models::{Claims, NewActiveSession};

fn session(conn: &mut backend::db::DbConnection, user: Uuid) -> Uuid {
    backend::repository::active_sessions::create_session(
        conn,
        NewActiveSession {
            user_uuid: user,
            device_name: None,
            ip_address: None,
            user_agent: None,
            location: None,
            expires_at: (chrono::Utc::now() + chrono::Duration::hours(1)).naive_utc(),
            oidc_id_token: None,
            confirmed_at: None,
            provider_amr: None,
        },
    )
    .expect("session")
    .session_id
}

#[actix_web::test]
async fn self_hosted_sign_out_of_the_others_makes_no_control_plane_call() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let conn = &mut pool.get().expect("conn");
    let user = crate::common::insert_plain_user(conn, "Agent");
    let here = session(conn, user);
    session(conn, user);
    session(conn, user);

    let claims = Claims {
        sub: user.to_string(),
        name: "Agent".to_string(),
        email: String::new(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: Some(here.to_string()),
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(claims.clone());
                srv.call(req)
            })
            .route(
                "/auth/sessions/others",
                web::delete().to(backend::handlers::revoke_all_other_sessions),
            ),
    )
    .await;
    let sign_out_others = || {
        http_test::TestRequest::delete()
            .uri("/auth/sessions/others")
            .set_json(json!({}))
            .to_request()
    };
    let resp = http_test::call_service(&app, sign_out_others()).await;
    assert_eq!(resp.status(), 400, "an unconfirmed session is refused");

    backend::repository::active_sessions::record_confirmation(
        conn,
        &here,
        &user,
        chrono::Utc::now().naive_utc(),
        None,
    )
    .expect("confirm");
    let resp = http_test::call_service(&app, sign_out_others()).await;
    assert_eq!(resp.status(), 200);
    let body: Value = http_test::read_body_json(resp).await;
    assert_eq!(body["revoked_count"], 2);
    assert!(body.get("remembered_sign_ins_ended").is_none());

    use backend::schema::active_sessions;
    let left: Vec<Uuid> = active_sessions::table
        .filter(active_sessions::user_uuid.eq(user))
        .select(active_sessions::session_id)
        .load(conn)
        .expect("sessions");
    assert_eq!(left, vec![here]);
}
