//! `.well-known` endpoints for mobile deep-linking (iOS Universal Links +
//! Android App Links).
//!
//! Served unauthenticated on every tenant origin (`<slug>.nosdesk.app`,
//! `<slug>.nosdesk.dev`, ...), ahead of the SPA catch-all, so a scanned ticket
//! QR (`https://<slug>.nosdesk.app/tickets/<id>`) opens the app if installed.
//! Because all tenant subdomains are served by this one backend, a single
//! host-agnostic handler covers every tenant.
//!
//! Both files must be plain JSON over HTTPS with **no redirect** — Apple and
//! Google fetch them directly and reject a redirected or non-JSON response.
//! `.json()` sets `Content-Type: application/json`, which is what both expect.

use actix_web::{HttpResponse, Responder};
use serde_json::json;

const DEFAULT_IOS_APP_ID: &str = "S6DDZ86325.com.nosdesk.app";
const DEFAULT_ANDROID_PACKAGE: &str = "com.nosdesk.app";

/// `GET /.well-known/apple-app-site-association` — claims the ticket paths for
/// the iOS app. `appIDs` = `<TEAM_ID>.<BUNDLE_ID>`; the two components cover
/// host mode (`/tickets/:id`) and path mode (`/<slug>/tickets/:id`). Overridable
/// via `NOSDESK_IOS_APP_ID` (same app for staging + prod, so it rarely changes).
pub async fn apple_app_site_association() -> impl Responder {
    let app_id =
        std::env::var("NOSDESK_IOS_APP_ID").unwrap_or_else(|_| DEFAULT_IOS_APP_ID.to_string());
    HttpResponse::Ok().json(json!({
        "applinks": {
            "details": [
                {
                    "appIDs": [app_id],
                    "components": [
                        { "/": "/tickets/*", "comment": "Ticket detail (host mode)" },
                        { "/": "/*/tickets/*", "comment": "Ticket detail (path mode, workspace-slug prefix)" }
                    ]
                }
            ]
        }
    }))
}

/// `GET /.well-known/assetlinks.json` — Android App Links statement. The release
/// signing cert's SHA-256 fingerprint(s) come from `NOSDESK_ANDROID_CERT_SHA256`
/// (comma-separated). Until that is set, the statement lists no fingerprints and
/// Android App Links stay unverified (iOS is unaffected); the route still exists
/// so setting the env var is all that's needed to light Android up.
pub async fn assetlinks() -> impl Responder {
    let package = std::env::var("NOSDESK_ANDROID_PACKAGE")
        .unwrap_or_else(|_| DEFAULT_ANDROID_PACKAGE.to_string());
    let fingerprints: Vec<String> = std::env::var("NOSDESK_ANDROID_CERT_SHA256")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    HttpResponse::Ok().json(json!([
        {
            "relation": ["delegate_permission/common.handle_all_urls"],
            "target": {
                "namespace": "android_app",
                "package_name": package,
                "sha256_cert_fingerprints": fingerprints
            }
        }
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::http::header::CONTENT_TYPE;
    use actix_web::{test, web, App};

    #[actix_web::test]
    async fn aasa_is_json_claiming_ticket_paths() {
        let app = test::init_service(App::new().route(
            "/.well-known/apple-app-site-association",
            web::get().to(apple_app_site_association),
        ))
        .await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/.well-known/apple-app-site-association")
                .to_request(),
        )
        .await;
        assert!(resp.status().is_success());
        // Apple rejects a non-JSON content type.
        assert_eq!(
            resp.headers().get(CONTENT_TYPE).unwrap(),
            "application/json"
        );
        let body: serde_json::Value = test::read_body_json(resp).await;
        let details = &body["applinks"]["details"][0];
        assert!(details["appIDs"][0]
            .as_str()
            .unwrap()
            .ends_with("com.nosdesk.app"));
        let paths: Vec<&str> = details["components"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["/"].as_str().unwrap())
            .collect();
        assert!(paths.contains(&"/tickets/*"));
        assert!(paths.contains(&"/*/tickets/*"));
    }

    #[actix_web::test]
    async fn assetlinks_is_android_statement() {
        let app = test::init_service(
            App::new().route("/.well-known/assetlinks.json", web::get().to(assetlinks)),
        )
        .await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/.well-known/assetlinks.json")
                .to_request(),
        )
        .await;
        assert!(resp.status().is_success());
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body[0]["target"]["namespace"], "android_app");
        assert_eq!(body[0]["target"]["package_name"], "com.nosdesk.app");
        assert_eq!(
            body[0]["relation"][0],
            "delegate_permission/common.handle_all_urls"
        );
    }
}
