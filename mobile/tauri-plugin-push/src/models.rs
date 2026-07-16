use serde::{Deserialize, Serialize};

/// Result of `request_permission`: whether the user granted notification
/// permission. On iOS this also kicks off APNs registration; on Android 13+
/// it reflects the `POST_NOTIFICATIONS` runtime grant.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResponse {
  pub granted: bool,
}

/// Result of `get_token`: the platform push token (APNs hex token on iOS, FCM
/// registration token on Android), or `None` if unavailable (permission
/// denied, registration still pending, or the provider isn't configured yet).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenResponse {
  pub token: Option<String>,
}

/// A notification the user TAPPED, surfaced so the JS layer can deep-link. On
/// cold start (app launched from a tap) `get_pending_notification` returns the
/// buffered tap once; for taps while running, the plugin emits a
/// `notificationOpened` event with the same shape. All fields `None` = nothing
/// pending. Mirrors the PII-free push payload (`nd_type` + entity refs).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingNotification {
  pub nd_type: Option<String>,
  pub entity_type: Option<String>,
  pub entity_id: Option<i32>,
  pub ticket_id: Option<i32>,
}
