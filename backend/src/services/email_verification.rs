//! Address confirmation for emails added to a user profile.
//!
//! An address on a profile is a claim until something proves it. The proof
//! here is the same one the portal sign-in link uses: possession of a link
//! sent to the address. The consequence is much smaller, since clicking it
//! verifies one `user_emails` row rather than signing anyone in.
//!
//! Why the claim needs proving at all: `is_verified` is the inbound-mail
//! impersonation guard (`user_helpers::find_verified_user_by_email`), so an
//! unproven address that counted as verified would let its claimant receive
//! attribution for mail the real owner sent. Before this existed the flag was
//! settable from a request body, which is to say it asserted nothing.
//!
//! The token carries the `user_emails` row id in its `metadata`, not just the
//! user: `reset_tokens` is user-scoped and one user may have several unverified
//! addresses at once, so "which address did they confirm" is not derivable from
//! the token's subject.

use serde_json::json;
use tracing::{error, info};

use crate::models::{User, UserEmail};
use crate::utils::reset_tokens::{ResetTokenUtils, TokenType};

/// Mint a confirmation token for `email` and enqueue the email to it.
///
/// Best-effort by design: the caller has already added the address, and a mail
/// failure should not undo that or fail their request. Failures are logged and
/// the user can resend.
pub fn send_verification(
    conn: &mut crate::db::DbConnection,
    user: &User,
    email: &UserEmail,
    workspace_id: i32,
    ip_address: Option<&str>,
    user_agent: Option<&str>,
) {
    if email.is_verified {
        return;
    }

    let token = ResetTokenUtils::generate_token();
    let token_hash = ResetTokenUtils::hash_token(&token);
    let token_type = TokenType::EmailVerification;
    let expires_at = chrono::Utc::now() + token_type.expiration_duration();

    if let Err(e) = crate::repository::reset_tokens::create_reset_token(
        conn,
        &token_hash,
        user.uuid,
        token_type.as_str(),
        ip_address,
        user_agent,
        expires_at,
        // The row id is what redemption acts on; the address is recorded beside
        // it so a token cannot be replayed against a row that has since been
        // pointed at a different address.
        Some(json!({ "user_email_id": email.id, "address": email.email })),
    ) {
        error!(user_uuid = %user.uuid, error = ?e, "email verification: could not mint token");
        return;
    }

    let workspace = match crate::repository::workspaces::find_by_id(conn, workspace_id) {
        Ok(Some(w)) => w,
        _ => {
            error!(user_uuid = %user.uuid, workspace_id, "email verification: workspace not found");
            return;
        }
    };

    // Same rule as the password-reset link, and for the same reason: never
    // derive the link host from the request `Host` header. A forged one would
    // point the confirmation link at a domain the attacker controls, handing
    // them a token that verifies an address on someone else's account.
    let Some(base_url) = crate::utils::tenant_origin::email_link_base(
        crate::utils::tenant_origin::workspace_origin(&workspace),
    ) else {
        error!(
            user_uuid = %user.uuid,
            "refusing to send address confirmation: no canonical origin and FRONTEND_URL is unset"
        );
        return;
    };

    let email_service = match crate::utils::email::EmailService::from_env() {
        Ok(s) => s,
        Err(e) => {
            error!(error = ?e, "email verification: mail is not configured");
            return;
        }
    };

    let recipient = email.email.clone();
    let user_name = user.name.clone();
    let user_uuid = user.uuid;

    // Branding and the outbound queue are workspace-isolated. Pinned as the
    // RLS-enforced runtime role rather than the bypass variant, so the branding
    // read (which carries no explicit workspace filter) returns THIS
    // workspace's settings instead of an arbitrary tenant's.
    //
    // `with_actor_context` on the connection already in hand, rather than
    // `run_in_workspace`, which is the same call preceded by `pool.get()`. The
    // caller is holding a connection for the whole request, so going through
    // the pool here would hold two at once, and a burst near the pool size is
    // how that turns into a deadlock.
    let actor = crate::sync::actor::ActorContext::system("background:email_verification")
        .with_workspace(workspace_id);
    let enqueued = crate::sync::session::with_actor_context::<_, diesel::result::Error>(
        conn,
        &actor,
        move |conn| {
            let branding = crate::utils::email_branding::get_email_branding(
                conn,
                &base_url,
                crate::utils::email_branding::SentFrom::Workspace,
            );
            let locale = crate::repository::user_locale::resolve_effective_locale(conn, user_uuid);
            crate::services::transactional_email::enqueue_email_verification(
                conn,
                &email_service,
                &branding,
                &recipient,
                &user_name,
                &token,
                &locale,
            )
        },
    );

    match enqueued {
        Ok(row) => info!(
            queue_id = row.id,
            user_uuid = %user.uuid,
            "Address confirmation email enqueued"
        ),
        Err(e) => error!(user_uuid = %user.uuid, error = ?e, "email verification: enqueue failed"),
    }
}
