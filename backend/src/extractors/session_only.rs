//! `SessionOnly`: the requirement an account handler carries. Accounts are
//! shared across workspaces, and an API token is one workspace's automation,
//! so the routes that read or change an account (its addresses, sign-in
//! identities, credentials, avatar, preferences, push devices, the workspaces
//! it belongs to) answer only a signed-in session. Taking this extractor is
//! the whole rule: a token is refused before the handler runs.

use std::future::{ready, Ready};

use actix_web::{dev::Payload, FromRequest, HttpMessage, HttpRequest};

use crate::middleware::api_token::ApiTokenRequest;

/// What a token is told on an account route.
pub const SESSION_ONLY: &str = "API tokens can't manage accounts. Sign in to do this.";

/// A request made with a signed-in session, not an API token.
pub struct SessionOnly;

impl FromRequest for SessionOnly {
    type Error = actix_web::Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        ready(match refuse_api_token(req) {
            Ok(()) => Ok(SessionOnly),
            Err(e) => Err(e),
        })
    }
}

/// Refuse an API token on an account route, for a handler whose rule depends
/// on what it is asked to change (its own account, say).
pub fn refuse_api_token(req: &HttpRequest) -> Result<(), actix_web::Error> {
    if req.extensions().get::<ApiTokenRequest>().is_some() {
        return Err(crate::errors::forbidden_error(SESSION_ONLY));
    }
    Ok(())
}

/// Whether the request is an API token made by someone other than its
/// holder (an admin's automation acting as them).
pub fn is_token_made_for_someone_else(req: &HttpRequest) -> bool {
    req.extensions()
        .get::<ApiTokenRequest>()
        .is_some_and(|t| t.made_for_someone_else)
}

/// Whether the request is made with an API token.
pub fn is_api_token(req: &HttpRequest) -> bool {
    req.extensions().get::<ApiTokenRequest>().is_some()
}
