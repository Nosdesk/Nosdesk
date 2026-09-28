//! `/api/public/email/stop`: someone asks a workspace to stop emailing an
//! address (the link in mail sent to addresses nobody has confirmed). GET shows
//! a page with a button, so a mail scanner following the link changes nothing;
//! POST (the button, or a mail client's RFC 8058 one-click) adds the address to
//! the workspace's suppression list.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use serde::Deserialize;

use crate::db::Pool;
use crate::extractors::WorkspaceContext;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/email/stop", web::get().to(landing))
        .route("/email/stop", web::post().to(stop));
}

#[derive(Deserialize)]
pub struct StopQuery {
    t: String,
}

fn page(title: &str, body: &str) -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .insert_header(("Cache-Control", "no-store"))
        .body(format!(
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{title}</title></head>\
<body style=\"font-family:system-ui,sans-serif;max-width:32rem;margin:4rem auto;padding:0 1rem;line-height:1.5\">\
<h1 style=\"font-size:1.4rem\">{title}</h1>{body}</body></html>"
        ))
}

fn invalid() -> HttpResponse {
    page(
        "This link doesn't work",
        "<p>It may have been copied incompletely. Try the link in the email again.</p>",
    )
}

pub async fn landing(req: HttpRequest, query: web::Query<StopQuery>) -> HttpResponse {
    let Some(ws) = req.extensions().get::<WorkspaceContext>().cloned() else {
        return invalid();
    };
    if crate::utils::email_stop_link::verify(ws.workspace_id, &query.t).is_none() {
        return invalid();
    }
    // The token is URL-safe base64 and a dot, so it needs no escaping.
    page(
        "Stop these emails?",
        &format!(
            "<p>We'll stop sending email to this address about requests you didn't make.</p>\
<form method=\"post\" action=\"/api/public/email/stop?t={t}\">\
<button type=\"submit\" style=\"font:inherit;padding:.5rem 1rem;border-radius:.5rem;border:1px solid #888;cursor:pointer\">Stop these emails</button>\
</form>",
            t = query.t
        ),
    )
}

pub async fn stop(
    req: HttpRequest,
    query: web::Query<StopQuery>,
    pool: web::Data<Pool>,
) -> HttpResponse {
    let Some(ws) = req.extensions().get::<WorkspaceContext>().cloned() else {
        return invalid();
    };
    let Some(email) = crate::utils::email_stop_link::verify(ws.workspace_id, &query.t) else {
        return invalid();
    };
    let workspace_id = ws.workspace_id;
    let saved = crate::sync::session::run_in_workspace(
        &pool,
        "background:email_stop",
        workspace_id,
        |conn| {
            crate::repository::email_suppressions::upsert(
                conn,
                crate::models::NewEmailSuppression {
                    email: email.clone(),
                    reason: crate::models::email_suppression_reason::REQUESTED.to_string(),
                    bounce_diagnostic: None,
                    workspace_id,
                },
            )
        },
    );
    if let Err(e) = saved {
        tracing::error!(error = ?e, "email stop: suppression failed");
        return crate::errors::internal("Couldn't record that. Try again in a moment.");
    }
    page(
        "Done",
        "<p>This address won't get these emails any more.</p>",
    )
}
