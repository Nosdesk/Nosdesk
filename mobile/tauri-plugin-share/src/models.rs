use serde::{Deserialize, Serialize};

/// Arguments for `share_file`.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareFileRequest {
    /// Absolute path of the file to share. On Android it must sit under the
    /// cache `share/` folder, the only one the plugin's `FileProvider` exposes.
    pub path: String,
    /// MIME type of the file, e.g. `application/pdf`.
    pub mime_type: String,
    /// Chooser title. Android only: iOS titles the sheet from the file itself.
    pub title: Option<String>,
}

/// Result of `share_file`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareFileResponse {
    /// True when the user picked an action, false when they dismissed the
    /// sheet. Android always reports true once the chooser opens, since it
    /// does not report the outcome reliably.
    pub completed: bool,
}
