use serde::{Deserialize, Serialize};

/// Arguments for `save`: the refresh token to persist.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveRequest {
  pub token: String,
}

/// Result of `load`: the stored token, or `None` if nothing is stored.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadResponse {
  pub value: Option<String>,
}
