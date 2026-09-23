//! Which push sender is live, and switching it at runtime.
//!
//! The mode comes from `NOSDESK_PUSH_MODE` when set, else the value an admin
//! stored in `instance_settings`, else the historical default:
//!
//!   unset   native when NOSDESK_APNS_* / NOSDESK_FCM_* are set, else inert.
//!           Hosted therefore needs no new variable.
//!   native  same, stated explicitly.
//!   relay   forward through the cloud relay, which holds the com.nosdesk.app
//!           credentials. Native creds are IGNORED in this mode, so a
//!           self-hoster cannot accidentally send official-app device tokens
//!           with their own key.
//!   off     inert even when credentials are present.
//!
//! Inert means is_available=false: push preferences still exist and device
//! registration still works, nothing is delivered.
//!
//! [`SwitchablePushSender`] is the one sender the push channel holds; it
//! delegates to the live inner sender and rebuilds it when the mode changes,
//! so an admin choosing relay mode (or a licence arriving) needs no restart.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;

use super::push::{NoopPushSender, PushOutcome, PushPayload, PushSender, PushTarget};
use super::relay_client::{CloudRelayPushSender, RelayStatus};

pub const PUSH_MODE_ENV: &str = "NOSDESK_PUSH_MODE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushMode {
    /// Nothing chosen: native when credentials exist, else inert.
    Default,
    Native,
    Relay,
    Off,
}

impl PushMode {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "default" => Some(Self::Default),
            "native" => Some(Self::Native),
            "relay" => Some(Self::Relay),
            "off" => Some(Self::Off),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Native => "native",
            Self::Relay => "relay",
            Self::Off => "off",
        }
    }
}

/// Where the live mode came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushModeSource {
    Env,
    Stored,
    Default,
}

impl PushModeSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::Stored => "stored",
            Self::Default => "default",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedPushMode {
    pub mode: PushMode,
    pub source: PushModeSource,
}

/// Apply the precedence. An unrecognised env value is an error: at boot it is
/// fatal, as it always was.
pub fn resolve(env: Option<&str>, stored: Option<&str>) -> Result<ResolvedPushMode, String> {
    if let Some(raw) = env.filter(|s| !s.trim().is_empty()) {
        return PushMode::parse(raw)
            .map(|mode| ResolvedPushMode {
                mode,
                source: PushModeSource::Env,
            })
            .ok_or_else(|| {
                format!(
                    "{PUSH_MODE_ENV}={raw:?} is not recognised (expected relay, native, or off)"
                )
            });
    }
    Ok(match stored.and_then(PushMode::parse) {
        Some(mode) if mode != PushMode::Default => ResolvedPushMode {
            mode,
            source: PushModeSource::Stored,
        },
        _ => ResolvedPushMode {
            mode: PushMode::Default,
            source: PushModeSource::Default,
        },
    })
}

/// Whether this instance carries its own APNs or FCM credentials, which is
/// what native mode sends with. Presence only; validity is checked on build.
pub fn native_credentials_present() -> bool {
    [
        "NOSDESK_APNS_KEY_ID",
        "NOSDESK_FCM_SERVICE_ACCOUNT",
        "NOSDESK_FCM_SERVICE_ACCOUNT_PATH",
    ]
    .iter()
    .any(|k| std::env::var(k).is_ok_and(|v| !v.trim().is_empty()))
}

pub fn env_value() -> Option<String> {
    std::env::var(PUSH_MODE_ENV)
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Build the sender for `mode`. A configured-but-malformed native provider is
/// an error, so a half-provisioned deploy fails loudly rather than going quiet.
pub fn build(mode: PushMode, instance_id: &str) -> Result<Arc<dyn PushSender>, String> {
    match mode {
        PushMode::Relay => CloudRelayPushSender::new(instance_id.to_string())
            .map(|s| Arc::new(s) as Arc<dyn PushSender>)
            .map_err(|e| format!("relay push sender is configured but invalid: {e}")),
        PushMode::Off => Ok(Arc::new(NoopPushSender)),
        PushMode::Default | PushMode::Native => {
            match super::push_sender::NativePushSender::from_env() {
                Ok(Some(sender)) => Ok(sender),
                Ok(None) => Ok(Arc::new(NoopPushSender)),
                Err(e) => Err(format!("push sender is configured but invalid: {e:#}")),
            }
        }
    }
}

struct Live {
    resolved: ResolvedPushMode,
    sender: Arc<dyn PushSender>,
}

/// The push channel's sender. Delegates to the sender for the current mode.
pub struct SwitchablePushSender {
    live: RwLock<Live>,
    instance_id: String,
}

impl SwitchablePushSender {
    pub fn new(resolved: ResolvedPushMode, instance_id: String) -> Result<Self, String> {
        let sender = build(resolved.mode, &instance_id)?;
        Ok(Self {
            live: RwLock::new(Live { resolved, sender }),
            instance_id,
        })
    }

    fn inner(&self) -> Arc<dyn PushSender> {
        self.live
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .sender
            .clone()
    }

    pub fn resolved(&self) -> ResolvedPushMode {
        self.live.read().unwrap_or_else(|p| p.into_inner()).resolved
    }

    /// Switch to `resolved`. Rebuilds only when the mode itself changes, so a
    /// relay sender keeps its status and token across a no-op re-apply.
    /// Returns whether the sender was rebuilt.
    pub fn apply(&self, resolved: ResolvedPushMode) -> Result<bool, String> {
        if self.resolved().mode == resolved.mode {
            self.live
                .write()
                .unwrap_or_else(|p| p.into_inner())
                .resolved = resolved;
            return Ok(false);
        }
        let sender = build(resolved.mode, &self.instance_id)?;
        tracing::info!(
            mode = resolved.mode.as_str(),
            source = resolved.source.as_str(),
            sender = sender.name(),
            configured = sender.is_configured(),
            "Push sender switched"
        );
        *self.live.write().unwrap_or_else(|p| p.into_inner()) = Live { resolved, sender };
        Ok(true)
    }
}

#[async_trait]
impl PushSender for SwitchablePushSender {
    fn is_configured(&self) -> bool {
        self.inner().is_configured()
    }

    async fn send(&self, targets: &[PushTarget], payload: &PushPayload) -> PushOutcome {
        self.inner().send(targets, payload).await
    }

    fn relay_status(&self) -> Option<RelayStatus> {
        self.inner().relay_status()
    }

    async fn reset_relay(&self) {
        self.inner().reset_relay().await;
    }

    fn name(&self) -> &'static str {
        self.inner().name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_wins_over_stored() {
        let r = resolve(Some("off"), Some("relay")).expect("resolve");
        assert_eq!(r.mode, PushMode::Off);
        assert_eq!(r.source, PushModeSource::Env);
    }

    #[test]
    fn stored_applies_when_env_is_unset_or_blank() {
        for env in [None, Some(""), Some("  ")] {
            let r = resolve(env, Some("relay")).expect("resolve");
            assert_eq!(r.mode, PushMode::Relay);
            assert_eq!(r.source, PushModeSource::Stored);
        }
    }

    #[test]
    fn nothing_set_is_the_historical_default() {
        let r = resolve(None, None).expect("resolve");
        assert_eq!(r.mode, PushMode::Default);
        assert_eq!(r.source, PushModeSource::Default);
    }

    #[test]
    fn an_unrecognised_env_value_is_an_error() {
        assert!(resolve(Some("carrier-pigeon"), None).is_err());
    }

    #[test]
    fn a_corrupt_stored_value_falls_back_to_default() {
        let r = resolve(None, Some("carrier-pigeon")).expect("resolve");
        assert_eq!(r.mode, PushMode::Default);
    }
}
