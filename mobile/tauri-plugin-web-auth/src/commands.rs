use tauri::{command, AppHandle};

use crate::models::*;
use crate::WebAuthExt;

#[command]
pub async fn authenticate<R: tauri::Runtime>(
    app: AppHandle<R>,
    payload: AuthenticateRequest,
) -> crate::Result<AuthenticateResponse> {
    app.web_auth().authenticate(payload)
}
