//! OS keychain storage for the auth refresh token.
//!
//! The genuine iOS Keychain (and the macOS keychain for `tauri dev`) via
//! SecItem generic passwords, exposed as Tauri commands the JS `SecureStore`
//! calls. The token is stored device-only (never iCloud-synced) with the
//! default `WhenUnlocked` accessibility: readable while the device is unlocked,
//! which it always is when the app is foreground doing a silent token refresh,
//! and inaccessible while locked. Reads are non-interactive (no biometric
//! prompt), so refresh stays silent. `WhenUnlocked` is deliberately more
//! restrictive than `AfterFirstUnlock`: this is a foreground app, so the token
//! is never read while the device is locked.

const SERVICE: &str = "com.nosdesk.app";
const ACCOUNT: &str = "refresh_token";

#[cfg(any(target_os = "ios", target_os = "macos"))]
mod imp {
    use super::{ACCOUNT, SERVICE};
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password_options,
        PasswordOptions,
    };
    use security_framework_sys::base::errSecItemNotFound;

    pub fn save(token: &str) -> Result<(), String> {
        let mut options = PasswordOptions::new_generic_password(SERVICE, ACCOUNT);
        // Device-only: never replicated to iCloud or other devices.
        options.set_access_synchronized(Some(false));
        // Creates or updates (handles the token rotated on every refresh).
        set_generic_password_options(token.as_bytes(), options).map_err(|e| e.to_string())
    }

    pub fn load() -> Result<Option<String>, String> {
        match get_generic_password(SERVICE, ACCOUNT) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|e| format!("keychain value not valid UTF-8: {e}")),
            // No stored token -> a fresh / signed-out session, not an error.
            Err(e) if e.code() == errSecItemNotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn clear() -> Result<(), String> {
        match delete_generic_password(SERVICE, ACCOUNT) {
            Ok(()) => Ok(()),
            // Already absent is success.
            Err(e) if e.code() == errSecItemNotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

// Non-Apple targets (Linux/Windows desktop, Android) aren't built yet; the
// Android Keystore / Secret Service paths would go here. Stub so the crate
// still compiles everywhere.
#[cfg(not(any(target_os = "ios", target_os = "macos")))]
mod imp {
    const UNSUPPORTED: &str = "secure store is not implemented on this platform";
    pub fn save(_token: &str) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    pub fn load() -> Result<Option<String>, String> {
        Err(UNSUPPORTED.into())
    }
    pub fn clear() -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
}

#[tauri::command]
pub fn secure_store_save(token: String) -> Result<(), String> {
    imp::save(&token)
}

#[tauri::command]
pub fn secure_store_load() -> Result<Option<String>, String> {
    imp::load()
}

#[tauri::command]
pub fn secure_store_clear() -> Result<(), String> {
    imp::clear()
}
