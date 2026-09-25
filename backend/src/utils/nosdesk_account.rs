//! Links into a person's Nosdesk account (the control-plane dashboard), where
//! hosted staff identities and seats are managed. `None` when no dashboard URL
//! is configured (self-hosted), so callers simply omit the link.

fn base() -> Option<String> {
    std::env::var("CONTROL_PLANE_URL")
        .ok()
        .map(|u| u.trim().trim_end_matches('/').to_string())
        .filter(|u| !u.is_empty())
}

/// The signed-in person's own account settings (name, avatar, email addresses,
/// sign-in).
pub fn account_settings_url() -> Option<String> {
    base().map(|b| format!("{b}/settings"))
}

/// A workspace's staff seats, opened on one person when their email is known.
pub fn seat_url(workspace_slug: Option<&str>, email: Option<&str>) -> Option<String> {
    let base = base()?;
    let mut url = format!("{base}/workspaces");
    if let Some(slug) = workspace_slug {
        url.push_str(&format!("?workspace={}", urlencoding::encode(slug)));
        if let Some(email) = email {
            url.push_str(&format!("&person={}", urlencoding::encode(email)));
        }
    }
    Some(url)
}

/// Where to change something the Nosdesk account owns: your own settings, or
/// the other person's seat.
pub fn manage_url(
    is_self: bool,
    workspace_slug: Option<&str>,
    email: Option<&str>,
) -> Option<String> {
    if is_self {
        account_settings_url()
    } else {
        seat_url(workspace_slug, email)
    }
}
