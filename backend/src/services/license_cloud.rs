//! Connecting this instance to Nosdesk Cloud, and renewing its licence
//! (docs/plans/self-hosted-license-activation.md, step 3).
//!
//! Connecting is RFC 8628's device flow run from this side: ask the control
//! plane for a code, show it to the admin, poll until they approve it on the
//! dashboard, install the licence that comes back. The instance always calls
//! out; the cloud never calls in, so a server on a private network works.
//!
//! The poll runs here, not in the browser, so the device code (a bearer
//! secret for the licence until it is collected) never reaches the page, and
//! connecting finishes even if the admin closes the tab. The active link is
//! held in this process: the licence it installs goes to the database and
//! reaches other replicas through the licence reload, and a link lost to a
//! restart is started again in one click.
//!
//! Renewal presents the current licence and installs a newer one if the
//! account has it. A failed renewal is recorded, never acted on: the licence
//! the instance holds keeps working until its own expiry.
//!
//! Nothing here logs a device code or a licence.

use std::sync::Mutex;
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::db::Pool;
use crate::license::{self, InstallError, LicenseSource};
use crate::services::notifications::channels::push::PushSender;
use crate::services::notifications::channels::push_mode::SwitchablePushSender;
use crate::services::notifications::channels::relay_client::{cloud_base_url, cloud_http};

/// RFC 8628 §3.5: a `slow_down` adds five seconds to the interval.
const SLOW_DOWN_STEP_SECS: u64 = 5;
/// Env switch for the daily renewal. On unless set to `false`.
pub const AUTO_REFRESH_ENV: &str = "NOSDESK_LICENSE_AUTO_REFRESH";

/// Why a call to the cloud did not produce an answer. Static kinds, safe to
/// log and to show an operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudError {
    /// No answer: DNS, TLS, a firewall, or a timeout.
    Unreachable,
    /// The cloud answered but is not able to serve this (misconfigured,
    /// rate limited).
    Unavailable,
    /// The cloud answered with something this build does not understand.
    Unexpected,
}

impl CloudError {
    pub fn kind(self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::Unavailable => "unavailable",
            Self::Unexpected => "unexpected",
        }
    }
}

/// The process's shared cloud client (see `relay_client::cloud_http`).
fn client() -> Result<reqwest::Client, CloudError> {
    cloud_http().ok_or(CloudError::Unexpected)
}

async fn post(path: &str, body: &serde_json::Value) -> Result<reqwest::Response, CloudError> {
    client()?
        .post(format!("{}{path}", cloud_base_url()))
        .json(body)
        .send()
        .await
        .map_err(|e| {
            // Without the URL: it is not secret, but keep the habit the relay
            // client has so nothing request-derived rides a rendered error.
            tracing::warn!(error = %e.without_url(), "license cloud request failed");
            CloudError::Unreachable
        })
}

// ---------------------------------------------------------------------------
// Connecting
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct StartResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: String,
    expires_in: i64,
    interval: u64,
}

/// What the admin page shows about a connection in progress.
#[derive(Debug, Clone, Serialize)]
pub struct LinkView {
    pub id: Uuid,
    /// `XXXX-XXXX`, as the cloud formats it.
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    /// Unix seconds.
    pub expires_at: i64,
    /// `pending`, `connected`, `denied`, `expired` or `failed`.
    pub status: &'static str,
    /// Why it failed, when `status` is `failed`: a static kind.
    pub error: Option<&'static str>,
}

struct ActiveLink {
    view: LinkView,
    cancel: CancellationToken,
}

static ACTIVE: Mutex<Option<ActiveLink>> = Mutex::new(None);

fn active() -> std::sync::MutexGuard<'static, Option<ActiveLink>> {
    ACTIVE.lock().unwrap_or_else(|p| p.into_inner())
}

/// The connection this process is running or last ran, if any.
pub fn current_link() -> Option<LinkView> {
    active().as_ref().map(|a| a.view.clone())
}

/// Stop polling and forget the connection.
pub fn cancel_link() {
    if let Some(a) = active().take() {
        a.cancel.cancel();
    }
}

fn set_status(id: Uuid, status: &'static str, error: Option<&'static str>) {
    if let Some(a) = active().as_mut().filter(|a| a.view.id == id) {
        a.view.status = status;
        a.view.error = error;
    }
}

/// Open a connection and start polling for its approval. Replaces any
/// connection already running.
pub async fn start_link(
    pool: Pool,
    push: Option<std::sync::Arc<SwitchablePushSender>>,
    instance_id: String,
    display_host: Option<String>,
    started_by: Option<Uuid>,
) -> Result<LinkView, CloudError> {
    let res = post(
        "/api/relay/v1/link/start",
        &serde_json::json!({
            "instance_id": instance_id,
            "display_host": display_host,
            "version": crate::handlers::system::get_current_version(),
        }),
    )
    .await?;
    match res.status().as_u16() {
        200 => {}
        429 | 503 => return Err(CloudError::Unavailable),
        _ => return Err(CloudError::Unexpected),
    }
    let started: StartResponse = res.json().await.map_err(|_| CloudError::Unexpected)?;

    let view = LinkView {
        id: Uuid::new_v4(),
        user_code: started.user_code,
        verification_uri: started.verification_uri,
        verification_uri_complete: started.verification_uri_complete,
        expires_at: Utc::now().timestamp() + started.expires_in,
        status: "pending",
        error: None,
    };
    let cancel = CancellationToken::new();
    if let Some(previous) = active().replace(ActiveLink {
        view: view.clone(),
        cancel: cancel.clone(),
    }) {
        previous.cancel.cancel();
    }
    tokio::spawn(poll_until_decided(
        pool,
        push,
        view.id,
        started.device_code,
        view.expires_at,
        started.interval.max(1),
        cancel,
        started_by,
    ));
    Ok(view)
}

#[allow(clippy::too_many_arguments)]
async fn poll_until_decided(
    pool: Pool,
    push: Option<std::sync::Arc<SwitchablePushSender>>,
    id: Uuid,
    device_code: String,
    expires_at: i64,
    mut interval: u64,
    cancel: CancellationToken,
    started_by: Option<Uuid>,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(Duration::from_secs(interval)) => {}
        }
        if Utc::now().timestamp() >= expires_at {
            set_status(id, "expired", None);
            return;
        }
        let res = match post(
            "/api/relay/v1/link/poll",
            &serde_json::json!({ "device_code": device_code }),
        )
        .await
        {
            Ok(r) => r,
            // Transient: keep trying until the code expires.
            Err(_) => continue,
        };
        if res.status().is_success() {
            #[derive(Deserialize)]
            struct Delivered {
                license: String,
            }
            let Ok(Delivered { license: key }) = res.json().await else {
                set_status(id, "failed", Some(CloudError::Unexpected.kind()));
                return;
            };
            let pool = pool.clone();
            let installed = tokio::task::spawn_blocking(move || {
                let mut conn = pool.get().map_err(|_| None)?;
                let info = license::install(&mut conn, &key, LicenseSource::Linked, started_by)
                    .map_err(|e| match e {
                        InstallError::Invalid(err) => Some(err.kind()),
                        InstallError::EnvManaged => Some("env_managed"),
                        InstallError::Storage(_) => None,
                    })?;
                let _ = crate::utils::security_events::record_security_event(
                    &mut conn,
                    crate::utils::security_events::SecurityEventInput {
                        user_uuid: started_by,
                        event_type: "license_installed",
                        severity: "info",
                        details: Some(serde_json::json!({
                            "source": "linked",
                            "license_id": info.license_id,
                            "licensee": info.licensee,
                        })),
                        request: None,
                    },
                );
                Ok::<_, Option<&'static str>>(())
            })
            .await;
            match installed {
                Ok(Ok(())) => {
                    tracing::info!("license connected to Nosdesk Cloud and installed");
                    set_status(id, "connected", None);
                    if let Some(push) = push {
                        push.reset_relay().await;
                    }
                }
                Ok(Err(kind)) => {
                    tracing::warn!(
                        error_kind = kind.unwrap_or("storage"),
                        "connected license was not installed"
                    );
                    set_status(id, "failed", Some(kind.unwrap_or("storage")));
                }
                Err(_) => set_status(id, "failed", Some("storage")),
            }
            return;
        }
        #[derive(Deserialize)]
        struct PollError {
            error: String,
        }
        let code = res
            .json::<PollError>()
            .await
            .map(|e| e.error)
            .unwrap_or_default();
        match code.as_str() {
            "authorization_pending" => {}
            "slow_down" => interval += SLOW_DOWN_STEP_SECS,
            "access_denied" => {
                set_status(id, "denied", None);
                return;
            }
            "expired_token" => {
                set_status(id, "expired", None);
                return;
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Renewal
// ---------------------------------------------------------------------------

/// Outcome of a renewal check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// A newer licence was installed.
    Updated,
    /// The licence held is the current one.
    Unchanged,
    /// Nothing to renew: no stored licence, or the environment manages it.
    NotApplicable,
    /// The cloud refused this licence (revoked, replaced long ago, erased).
    Rejected,
    /// The cloud could not be asked.
    Failed(CloudError),
}

impl RefreshOutcome {
    /// Stored as `license_last_refresh_error`; `None` for a success.
    fn error_kind(self) -> Option<&'static str> {
        match self {
            Self::Updated | Self::Unchanged | Self::NotApplicable => None,
            Self::Rejected => Some("rejected"),
            Self::Failed(e) => Some(e.kind()),
        }
    }
}

/// Ask the cloud for a newer licence and install it. Never removes or
/// degrades the licence held: a rejection or an outage is recorded and the
/// current licence runs to its own expiry.
pub async fn refresh(
    pool: Pool,
    push: Option<std::sync::Arc<SwitchablePushSender>>,
) -> RefreshOutcome {
    let state = license::state();
    let source = state.source;
    if !matches!(source, LicenseSource::Pasted | LicenseSource::Linked) {
        return RefreshOutcome::NotApplicable;
    }
    let Some(current) = state.relay_credential() else {
        return RefreshOutcome::NotApplicable;
    };

    let outcome = match post(
        "/api/relay/v1/license/refresh",
        &serde_json::json!({ "license": &*current }),
    )
    .await
    {
        Err(e) => RefreshOutcome::Failed(e),
        Ok(res) => match res.status().as_u16() {
            204 => RefreshOutcome::Unchanged,
            401 => RefreshOutcome::Rejected,
            200 => {
                #[derive(Deserialize)]
                struct Renewed {
                    license: String,
                }
                match res.json::<Renewed>().await {
                    Ok(Renewed { license: key }) => {
                        let pool = pool.clone();
                        let installed = tokio::task::spawn_blocking(move || {
                            let mut conn = pool.get().ok()?;
                            license::install(&mut conn, &key, source, None).ok()
                        })
                        .await
                        .ok()
                        .flatten();
                        match installed {
                            Some(info) => {
                                tracing::info!(license_id = %info.license_id, "license renewed from Nosdesk Cloud");
                                if let Some(push) = push {
                                    push.reset_relay().await;
                                }
                                RefreshOutcome::Updated
                            }
                            None => RefreshOutcome::Failed(CloudError::Unexpected),
                        }
                    }
                    Err(_) => RefreshOutcome::Failed(CloudError::Unexpected),
                }
            }
            _ => RefreshOutcome::Failed(CloudError::Unavailable),
        },
    };

    if outcome != RefreshOutcome::NotApplicable {
        let error = outcome.error_kind();
        let _ = tokio::task::spawn_blocking(move || {
            let mut conn = pool.get().ok()?;
            crate::repository::instance_settings::record_refresh(&mut conn, error).ok()
        })
        .await;
        if let Some(kind) = error {
            tracing::warn!(error_kind = kind, "license renewal did not complete");
        }
    }
    outcome
}

/// Whether the daily renewal runs. On unless the env says `false`.
pub fn auto_refresh_enabled() -> bool {
    !std::env::var(AUTO_REFRESH_ENV).is_ok_and(|v| v.trim().eq_ignore_ascii_case("false"))
}

/// Renew once a day. The first check waits a few minutes so a boot storm of
/// replicas does not all ask at once.
pub fn spawn_daily_refresh(
    pool: Pool,
    push: std::sync::Arc<SwitchablePushSender>,
    shutdown: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let first = Duration::from_secs(300 + u64::from(rand::random::<u16>() % 300));
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = tokio::time::sleep(first) => {}
        }
        let mut tick = tokio::time::interval(Duration::from_secs(86_400));
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = tick.tick() => {}
            }
            if auto_refresh_enabled() {
                refresh(pool.clone(), Some(push.clone())).await;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_renewal_is_recorded_and_a_success_clears_it() {
        assert_eq!(RefreshOutcome::Updated.error_kind(), None);
        assert_eq!(RefreshOutcome::Unchanged.error_kind(), None);
        assert_eq!(RefreshOutcome::Rejected.error_kind(), Some("rejected"));
        assert_eq!(
            RefreshOutcome::Failed(CloudError::Unreachable).error_kind(),
            Some("unreachable")
        );
    }

    #[test]
    fn link_status_changes_only_touch_the_current_link() {
        cancel_link();
        let id = Uuid::new_v4();
        *active() = Some(ActiveLink {
            view: LinkView {
                id,
                user_code: "BCDF-GHJK".into(),
                verification_uri: String::new(),
                verification_uri_complete: String::new(),
                expires_at: 0,
                status: "pending",
                error: None,
            },
            cancel: CancellationToken::new(),
        });
        set_status(Uuid::new_v4(), "denied", None);
        assert_eq!(
            current_link().map(|v| v.status),
            Some("pending"),
            "a stale poller cannot overwrite"
        );
        set_status(id, "connected", None);
        assert_eq!(current_link().map(|v| v.status), Some("connected"));
        cancel_link();
        assert!(current_link().is_none());
    }
}
