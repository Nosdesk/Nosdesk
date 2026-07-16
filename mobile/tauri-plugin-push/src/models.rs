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
