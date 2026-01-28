use actix_web::{web, HttpResponse, Responder, HttpMessage};
use serde_json::json;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;
use reqwest;
use urlencoding;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::Duration;
// Removed unused imports: std::path::Path, tokio::fs, tokio::io::AsyncWriteExt, image::{ImageFormat, DynamicImage}, tracing::{span, Level}
use tracing::{info, warn, error, debug, trace, instrument};

use crate::db::{Pool, DbConnection};
// Auth providers are now configured via environment variables
use crate::repository::users as user_repo;
use crate::repository::devices as device_repo;
use crate::repository::user_auth_identities as identity_repo;
use crate::repository::user_emails as user_emails_repo;
use crate::repository::sync_history as sync_history_repo;
use crate::repository::groups as groups_repo;
use crate::config_utils;
use crate::models::{NewUserAuthIdentity, User, UserAuthIdentity, NewSyncHistory, SyncHistoryUpdate, AuthProvider};
use crate::utils;

// Helper function for environment-based auth providers
fn get_default_microsoft_provider() -> Result<AuthProvider, diesel::result::Error> {
    // Using environment variables, return a fixed provider for Microsoft
    if std::env::var("MICROSOFT_CLIENT_ID").is_ok() 
        && std::env::var("MICROSOFT_CLIENT_SECRET").is_ok() 
        && std::env::var("MICROSOFT_TENANT_ID").is_ok() {
        Ok(AuthProvider::new(2, "Microsoft".to_string(), "microsoft".to_string(), true, false))
    } else {
        Err(diesel::result::Error::NotFound)
    }
}

// Global progress tracker with cancellation support
lazy_static::lazy_static! {
    static ref SYNC_PROGRESS: Arc<Mutex<HashMap<String, SyncProgressState>>> = Arc::new(Mutex::new(HashMap::new()));
    static ref SYNC_CANCELLATION: Arc<Mutex<HashMap<String, bool>>> = Arc::new(Mutex::new(HashMap::new()));
}

// Configuration constants for optimization
const CONCURRENT_USER_PROCESSING: usize = 8; // Number of concurrent user processing tasks
const USER_BATCH_SIZE: usize = 25; // Number of users to process in each user sync batch
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30); // HTTP request timeout

// Helper function to get user sync concurrency settings
fn get_user_sync_config() -> (usize, usize) {
    let concurrent_processing = std::env::var("MSGRAPH_CONCURRENT_USER_PROCESSING")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(CONCURRENT_USER_PROCESSING);
    
    let user_batch_size = std::env::var("MSGRAPH_USER_BATCH_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(USER_BATCH_SIZE);
    
    (concurrent_processing, user_batch_size)
}

/// Creates a Microsoft Graph HTTP client with access token using client credentials flow.
/// This is the shared helper to eliminate token acquisition duplication across sync functions.
async fn get_msgraph_client_and_token() -> Result<(reqwest::Client, String), String> {
    let client_id = std::env::var("MICROSOFT_CLIENT_ID")
        .map_err(|_| "MICROSOFT_CLIENT_ID not configured")?;
    let client_secret = std::env::var("MICROSOFT_CLIENT_SECRET")
        .map_err(|_| "MICROSOFT_CLIENT_SECRET not configured")?;
    let tenant_id = std::env::var("MICROSOFT_TENANT_ID")
        .map_err(|_| "MICROSOFT_TENANT_ID not configured")?;

    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .pool_max_idle_per_host(10)
        .pool_idle_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let params = [
        ("client_id", client_id.as_str()),
        ("client_secret", client_secret.as_str()),
        ("grant_type", "client_credentials"),
        ("scope", "https://graph.microsoft.com/.default"),
    ];

    let token_response = client
        .post(format!("https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/token"))
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Failed to request access token: {e}"))?;

    let token_data: serde_json::Value = token_response
        .json()
        .await
        .map_err(|e| format!("Failed to parse token response: {e}"))?;

    let access_token = token_data["access_token"]
        .as_str()
        .ok_or_else(|| {
            let error_desc = token_data.get("error_description")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            format!("Failed to obtain access token: {error_desc}")
        })?
        .to_string();

    Ok((client, access_token))
}

#[derive(Serialize, Debug, Clone)]
pub struct SyncProgressState {
    pub session_id: String,
    pub entity: String,
    pub current: usize,
    pub total: usize,
    pub status: String,
    pub message: String,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub sync_type: String, // "users", "profile_photos", "devices", "groups", or "multiple"
    pub is_delta: bool,
}

#[derive(Deserialize, Debug)]
pub struct SyncDataRequest {
    pub entities: Vec<String>,
    /// Use delta sync (incremental) instead of full sync
    #[serde(default)]
    pub use_delta: bool,
}

#[derive(Serialize, Debug)]
pub struct ConnectionStatus {
    pub status: String,
    pub message: String,
    pub last_sync: Option<DateTime<Utc>>,
    pub available_entities: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct SyncProgress {
    pub entity: String,
    pub processed: usize,
    pub total: usize,
    pub status: String,
    pub errors: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct SyncResult {
    pub success: bool,
    pub message: String,
    pub results: Vec<SyncProgress>,
    pub total_processed: usize,
    pub total_errors: usize,
}

// Microsoft Graph User structure from API response
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct MicrosoftGraphUser {
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "givenName")]
    pub given_name: Option<String>,
    pub surname: Option<String>,
    pub mail: Option<String>,
    #[serde(rename = "userPrincipalName")]
    pub user_principal_name: String,
    #[serde(rename = "jobTitle")]
    pub job_title: Option<String>,
    pub department: Option<String>,
    #[serde(rename = "officeLocation")]
    pub office_location: Option<String>,
    #[serde(rename = "mobilePhone")]
    pub mobile_phone: Option<String>,
    #[serde(rename = "businessPhones")]
    pub business_phones: Option<Vec<String>>,
    #[serde(rename = "proxyAddresses")]
    pub proxy_addresses: Option<Vec<String>>,
    #[serde(rename = "otherMails")]
    pub other_mails: Option<Vec<String>>,
    #[serde(rename = "accountEnabled")]
    pub account_enabled: Option<bool>,
}

// Entra ID Device structure from API response (/devices endpoint)
// This is for device identity from Microsoft Entra ID, supports delta sync
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct EntraDevice {
    /// Entra ID object ID (used for group membership)
    pub id: String,
    /// The actual device identifier
    #[serde(rename = "deviceId")]
    pub device_id: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "operatingSystem")]
    pub operating_system: Option<String>,
    #[serde(rename = "operatingSystemVersion")]
    pub operating_system_version: Option<String>,
    /// Trust type: AzureAd, ServerAd, Workplace
    #[serde(rename = "trustType")]
    pub trust_type: Option<String>,
    #[serde(rename = "isManaged")]
    pub is_managed: Option<bool>,
    #[serde(rename = "isCompliant")]
    pub is_compliant: Option<bool>,
    #[serde(rename = "accountEnabled")]
    pub account_enabled: Option<bool>,
    #[serde(rename = "approximateLastSignInDateTime")]
    pub approximate_last_sign_in_date_time: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    #[serde(rename = "profileType")]
    pub profile_type: Option<String>,
    #[serde(rename = "registrationDateTime")]
    pub registration_date_time: Option<String>,
}

// Microsoft Graph Device structure from API response (Intune managedDevice)
// This is for device management/compliance from Intune, does NOT support delta sync
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct MicrosoftGraphDevice {
    pub id: String,
    #[serde(rename = "deviceName")]
    pub device_name: Option<String>,
    #[serde(rename = "operatingSystem")]
    pub operating_system: Option<String>,
    #[serde(rename = "osVersion")]
    pub os_version: Option<String>,
    #[serde(rename = "manufacturer")]
    pub manufacturer: Option<String>,
    #[serde(rename = "model")]
    pub model: Option<String>,
    #[serde(rename = "serialNumber")]
    pub serial_number: Option<String>,
    #[serde(rename = "azureADDeviceId")]
    pub azure_ad_device_id: Option<String>,
    #[serde(rename = "userPrincipalName")]
    pub user_principal_name: Option<String>,
    #[serde(rename = "userId")]
    pub user_id: Option<String>,
    #[serde(rename = "complianceState")]
    pub compliance_state: Option<String>,
    #[serde(rename = "lastSyncDateTime")]
    pub last_sync_date_time: Option<String>,
    #[serde(rename = "enrolledDateTime")]
    pub enrolled_date_time: Option<String>,
    #[serde(rename = "deviceEnrollmentType")]
    pub device_enrollment_type: Option<String>,
    #[serde(rename = "managementAgent")]
    pub management_agent: Option<String>,
    /// Entra Object ID - resolved from azure_ad_device_id during sync.
    /// This is the directory object ID used for group membership matching.
    #[serde(skip)]
    #[allow(dead_code)]
    pub entra_object_id: Option<String>,
}

// Microsoft Graph Group structure from API response
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct MicrosoftGraphGroup {
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "mailEnabled")]
    pub mail_enabled: Option<bool>,
    #[serde(rename = "securityEnabled")]
    pub security_enabled: Option<bool>,
    #[serde(rename = "groupTypes")]
    pub group_types: Option<Vec<String>>,
    pub mail: Option<String>,
}

impl MicrosoftGraphGroup {
    /// Determine the group type based on Microsoft Graph properties
    pub fn get_group_type(&self) -> &'static str {
        let types = self.group_types.as_deref().unwrap_or(&[]);
        let mail_enabled = self.mail_enabled.unwrap_or(false);
        let security_enabled = self.security_enabled.unwrap_or(false);

        if types.iter().any(|t| t == "DynamicMembership") {
            "dynamic"
        } else if types.iter().any(|t| t == "Unified") {
            "m365"
        } else if security_enabled && !mail_enabled {
            "security"
        } else if mail_enabled && !security_enabled {
            "distribution"
        } else {
            "other"
        }
    }

    /// Check if this is a distribution list
    #[allow(dead_code)]
    pub fn is_distribution_list(&self) -> bool {
        self.get_group_type() == "distribution"
    }
}

// Microsoft Graph Group Member structure (for membership sync)
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct MicrosoftGraphGroupMember {
    #[serde(rename = "@odata.type")]
    pub odata_type: Option<String>,
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "userPrincipalName")]
    pub user_principal_name: Option<String>,
}

impl MicrosoftGraphGroupMember {
    /// Check if this member is a user (not a nested group or other type)
    pub fn is_user(&self) -> bool {
        self.odata_type.as_deref() == Some("#microsoft.graph.user")
    }

    /// Check if this member is a device
    pub fn is_device(&self) -> bool {
        self.odata_type.as_deref() == Some("#microsoft.graph.device")
    }
}

// Group sync configuration
#[derive(Debug, Clone)]
pub struct GroupSyncConfig {
    pub sync_security_groups: bool,
    pub sync_m365_groups: bool,
    pub sync_dynamic_groups: bool,
    pub sync_distribution_lists: bool,
}

impl Default for GroupSyncConfig {
    fn default() -> Self {
        Self {
            sync_security_groups: true,
            sync_m365_groups: true,
            sync_dynamic_groups: true,
            sync_distribution_lists: false, // Off by default
        }
    }
}

impl GroupSyncConfig {
    /// Load config from environment variables (can be extended to load from site_settings)
    pub fn from_env() -> Self {
        Self {
            sync_security_groups: std::env::var("MSGRAPH_SYNC_SECURITY_GROUPS")
                .map(|v| v.to_lowercase() != "false")
                .unwrap_or(true),
            sync_m365_groups: std::env::var("MSGRAPH_SYNC_M365_GROUPS")
                .map(|v| v.to_lowercase() != "false")
                .unwrap_or(true),
            sync_dynamic_groups: std::env::var("MSGRAPH_SYNC_DYNAMIC_GROUPS")
                .map(|v| v.to_lowercase() != "false")
                .unwrap_or(true),
            sync_distribution_lists: std::env::var("MSGRAPH_SYNC_DISTRIBUTION_LISTS")
                .map(|v| v.to_lowercase() == "true")
                .unwrap_or(false),
        }
    }

    /// Check if a group should be synced based on its type
    pub fn should_sync_group(&self, group: &MicrosoftGraphGroup) -> bool {
        match group.get_group_type() {
            "security" => self.sync_security_groups,
            "m365" => self.sync_m365_groups,
            "dynamic" => self.sync_dynamic_groups,
            "distribution" => self.sync_distribution_lists,
            _ => true, // Sync "other" types by default
        }
    }
}

// Group sync statistics
#[derive(Serialize, Debug, Default)]
pub struct GroupSyncStats {
    pub groups_created: usize,
    pub groups_updated: usize,
    pub user_membership_changes: usize,
    pub device_membership_changes: usize,
    pub errors: Vec<String>,
}

// User sync statistics
#[derive(Serialize, Debug)]
pub struct UserSyncStats {
    pub new_users_created: usize,
    pub existing_users_updated: usize,
    pub identities_linked: usize,
    pub errors: Vec<String>,
}

// Device sync statistics
#[derive(Serialize, Debug)]
pub struct DeviceSyncStats {
    pub new_devices_created: usize,
    pub existing_devices_updated: usize,
    pub devices_assigned: usize,
    pub errors: Vec<String>,
}

/// Parameters for updating sync progress
struct SyncProgressUpdate<'a> {
    session_id: &'a str,
    entity: &'a str,
    current: usize,
    total: usize,
    status: &'a str,
    message: &'a str,
    sync_type: &'a str,
    is_delta: Option<bool>,
}

impl<'a> SyncProgressUpdate<'a> {
    /// Create a new progress update with the given parameters
    fn new(session_id: &'a str, entity: &'a str, current: usize, total: usize, status: &'a str, message: &'a str) -> Self {
        Self {
            session_id,
            entity,
            current,
            total,
            status,
            message,
            sync_type: entity, // Default sync_type to entity
            is_delta: None,
        }
    }

    /// Set the sync type (defaults to entity if not set)
    fn with_sync_type(mut self, sync_type: &'a str) -> Self {
        self.sync_type = sync_type;
        self
    }

    /// Set the is_delta flag explicitly
    fn with_is_delta(mut self, is_delta: bool) -> Self {
        self.is_delta = Some(is_delta);
        self
    }

    /// Apply the update to the in-memory progress map
    fn apply(self) {
        let now = Utc::now();

        if let Ok(mut progress_map) = SYNC_PROGRESS.lock() {
            // Preserve existing values if not explicitly provided
            let existing = progress_map.get(self.session_id);
            let started_at = existing.map(|p| p.started_at).unwrap_or(now);
            let preserved_is_delta = self.is_delta.unwrap_or_else(|| {
                existing.map(|p| p.is_delta).unwrap_or(false)
            });

            let progress = SyncProgressState {
                session_id: self.session_id.to_string(),
                entity: self.entity.to_string(),
                current: self.current,
                total: self.total,
                status: self.status.to_string(),
                message: self.message.to_string(),
                started_at,
                updated_at: now,
                sync_type: self.sync_type.to_string(),
                is_delta: preserved_is_delta,
            };
            progress_map.insert(self.session_id.to_string(), progress);
        }
    }
}

// Helper functions for progress tracking (convenience wrappers)
fn update_sync_progress(session_id: &str, entity: &str, current: usize, total: usize, status: &str, message: &str) {
    SyncProgressUpdate::new(session_id, entity, current, total, status, message).apply();
}

fn update_sync_progress_with_type(session_id: &str, entity: &str, current: usize, total: usize, status: &str, message: &str, sync_type: &str, is_delta: Option<bool>) {
    let mut update = SyncProgressUpdate::new(session_id, entity, current, total, status, message)
        .with_sync_type(sync_type);
    if let Some(delta) = is_delta {
        update = update.with_is_delta(delta);
    }
    update.apply();
}



fn get_sync_progress(session_id: &str) -> Option<SyncProgressState> {
    if let Ok(progress_map) = SYNC_PROGRESS.lock() {
        progress_map.get(session_id).cloned()
    } else {
        None
    }
}

#[allow(dead_code)]
fn clear_sync_progress(session_id: &str) {
    if let Ok(mut progress_map) = SYNC_PROGRESS.lock() {
        progress_map.remove(session_id);
    }
    if let Ok(mut cancellation_map) = SYNC_CANCELLATION.lock() {
        cancellation_map.remove(session_id);
    }
}

// Cancellation support functions
fn is_sync_cancelled(session_id: &str) -> bool {
    if let Ok(cancellation_map) = SYNC_CANCELLATION.lock() {
        cancellation_map.get(session_id).copied().unwrap_or(false)
    } else {
        false
    }
}

fn cancel_sync(session_id: &str) {
    if let Ok(mut cancellation_map) = SYNC_CANCELLATION.lock() {
        cancellation_map.insert(session_id.to_string(), true);
    }
}

fn initialize_sync_session(session_id: &str) {
    if let Ok(mut cancellation_map) = SYNC_CANCELLATION.lock() {
        cancellation_map.insert(session_id.to_string(), false);
    }
}

/// Get sync progress for a specific session
pub async fn get_sync_progress_endpoint(
    req: actix_web::HttpRequest,
    db_pool: web::Data<Pool>,
    path: web::Path<String>,
) -> impl Responder {
    let _conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Database connection failed"
        })),
    };
    // Extract claims from cookie auth middleware
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    let session_id = path.into_inner();
    
    match get_sync_progress(&session_id) {
        Some(progress) => HttpResponse::Ok().json(progress),
        None => HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "Sync session not found"
        }))
    }
}

/// Get all active sync sessions
pub async fn get_active_syncs(
    req: actix_web::HttpRequest,
    db_pool: web::Data<Pool>,
    ) -> impl Responder {
    let _conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Database connection failed"
        })),
    };
    // Extract claims from cookie auth middleware
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    if let Ok(progress_map) = SYNC_PROGRESS.lock() {
        let active_syncs: Vec<SyncProgressState> = progress_map
            .values()
            .filter(|progress| {
                // Only return syncs that are truly active (running or starting)
                progress.status == "running" || 
                progress.status == "starting" ||
                progress.status == "cancelling"
            })
            .cloned()
            .collect();
        
        HttpResponse::Ok().json(json!({
            "active_syncs": active_syncs,
            "count": active_syncs.len()
        }))
    } else {
        HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Failed to access sync progress"
        }))
    }
}

/// Get the most recent completed sync session
pub async fn get_last_sync(
    req: actix_web::HttpRequest,
    db_pool: web::Data<Pool>,
    ) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Database connection failed"
        })),
    };
    // Extract claims from cookie auth middleware
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Try to get from database first (persistent storage)
    match sync_history_repo::get_last_completed_sync(&mut conn) {
        Ok(sync_history) => {
            // Convert database model to API response format
            let response = SyncProgressState {
                session_id: sync_history.id.to_string(),
                entity: sync_history.sync_type.clone(),
                current: sync_history.records_processed.unwrap_or(0) as usize,
                total: sync_history.records_processed.unwrap_or(0) as usize,
                status: sync_history.status,
                message: sync_history.error_message.unwrap_or_else(|| "Sync completed".to_string()),
                started_at: DateTime::from_naive_utc_and_offset(sync_history.started_at, Utc),
                updated_at: DateTime::from_naive_utc_and_offset(sync_history.completed_at.unwrap_or(sync_history.started_at), Utc),
                sync_type: sync_history.sync_type,
                is_delta: sync_history.is_delta,
            };
            HttpResponse::Ok().json(response)
        },
        Err(_) => {
            // Fallback to in-memory storage if database query fails
            if let Ok(progress_map) = SYNC_PROGRESS.lock() {
                let last_sync = progress_map
                    .values()
                    .filter(|progress| {
                        progress.status == "completed" || 
                        progress.status == "error" || 
                        progress.status == "cancelled"
                    })
                    .max_by_key(|progress| progress.updated_at);
                
                match last_sync {
                    Some(sync) => HttpResponse::Ok().json(sync),
                    None => HttpResponse::NotFound().json(json!({
                        "status": "error",
                        "message": "No completed synchronizations found"
                    }))
                }
            } else {
                HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Failed to access sync progress"
                }))
            }
        }
    }
}

/// Cancel a sync session
pub async fn cancel_sync_session(
    req: actix_web::HttpRequest,
    db_pool: web::Data<Pool>,
    path: web::Path<String>,
) -> impl Responder {
    let _conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Database connection failed"
        })),
    };
    // Extract claims from cookie auth middleware
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    let session_id = path.into_inner();
    
    // Check if the session exists and is cancellable
    if let Some(progress) = get_sync_progress(&session_id) {
        if progress.status == "running" || progress.status == "starting" {
            cancel_sync(&session_id);
            update_sync_progress_with_type(&session_id, &progress.entity, progress.current, progress.total, "cancelling", "Cancellation requested", &progress.sync_type, None);
            
            HttpResponse::Ok().json(json!({
                "success": true,
                "message": "Sync cancellation requested"
            }))
        } else {
            HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "Sync is not running"
            }))
        }
    } else {
        HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "Sync session not found"
        }))
    }
}

/// Validate Microsoft Graph configuration
pub async fn get_config_validation(
    req: actix_web::HttpRequest,
) -> impl Responder {
    // Extract claims from cookie auth middleware
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    let mut missing_fields = Vec::new();

    // Check and retrieve each required environment variable
    let client_id = std::env::var("MICROSOFT_CLIENT_ID").ok();
    let client_secret = std::env::var("MICROSOFT_CLIENT_SECRET").ok();
    let tenant_id = std::env::var("MICROSOFT_TENANT_ID").ok();
    let redirect_uri = std::env::var("MICROSOFT_REDIRECT_URI").ok();

    if client_id.is_none() {
        missing_fields.push("MICROSOFT_CLIENT_ID".to_string());
    }
    if client_secret.is_none() {
        missing_fields.push("MICROSOFT_CLIENT_SECRET".to_string());
    }
    if tenant_id.is_none() {
        missing_fields.push("MICROSOFT_TENANT_ID".to_string());
    }
    if redirect_uri.is_none() {
        missing_fields.push("MICROSOFT_REDIRECT_URI".to_string());
    }

    if !missing_fields.is_empty() {
        return HttpResponse::Ok().json(json!({
            "valid": false,
            "message": format!("Missing required environment variables: {}", missing_fields.join(", ")),
            "missing_fields": missing_fields,
            "client_id": client_id,
            "tenant_id": tenant_id,
            "client_secret_configured": client_secret.is_some(),
            "redirect_uri": redirect_uri
        }));
    }

    // All required fields are present
    HttpResponse::Ok().json(json!({
        "valid": true,
        "message": "Microsoft Graph configuration is valid",
        "client_id": client_id,
        "tenant_id": tenant_id,
        "client_secret_configured": client_secret.is_some(),
        "redirect_uri": redirect_uri
    }))
}

/// Get Microsoft Graph connection status
pub async fn get_connection_status(
    req: actix_web::HttpRequest,
    db_pool: web::Data<Pool>,
    ) -> impl Responder {
    let _conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Database connection failed"
        })),
    };
    // Extract claims from cookie auth middleware
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Check if Microsoft is configured via environment variables
    let microsoft_configured = std::env::var("MICROSOFT_CLIENT_ID").is_ok() 
        && std::env::var("MICROSOFT_CLIENT_SECRET").is_ok()
        && std::env::var("MICROSOFT_TENANT_ID").is_ok();
    
    if !microsoft_configured {
        return HttpResponse::Ok().json(ConnectionStatus {
            status: "disconnected".to_string(),
            message: "Microsoft auth provider not configured".to_string(),
            last_sync: None,
            available_entities: vec![],
        });
    }

    // Check environment configuration
    let config_check = check_microsoft_config();
    if let Err(error_msg) = config_check {
        return HttpResponse::Ok().json(ConnectionStatus {
            status: "error".to_string(),
            message: format!("Configuration error: {error_msg}"),
            last_sync: None,
            available_entities: vec![],
        });
    }

    // Configuration looks good
    HttpResponse::Ok().json(ConnectionStatus {
        status: "connected".to_string(),
        message: "Microsoft Graph connection is configured and ready".to_string(),
        last_sync: None, // TODO: Track actual sync times
        available_entities: vec!["users".to_string(), "devices".to_string(), "groups".to_string()],
    })
}

/// Test Microsoft Graph connection
pub async fn test_connection(
    req: actix_web::HttpRequest,
) -> impl Responder {
    // Extract claims from cookie auth middleware
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    tracing::info!("🔬 Testing Microsoft Graph connection");

    // Get Microsoft provider
    let provider = match get_default_microsoft_provider() {
        Ok(provider) => provider,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Microsoft auth provider not found"
        })),
    };

    // Test the connection by making a simple Graph API call
    match test_graph_connection(provider.id).await {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(error) => HttpResponse::Ok().json(json!({
            "success": false,
            "status": "error",
            "message": format!("Connection test failed: {}", error)
        }))
    }
}

/// Sync data from Microsoft Graph
#[instrument(level = "info", skip(req, db_pool, request), fields(entities = ?request.entities))]
pub async fn sync_data(
    req: actix_web::HttpRequest,
    db_pool: web::Data<Pool>,
    request: web::Json<SyncDataRequest>,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Database connection failed"
        })),
    };
    // Extract claims from cookie auth middleware
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Get Microsoft provider
    let provider = match get_default_microsoft_provider() {
        Ok(provider) => provider,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Microsoft auth provider not found"
        })),
    };

    // Determine the primary sync type based on entities
    let entities = request.entities.clone();
    let use_delta = request.use_delta;

    // Log delta sync request (full implementation pending)
    if use_delta {
        info!("Delta sync requested - using incremental sync where supported");
    } else {
        info!("Full sync requested");
    }

    let sync_type = if entities.len() > 1 {
        "multiple".to_string()
    } else if entities.contains(&"devices".to_string()) {
        "devices".to_string()
    } else if entities.contains(&"users".to_string()) {
        "users".to_string()
    } else if entities.contains(&"groups".to_string()) {
        "groups".to_string()
    } else {
        "sync".to_string()
    };

    // Create sync history record in database first
    let new_sync = NewSyncHistory {
        sync_type: sync_type.clone(),
        status: "starting".to_string(),
        started_at: Utc::now().naive_utc(),
        completed_at: None,
        error_message: None,
        records_processed: Some(0),
        records_created: Some(0),
        records_updated: Some(0),
        records_failed: Some(0),
        tenant_id: None,
        is_delta: use_delta,
    };

    let sync_history = match sync_history_repo::create_sync_history(&mut conn, new_sync) {
        Ok(history) => history,
        Err(e) => {
            error!("Failed to create sync history record: {:?}", e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to create sync history record"
            }));
        }
    };

    let session_id = sync_history.id.to_string();
    info!("Created sync history record with ID: {}", session_id);

    // Initialize session and progress tracking
    initialize_sync_session(&session_id);

    update_sync_progress_with_type(&session_id, "initializing", 0, 0, "starting", "Initializing sync process", &sync_type, Some(use_delta));

    // Start the sync process in the background
    let provider_id = provider.id;
    let session_id_clone = session_id.clone();

    tokio::spawn(async move {
        let mut conn = match db_pool.get() {
            Ok(conn) => conn,
            Err(_) => {
                update_sync_progress(&session_id_clone, "error", 0, 0, "error", "Database connection failed");
                return;
            }
        };

        match perform_sync(&mut conn, provider_id, &entities, &session_id_clone, use_delta).await {
            Ok(sync_result) => {
                // Check if sync was cancelled by looking at the result
                if !sync_result.success && sync_result.message.contains("cancelled") {
                    // Update database with cancellation details
                    let update = SyncHistoryUpdate {
                        status: Some("cancelled".to_string()),
                        error_message: Some(sync_result.message),
                        records_processed: Some(sync_result.total_processed as i32),
                        records_created: Some(0),
                        records_updated: Some(sync_result.total_processed as i32),
                        records_failed: Some(sync_result.total_errors as i32),
                        completed_at: Some(Some(Utc::now().naive_utc())),
                    };
                    
                    if let Ok(sync_id) = session_id_clone.parse::<i32>() {
                        let _ = sync_history_repo::update_sync_history(&mut conn, sync_id, update);
                    }
                } else {
                    // Normal completion - update with comprehensive results
                    let status = if sync_result.total_errors > 0 {
                        "completed_with_errors"
                    } else {
                        "completed"
                    };

                    let completion_message = if sync_result.total_errors > 0 {
                        format!(
                            "Sync completed with {} errors: {} items processed ({})", 
                            sync_result.total_errors,
                            sync_result.total_processed,
                            entities.join(", ")
                        )
                    } else {
                        format!(
                            "Sync completed successfully: {} items processed ({})", 
                            sync_result.total_processed,
                            entities.join(", ")
                        )
                    };
                    
                    let update = SyncHistoryUpdate {
                        status: Some(status.to_string()),
                        error_message: if sync_result.total_errors > 0 { 
                            Some(completion_message.clone()) 
                        } else { 
                            None 
                        },
                        records_processed: Some(sync_result.total_processed as i32),
                        records_created: Some(0), // Could track this separately in the future
                        records_updated: Some(sync_result.total_processed as i32),
                        records_failed: Some(sync_result.total_errors as i32),
                        completed_at: Some(Some(Utc::now().naive_utc())),
                    };
                    
                    if let Ok(sync_id) = session_id_clone.parse::<i32>() {
                        match sync_history_repo::update_sync_history(&mut conn, sync_id, update) {
                            Ok(_) => info!("Successfully updated sync history for session {}", sync_id),
                            Err(e) => error!("Failed to update sync history: {:?}", e),
                        }
                    }

                    // Update in-memory progress with completion message
                    update_sync_progress_with_type(
                        &session_id_clone,
                        &sync_type,
                        sync_result.total_processed,
                        sync_result.total_processed,
                        status,
                        &completion_message,
                        &sync_type,
                        None
                    );

                    // Check if background photo sync should start after user sync completes
                    if entities.contains(&"users".to_string()) && sync_result.total_processed > 0 {
                        let background_photo_sync = std::env::var("MSGRAPH_BACKGROUND_PHOTOS")
                            .unwrap_or("true".to_string())
                            .parse::<bool>()
                            .unwrap_or(true);
                        
                        if background_photo_sync {
                            info!("Starting background photo sync for {} processed users", sync_result.total_processed);
                            let db_pool_bg = db_pool.clone();
                            let session_id_bg = session_id_clone.clone();
                            
                            tokio::spawn(async move {
                                // Give the main sync a moment to finish database operations
                                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                                
                                // Get access token for photo sync
                                match fetch_microsoft_graph_users_optimized(provider_id).await {
                                    Ok((_, access_token)) => {
                                        if let Err(e) = background_photo_sync_task(db_pool_bg, provider_id, session_id_bg, access_token).await {
                                            error!("Background photo sync failed: {}", e);
                                        }
                                    },
                                    Err(e) => {
                                        error!("Failed to get access token for background photo sync: {}", e);
                                    }
                                }
                            });
                        }
                    }
                }
            },
            Err(error) => {
                let error_message = format!("Sync failed: {error}");
                error!("Sync failed for session {}: {}", session_id_clone, error);
                
                update_sync_progress_with_type(&session_id_clone, &sync_type, 0, 0, "error", &error_message, &sync_type, None);
                
                // Update database with error
                let update = SyncHistoryUpdate {
                    status: Some("error".to_string()),
                    error_message: Some(error_message),
                    records_processed: Some(0),
                    records_created: Some(0),
                    records_updated: Some(0),
                    records_failed: Some(1),
                    completed_at: Some(Some(Utc::now().naive_utc())),
                };
                
                if let Ok(sync_id) = session_id_clone.parse::<i32>() {
                    let _ = sync_history_repo::update_sync_history(&mut conn, sync_id, update);
                }
            }
        }
    });

    // Return the session ID immediately
    HttpResponse::Ok().json(json!({
        "success": true,
        "message": "Sync started successfully",
        "session_id": session_id
    }))
}





/// Check Microsoft configuration
fn check_microsoft_config() -> Result<(), String> {
    config_utils::get_microsoft_client_id().map_err(|e| format!("Client ID: {e}"))?;
    config_utils::get_microsoft_tenant_id().map_err(|e| format!("Tenant ID: {e}"))?;
    config_utils::get_microsoft_client_secret().map_err(|e| format!("Client Secret: {e}"))?;
    config_utils::get_microsoft_redirect_uri().map_err(|e| format!("Redirect URI: {e}"))?;
    Ok(())
}

/// Test Graph connection by making a simple API call
async fn test_graph_connection(_provider_id: i32) -> Result<serde_json::Value, String> {
    let start_time = std::time::Instant::now();

    let (client, access_token) = get_msgraph_client_and_token().await?;

    // Test the connection with a simple API call to get organization info
    let url = "https://graph.microsoft.com/v1.0/organization";

    let graph_response = client
        .get(url)
        .bearer_auth(&access_token)
        .send()
        .await
        .map_err(|e| format!("Failed to send Microsoft Graph test request: {e}"))?;

    let response_time = start_time.elapsed().as_millis();
    let status = graph_response.status();
    
    if status.is_success() {
        let response_data: serde_json::Value = graph_response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Microsoft Graph response: {e}"))?;

        // Check for organization data
        let org_count = response_data
            .get("value")
            .and_then(|v| v.as_array())
            .map(|arr| arr.len())
            .unwrap_or(0);

        Ok(json!({
            "success": true,
            "status": "connected",
            "message": "Successfully connected to Microsoft Graph API",
            "details": {
                "test_endpoint": "/organization",
                "response_time_ms": response_time,
                "permissions_verified": true,
                "organization_count": org_count
            }
        }))
    } else {
        let error_data: serde_json::Value = graph_response
            .json()
            .await
            .unwrap_or_else(|_| json!({"error": {"message": "Unknown error"}}));

        let error_msg = error_data
            .get("error")
            .and_then(|err| err.get("message"))
            .and_then(|msg| msg.as_str())
            .unwrap_or("Unknown Microsoft Graph error");

        let error_code = error_data
            .get("error")
            .and_then(|err| err.get("code"))
            .and_then(|code| code.as_str())
            .unwrap_or("UnknownError");

        // Provide detailed permission help for 403 Forbidden errors
        if status == 403 {
            return Err(format!(
                "Microsoft Graph API error (403 Forbidden): {error_msg}. \n\n\
                Your Azure AD application is missing required API permissions:\n\
                • Organization.Read.All (to read tenant/organization information)\n\n\
                To fix this:\n\
                1. Go to Azure Portal (portal.azure.com)\n\
                2. Navigate to 'Azure Active Directory' → 'App registrations'\n\
                3. Select your application\n\
                4. Click 'API permissions' → 'Add a permission' → 'Microsoft Graph' → 'Application permissions'\n\
                5. Add: Organization.Read.All, User.Read.All, Device.Read.All, Group.Read.All\n\
                6. Click 'Grant admin consent for [Your Tenant]' (requires Global Admin)\n\
                7. Wait 5-10 minutes for permissions to propagate\n\n\
                Common required permissions for full functionality:\n\
                • Organization.Read.All - Read tenant information\n\
                • User.Read.All - Read user profiles\n\
                • Device.Read.All - Read device information\n\
                • Group.Read.All - Read group information\n\
                • DeviceManagementManagedDevices.Read.All - Read Intune devices\n\n\
                Error Code: {error_code}"
            ));
        }

        Err(format!("Microsoft Graph API error ({} {}): {} (Error Code: {})", status.as_u16(), status.canonical_reason().unwrap_or("Unknown"), error_msg, error_code))
    }
}

/// Perform data synchronization
async fn perform_sync(
    conn: &mut DbConnection,
    provider_id: i32,
    entities: &[String],
    session_id: &str,
    use_delta: bool,
) -> Result<SyncResult, String> {
    let mut results = Vec::new();
    let mut total_processed = 0;
    let mut total_errors = 0;
    let mut was_cancelled = false;

    if use_delta {
        info!("Delta sync mode enabled - using incremental sync where supported");
    } else {
        info!("Full sync mode - fetching all data from Microsoft Graph");
    }

    // Determine the primary sync type based on entities
    let primary_sync_type = if entities.contains(&"devices".to_string()) {
        "devices"
    } else if entities.contains(&"users".to_string()) {
        "users"
    } else if entities.contains(&"groups".to_string()) {
        "groups"
    } else {
        "sync"
    };

    // Don't overwrite entity-specific progress - just process each entity
    for (_index, entity) in entities.iter().enumerate() {
        let sync_progress = match entity.as_str() {
            "users" => sync_users(conn, provider_id, session_id, use_delta).await,
            "devices" => sync_devices(conn, provider_id, session_id, use_delta).await,
            "groups" => sync_groups(conn, provider_id, session_id, use_delta).await,
            _ => {
                total_errors += 1;
                // Update progress with error for unsupported entity
                update_sync_progress_with_type(session_id, entity, 0, 0, "error", &format!("Unsupported entity type: {entity}"), primary_sync_type, None);
                SyncProgress {
                    entity: entity.clone(),
                    processed: 0,
                    total: 0,
                    status: "error".to_string(),
                    errors: vec![format!("Unsupported entity type: {}", entity)],
                }
            }
        };

        // Check if sync was cancelled
        if sync_progress.status == "cancelled" {
            was_cancelled = true;
        }

        total_processed += sync_progress.processed;
        total_errors += sync_progress.errors.len();
        results.push(sync_progress);

        // Break early if cancelled
        if was_cancelled {
            break;
        }
    }

    Ok(SyncResult {
        success: total_errors == 0 && !was_cancelled,
        message: if was_cancelled {
            format!("Sync was cancelled. Processed {total_processed} items")
        } else if total_errors == 0 {
            format!("Successfully synchronized {total_processed} items")
        } else {
            format!("Synchronized {total_processed} items with {total_errors} errors")
        },
        results,
        total_processed,
        total_errors,
    })
}

/// Sync users from Microsoft Graph (optimized with concurrent processing)
#[instrument(level = "info", skip(conn), fields(provider_id = provider_id, session_id = session_id, use_delta = use_delta))]
async fn sync_users(
    conn: &mut DbConnection,
    provider_id: i32,
    session_id: &str,
    use_delta: bool,
) -> SyncProgress {
    let mut stats = UserSyncStats {
        new_users_created: 0,
        existing_users_updated: 0,
        identities_linked: 0,
        errors: Vec::new(),
    };

    let sync_mode_msg = if use_delta { "delta" } else { "full" };
    update_sync_progress(session_id, "users", 0, 0, "running", &format!("Fetching users from Microsoft Graph ({sync_mode_msg})"));

    // Step 1: Fetch users from Microsoft Graph using delta query
    let delta_result = match fetch_microsoft_graph_users_delta(conn, use_delta).await {
        Ok(result) => result,
        Err(error) => {
            update_sync_progress(session_id, "users", 0, 0, "error", &format!("Failed to fetch users: {error}"));
            return SyncProgress {
                entity: "users".to_string(),
                processed: 0,
                total: 0,
                status: "error".to_string(),
                errors: vec![format!("Failed to fetch Microsoft Graph users: {}", error)],
            };
        }
    };

    let microsoft_users = delta_result.users;
    let removed_user_ids = delta_result.removed_user_ids;
    let access_token = delta_result.access_token;

    // Handle removed users first (if delta sync returned any)
    if !removed_user_ids.is_empty() {
        info!(count = removed_user_ids.len(), "Processing removed users from delta response");
        for removed_id in &removed_user_ids {
            // Find the identity for this Microsoft user
            if let Ok(identity) = find_identity_by_provider_user_id(conn, provider_id, removed_id) {
                // Soft-delete or mark the user as inactive
                // For now, we just log it - you might want to deactivate the user
                info!(user_uuid = %identity.user_uuid, ms_id = %removed_id, "User removed from Microsoft - consider deactivating");
                // TODO: Implement user deactivation if desired
            }
        }
    }

    let total_users = microsoft_users.len();
    
    // Check if filtering disabled accounts
    let skip_disabled_accounts = std::env::var("MSGRAPH_SKIP_DISABLED_ACCOUNTS")
        .unwrap_or("true".to_string())
        .parse::<bool>()
        .unwrap_or(true);
    
        // Performance configuration
    let background_photo_sync = std::env::var("MSGRAPH_BACKGROUND_PHOTOS")
        .unwrap_or("true".to_string())
        .parse::<bool>()
        .unwrap_or(true);
    
    if skip_disabled_accounts {
        info!("Fetched {} enabled users from Microsoft Graph (disabled accounts filtered out)", total_users);
    } else {
        info!("Fetched {} users from Microsoft Graph (including disabled accounts)", total_users);
    }
    
    if background_photo_sync {
        info!("Background photo sync enabled - users will be created immediately, photos synced separately");
    } else {
        info!("Inline photo sync enabled - users created with photos during sync (slower)");
    }

    if total_users == 0 {
        debug!("No users found to sync from Microsoft Graph");
        update_sync_progress(session_id, "users", 0, 0, "completed", "No users found to sync");
        return SyncProgress {
            entity: "users".to_string(),
            processed: 0,
            total: 0,
            status: "completed".to_string(),
            errors: Vec::new(),
        };
    }

    info!("Starting user sync: processing {} users concurrently", total_users);
    update_sync_progress(session_id, "users", 0, total_users, "running", &format!("Processing {total_users} users concurrently"));

    // Get concurrency configuration
    let (concurrent_processing, user_batch_size) = get_user_sync_config();

    // Create a shared HTTP client for profile photo downloads
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .pool_max_idle_per_host(concurrent_processing)
        .pool_idle_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| {
            let error_msg = format!("Failed to create HTTP client: {e}");
            update_sync_progress(session_id, "users", 0, total_users, "error", &error_msg);
            SyncProgress {
                entity: "users".to_string(),
                processed: 0,
                total: total_users,
                status: "error".to_string(),
                errors: vec![error_msg],
            }
        }).unwrap();

    // Step 2: Process users in optimized batches
    let mut processed_count = 0;
    let sync_was_cancelled = false;

    // Process users in batches with optimized database operations
    for batch in microsoft_users.chunks(user_batch_size) {
        let batch_start = processed_count;
        let batch_size = batch.len();
        
        update_sync_progress(
            session_id, 
            "users", 
            batch_start, 
            total_users, 
            "running", 
            &format!("Processing batch {}-{} of {}", batch_start + 1, batch_start + batch_size, total_users)
        );

        // Process each user in the batch with optimized profile photo handling
        for ms_user in batch {
            // Check for cancellation before processing each user
            if is_sync_cancelled(session_id) {
                let processed = stats.new_users_created + stats.existing_users_updated + stats.identities_linked;
                let cancel_message = format!("Sync was cancelled by user request. Processed {} of {} users ({} created, {} updated, {} linked)", 
                    processed_count, total_users, stats.new_users_created, stats.existing_users_updated, stats.identities_linked);
                
                // Update progress with cancellation status
                update_sync_progress_with_type(
                    session_id,
                    "users",
                    processed_count,
                    total_users,
                    "cancelled",
                    &cancel_message,
                    "users",
                    None
                );
                
                return SyncProgress {
                    entity: "users".to_string(),
                    processed,
                    total: total_users,
                    status: "cancelled".to_string(),
                    errors: stats.errors,
                };
            }
            
            processed_count += 1;
            
            update_sync_progress_with_type(
                session_id,
                "users",
                processed_count - 1,
                total_users,
                "running",
                &format!("Processing user: {}", ms_user.user_principal_name),
                "users",
                None
            );

            if background_photo_sync {
                // Fast sync without photos
                match process_microsoft_user_no_photos(conn, provider_id, ms_user, &mut stats).await {
                    Ok(_) => {
                        trace!("Successfully processed user (without photos): {}", ms_user.user_principal_name);
                    },
                    Err(error) => {
                        let error_msg = format!("Failed to process user {}: {}", ms_user.user_principal_name, error);
                        warn!("{}", error_msg);
                        stats.errors.push(error_msg);
                    }
                }
            } else {
                // Traditional sync with photos inline
                match process_microsoft_user_optimized_v2(conn, provider_id, ms_user, &mut stats, &access_token, &client).await {
                    Ok(_) => {
                        trace!("Successfully processed user: {}", ms_user.user_principal_name);
                    },
                    Err(error) => {
                        let error_msg = format!("Failed to process user {}: {}", ms_user.user_principal_name, error);
                        warn!("{}", error_msg);
                        stats.errors.push(error_msg);
                    }
                }
            }

            // Update progress more frequently
            if processed_count % 5 == 0 || processed_count == total_users {
                let _processed = stats.new_users_created + stats.existing_users_updated + stats.identities_linked;
                update_sync_progress(
                    session_id, 
                    "users", 
                    processed_count, 
                    total_users, 
                    "running", 
                    &format!("Processed {}/{} users ({} created, {} updated, {} linked, {} errors)", 
                        processed_count, total_users, stats.new_users_created, stats.existing_users_updated, 
                        stats.identities_linked, stats.errors.len())
                );
            }
        }

        // Small delay between batches to be respectful to the API and database
        if processed_count < total_users {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    let processed = stats.new_users_created + stats.existing_users_updated + stats.identities_linked;

    // Only mark as completed if the sync wasn't cancelled
    if !sync_was_cancelled {
        update_sync_progress(
            session_id, 
            "users", 
            total_users, 
            total_users, 
            "completed", 
            &format!("Completed: {} created, {} updated, {} linked, {} errors", 
                stats.new_users_created, stats.existing_users_updated, stats.identities_linked, stats.errors.len())
        );

        // Background photo sync will be handled at the main sync level if configured

        SyncProgress {
            entity: "users".to_string(),
            processed,
            total: total_users,
            status: if stats.errors.is_empty() { "completed".to_string() } else { "completed_with_errors".to_string() },
            errors: stats.errors,
        }
    } else {
        // Should never be reached since early return on cancellation,
        // but just in case, return the cancelled status
        SyncProgress {
            entity: "users".to_string(),
            processed,
            total: total_users,
            status: "cancelled".to_string(),
            errors: stats.errors,
        }
    }
}

/// Fetch users from Microsoft Graph API (optimized version)
async fn fetch_microsoft_graph_users_optimized(_provider_id: i32) -> Result<(Vec<MicrosoftGraphUser>, String), String> {
    let (client, access_token) = get_msgraph_client_and_token().await?;

    // Build the Microsoft Graph API request for users
    // Select fields for MicrosoftGraphUser struct
    // Important: Include proxyAddresses and otherMails for email aliases, and accountEnabled for filtering
    let select_fields = "id,displayName,givenName,surname,mail,userPrincipalName,jobTitle,department,officeLocation,mobilePhone,businessPhones,proxyAddresses,otherMails,accountEnabled";
    
    // Skip disabled accounts by default for performance
    let skip_disabled_accounts = std::env::var("MSGRAPH_SKIP_DISABLED_ACCOUNTS")
        .unwrap_or("true".to_string())
        .parse::<bool>()
        .unwrap_or(true);
    
    // Start with the first page
    let mut url = if skip_disabled_accounts {
        format!(
            "https://graph.microsoft.com/v1.0/users?$select={}&$filter=accountEnabled eq true",
            urlencoding::encode(select_fields)
        )
    } else {
        format!(
            "https://graph.microsoft.com/v1.0/users?$select={}",
            urlencoding::encode(select_fields)
        )
    };

    debug!(url = %url, "Microsoft Graph API query with email alias fields");

    let mut all_users = Vec::new();
    let mut page_count = 0;

    loop {
        page_count += 1;
        debug!("Fetching page {} from Microsoft Graph", page_count);

        // Make the request to Microsoft Graph
        let graph_response = client
            .get(&url)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| format!("Failed to send Microsoft Graph request (page {page_count}): {e}"))?;

        let status = graph_response.status();
        let response_data: serde_json::Value = graph_response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Microsoft Graph response (page {page_count}): {e}"))?;

        if !status.is_success() {
            let error_msg = response_data
                .get("error")
                .and_then(|err| err.get("message"))
                .and_then(|msg| msg.as_str())
                .unwrap_or("Unknown Microsoft Graph error");
            return Err(format!("Microsoft Graph API error (page {page_count}, {status}): {error_msg}"));
        }

        // Parse the users from this page
        let users_array = response_data
            .get("value")
            .and_then(|v| v.as_array())
            .ok_or_else(|| format!("Microsoft Graph response missing 'value' array (page {page_count})"))?;

        let mut page_users = Vec::new();
        for user_value in users_array {
            match serde_json::from_value::<MicrosoftGraphUser>(user_value.clone()) {
                Ok(user) => {
                    page_users.push(user);
                },
                Err(e) => {
                    warn!(page = page_count, error = %e, data = %user_value, "Failed to parse user from Microsoft Graph");
                    // Continue processing other users even if one fails to parse
                }
            }
        }

        debug!("Page {}: Parsed {} users from Microsoft Graph", page_count, page_users.len());
        all_users.extend(page_users);

        // Check if there's a next page
        if let Some(next_link) = response_data.get("@odata.nextLink").and_then(|link| link.as_str()) {
            url = next_link.to_string();
            trace!("Found next page link, continuing to page {}...", page_count + 1);
        } else {
            debug!("No more pages found, finished pagination");
            break;
        }

        // Add a small delay between requests to be respectful to the API
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    info!("Successfully fetched {} users from Microsoft Graph across {} pages", all_users.len(), page_count);
    
    // Log sample users at debug level
    if !all_users.is_empty() && log::log_enabled!(log::Level::Debug) {
        debug!("Sample users fetched: {} total", all_users.len().min(5));
        for (i, user) in all_users.iter().take(5).enumerate() {
            debug!("  {}: {} ({})", 
                i + 1, 
                user.display_name.as_deref().unwrap_or("N/A"), 
                user.user_principal_name
            );
        }
    }

    Ok((all_users, access_token.to_string()))
}

/// Result of a delta sync fetch operation
#[allow(dead_code)]
struct DeltaFetchResult {
    /// Users to create or update
    users: Vec<MicrosoftGraphUser>,
    /// IDs of users that were removed (from @removed marker in delta response)
    removed_user_ids: Vec<String>,
    /// The new delta link to store for next sync (if any)
    new_delta_link: Option<String>,
    /// Whether this was a full sync (no delta token or token expired)
    was_full_sync: bool,
    /// Access token for profile photo downloads
    access_token: String,
}

/// Fetch users from Microsoft Graph API with delta sync support
///
/// Delta queries return only changes since the last sync, making subsequent syncs much faster.
/// If no delta token exists or use_delta is false, performs a full sync.
/// If delta token has expired (410 Gone), automatically falls back to full sync.
#[instrument(level = "info", skip(conn), fields(use_delta = use_delta))]
async fn fetch_microsoft_graph_users_delta(
    conn: &mut DbConnection,
    use_delta: bool,
) -> Result<DeltaFetchResult, String> {
    let (client, access_token) = get_msgraph_client_and_token().await?;

    // Select fields for MicrosoftGraphUser struct
    let select_fields = "id,displayName,givenName,surname,mail,userPrincipalName,jobTitle,department,officeLocation,mobilePhone,businessPhones,proxyAddresses,otherMails,accountEnabled";

    // Check for existing delta token
    let delta_token = if use_delta {
        match crate::repository::sync_history::get_delta_token(conn, "microsoft", "users") {
            Ok(token) => {
                info!("Found existing delta token for users, using incremental sync");
                Some(token.delta_link)
            }
            Err(diesel::result::Error::NotFound) => {
                info!("No delta token found for users, performing initial delta sync");
                None
            }
            Err(e) => {
                warn!(error = %e, "Error retrieving delta token, performing full sync");
                None
            }
        }
    } else {
        info!("Full sync requested, ignoring any existing delta token");
        // Clear existing delta token when doing a full sync
        if let Err(e) = crate::repository::sync_history::delete_delta_token(conn, "microsoft", "users") {
            if !matches!(e, diesel::result::Error::NotFound) {
                warn!(error = %e, "Failed to clear delta token");
            }
        }
        None
    };

    // Build initial URL
    let skip_disabled_accounts = std::env::var("MSGRAPH_SKIP_DISABLED_ACCOUNTS")
        .unwrap_or("true".to_string())
        .parse::<bool>()
        .unwrap_or(true);

    let mut url = match &delta_token {
        Some(link) => {
            // Use the stored delta link for incremental sync
            link.clone()
        }
        None => {
            // Start fresh delta sync or full sync
            // Note: We use /users/delta endpoint even for full sync to get a delta link for next time
            // Important: Delta queries don't support $filter=accountEnabled, we filter client-side instead
            format!(
                "https://graph.microsoft.com/v1.0/users/delta?$select={}",
                urlencoding::encode(select_fields)
            )
        }
    };

    let mut was_full_sync = delta_token.is_none();

    debug!(url = %url, was_full_sync = was_full_sync, "Microsoft Graph delta query URL");

    let mut all_users = Vec::new();
    let mut removed_user_ids = Vec::new();
    let mut page_count = 0;
    let mut new_delta_link = None;

    loop {
        page_count += 1;
        debug!("Fetching delta page {} from Microsoft Graph", page_count);

        let graph_response = client
            .get(&url)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| format!("Failed to send Microsoft Graph delta request (page {page_count}): {e}"))?;

        let status = graph_response.status();

        // Handle 410 Gone - delta token expired, need to do full sync
        if status == reqwest::StatusCode::GONE {
            warn!("Delta token expired (410 Gone), falling back to full sync");

            // Clear the expired token
            let _ = crate::repository::sync_history::delete_delta_token(conn, "microsoft", "users");

            // Reset to full sync - rebuild the initial URL without delta token
            // Note: Delta queries don't support $filter=accountEnabled, we filter client-side instead
            url = format!(
                "https://graph.microsoft.com/v1.0/users/delta?$select={}",
                urlencoding::encode(select_fields)
            );
            was_full_sync = true;
            all_users.clear();
            removed_user_ids.clear();
            page_count = 0;
            continue;
        }

        let response_data: serde_json::Value = graph_response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Microsoft Graph delta response (page {page_count}): {e}"))?;

        if !status.is_success() {
            let error_msg = response_data
                .get("error")
                .and_then(|err| err.get("message"))
                .and_then(|msg| msg.as_str())
                .unwrap_or("Unknown Microsoft Graph error");
            return Err(format!("Microsoft Graph API error (page {page_count}, {status}): {error_msg}"));
        }

        // Parse users from this page
        let users_array = response_data
            .get("value")
            .and_then(|v| v.as_array())
            .ok_or_else(|| format!("Microsoft Graph delta response missing 'value' array (page {page_count})"))?;

        for user_value in users_array {
            // Check if this is a removed user
            if user_value.get("@removed").is_some() {
                if let Some(id) = user_value.get("id").and_then(|v| v.as_str()) {
                    debug!(user_id = %id, "User marked as removed in delta response");
                    removed_user_ids.push(id.to_string());
                }
                continue;
            }

            // Parse normal user
            match serde_json::from_value::<MicrosoftGraphUser>(user_value.clone()) {
                Ok(user) => {
                    // Client-side filtering for disabled accounts (delta queries don't support $filter)
                    if skip_disabled_accounts {
                        if let Some(enabled) = user.account_enabled {
                            if !enabled {
                                trace!(user_id = %user.id, "Skipping disabled user");
                                continue;
                            }
                        }
                    }
                    all_users.push(user);
                }
                Err(e) => {
                    warn!(page = page_count, error = %e, data = %user_value, "Failed to parse user from delta response");
                }
            }
        }

        debug!("Delta page {}: {} users, {} removed", page_count, all_users.len(), removed_user_ids.len());

        // Check for deltaLink (end of changes) or nextLink (more pages)
        if let Some(delta_link) = response_data.get("@odata.deltaLink").and_then(|v| v.as_str()) {
            new_delta_link = Some(delta_link.to_string());
            debug!("Received deltaLink, finished fetching changes");
            break;
        } else if let Some(next_link) = response_data.get("@odata.nextLink").and_then(|v| v.as_str()) {
            url = next_link.to_string();
            trace!("Found nextLink, continuing to page {}...", page_count + 1);
        } else {
            warn!("No deltaLink or nextLink found in response");
            break;
        }

        // Small delay between requests
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Store the new delta link for next sync
    if let Some(ref delta_link) = new_delta_link {
        match crate::repository::sync_history::upsert_delta_token(conn, "microsoft", "users", delta_link) {
            Ok(_) => info!("Saved delta token for users"),
            Err(e) => warn!(error = %e, "Failed to save delta token for users"),
        }
    }

    info!(
        users = all_users.len(),
        removed = removed_user_ids.len(),
        pages = page_count,
        was_full_sync = was_full_sync,
        "Delta fetch completed"
    );

    Ok(DeltaFetchResult {
        users: all_users,
        removed_user_ids,
        new_delta_link,
        was_full_sync,
        access_token: access_token.to_string(),
    })
}

/// Process a single Microsoft Graph user
#[allow(dead_code)]
async fn process_microsoft_user(
    conn: &mut DbConnection,
    provider_id: i32,
    ms_user: &MicrosoftGraphUser,
    stats: &mut UserSyncStats,
    access_token: &str,
) -> Result<(), String> {
    // Step 1: Check if this Microsoft user already has an identity in our system
    if let Ok(existing_identity) = find_identity_by_provider_user_id(conn, provider_id, &ms_user.id) {
        // User already has Microsoft identity - update existing user and identity
        return update_existing_microsoft_user(conn, ms_user, existing_identity, stats, access_token).await;
    }

    // Step 2: Check if a local user exists with matching email
    let email = ms_user.mail.as_ref().unwrap_or(&ms_user.user_principal_name);
    
    if let Ok(existing_user) = user_repo::get_user_by_email(email, conn) {
        // Local user exists but no Microsoft identity - link them
        return link_existing_user_to_microsoft(conn, provider_id, ms_user, existing_user, stats, access_token).await;
    }

    // Step 3: No existing user found - create new user with Microsoft identity
    create_new_user_from_microsoft(conn, provider_id, ms_user, stats, access_token).await
}

/// Process a single Microsoft Graph user with optimized HTTP client (v2)
async fn process_microsoft_user_optimized_v2(
    conn: &mut DbConnection,
    provider_id: i32,
    ms_user: &MicrosoftGraphUser,
    stats: &mut UserSyncStats,
    access_token: &str,
    client: &reqwest::Client,
) -> Result<(), String> {
    // Step 1: Check if this Microsoft user already has an identity in our system
    if let Ok(existing_identity) = find_identity_by_provider_user_id(conn, provider_id, &ms_user.id) {
        // User already has Microsoft identity - update existing user and identity
        return update_existing_microsoft_user_optimized(conn, ms_user, existing_identity, stats, access_token, client).await;
    }

    // Step 2: Extract all email addresses from Microsoft Graph user
    let emails = extract_user_emails(ms_user);
    let email_addresses: Vec<String> = emails.iter().map(|(email, _, _)| email.clone()).collect();
    
    // Step 3: Check if any user exists with any of these email addresses
    if let Ok(Some(existing_user)) = user_emails_repo::find_user_by_any_of_emails(conn, &email_addresses) {
        // Local user exists but no Microsoft identity - link them
        return link_existing_user_to_microsoft_optimized(conn, provider_id, ms_user, existing_user, stats, access_token, client).await;
    }

    // Step 4: No existing user found - create new user with Microsoft identity
    create_new_user_from_microsoft_optimized(conn, provider_id, ms_user, stats, access_token, client).await
}

/// Find identity by provider and user ID
fn find_identity_by_provider_user_id(
    conn: &mut DbConnection,
    _provider_id: i32,
    provider_user_id: &str,
) -> Result<UserAuthIdentity, diesel::result::Error> {
    use crate::schema::user_auth_identities;
    
    user_auth_identities::table
                    .filter(user_auth_identities::provider_type.eq("microsoft"))
        .filter(user_auth_identities::external_id.eq(provider_user_id))
        .first::<UserAuthIdentity>(conn)
}

/// Update existing user who already has Microsoft identity
#[allow(dead_code)]
async fn update_existing_microsoft_user(
    conn: &mut DbConnection,
    ms_user: &MicrosoftGraphUser,
    existing_identity: UserAuthIdentity,
    stats: &mut UserSyncStats,
    access_token: &str,
) -> Result<(), String> {
    debug!(user_principal_name = %ms_user.user_principal_name, "Updating existing Microsoft user");

    // Get the associated user
    let user = user_repo::get_user_by_uuid(&existing_identity.user_uuid, conn)
        .map_err(|e| format!("Failed to get user by UUID {}: {}", existing_identity.user_uuid, e))?;

    // Update user information with latest from Microsoft Graph
    let updated_name = ms_user.display_name.as_ref().unwrap_or(&user.name);
    let _updated_email = ms_user.mail.as_ref().unwrap_or(&ms_user.user_principal_name);

    // Only update core fields, preserve role/pronouns/avatars, but update timestamp
    let user_update = crate::models::UserUpdate {
        name: if updated_name != &user.name { Some(updated_name.clone()) } else { None },
        role: None, // Don't change role during sync
        pronouns: None, // Preserve pronouns
        avatar_url: None, // Preserve avatar
        banner_url: None, // Preserve banner
        avatar_thumb: None, // Preserve avatar thumb
        theme: None, // Preserve user's theme preference
        microsoft_uuid: None, // Don't update Microsoft UUID in this function
        updated_at: Some(chrono::Utc::now().naive_utc()), // Update timestamp to preserve created_at
    };

    // Update user if there are changes
    if user_update.name.is_some() || false /* email removed */ {
        user_repo::update_user(&user.uuid, user_update, conn)
            .map_err(|e| format!("Failed to update user: {e}"))?;
        debug!(user_name = %user.name, "Updated user information");
    }

    // Update identity data with latest from Microsoft Graph
    let identity_data = serde_json::to_value(ms_user)
        .map_err(|e| format!("Failed to serialize Microsoft user data: {e}"))?;

    update_identity_data(conn, existing_identity.id, Some(identity_data))
        .map_err(|e| format!("Failed to update identity data: {e}"))?;

    // Sync profile photo
    let client = reqwest::Client::new();
    if let Ok(photo_urls) = sync_user_profile_photo(&client, access_token, ms_user, &utils::uuid_to_string(&user.uuid)).await {
        trace!(user_name = %user.name, avatar_url = ?photo_urls.avatar_url, avatar_thumb = ?photo_urls.avatar_thumb, "sync_user_profile_photo returned URLs");
        if let Err(e) = update_user_avatar_by_id(conn, &user.uuid, photo_urls.avatar_url, photo_urls.avatar_thumb).await {
            warn!(user_name = %user.name, error = %e, "Failed to update avatar for user");
        } else {
            debug!(user_name = %user.name, "Successfully updated avatar for user");
        }
    } else {
        debug!(user_name = %user.name, "No profile photo available for user");
    }

    stats.existing_users_updated += 1;
    Ok(())
}

/// Update identity data for an existing identity
fn update_identity_data(
    conn: &mut DbConnection,
    identity_id: i32,
    identity_data: Option<serde_json::Value>,
) -> Result<UserAuthIdentity, diesel::result::Error> {
    use crate::schema::user_auth_identities;
    
    diesel::update(user_auth_identities::table.find(identity_id))
        .set(user_auth_identities::metadata.eq(identity_data))
        .get_result::<UserAuthIdentity>(conn)
}

/// Link existing local user to Microsoft identity
#[allow(dead_code)]
async fn link_existing_user_to_microsoft(
    conn: &mut DbConnection,
    _provider_id: i32,
    ms_user: &MicrosoftGraphUser,
    existing_user: User,
    stats: &mut UserSyncStats,
    access_token: &str,
) -> Result<(), String> {
    debug!(user_name = %existing_user.name, user_principal_name = %ms_user.user_principal_name, "Linking existing user to Microsoft");

    // Create Microsoft identity for existing user
    let identity_data = serde_json::to_value(ms_user)
        .map_err(|e| format!("Failed to serialize Microsoft user data: {e}"))?;

    let new_identity = NewUserAuthIdentity {
        user_uuid: existing_user.uuid,
        provider_type: "microsoft".to_string(),
        external_id: ms_user.id.clone(),
        email: ms_user.mail.clone(),
        metadata: Some(identity_data),
        password_hash: None, // Microsoft identities don't have password hashes
    };

    identity_repo::create_identity(new_identity, conn)
        .map_err(|e| format!("Failed to create Microsoft identity: {e}"))?;

    // Optionally update user information with Microsoft data
    let updated_name = ms_user.display_name.as_ref().unwrap_or(&existing_user.name);
    if updated_name != &existing_user.name {
        let user_update = crate::models::UserUpdate {
            name: Some(updated_name.clone()),
            role: None,
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            theme: None, // Preserve user's theme preference
            microsoft_uuid: None, // Don't update Microsoft UUID in this function
            updated_at: Some(chrono::Utc::now().naive_utc()),
        };

        user_repo::update_user(&existing_user.uuid, user_update, conn)
            .map_err(|e| format!("Failed to update user name: {e}"))?;
    }

    // Sync profile photo
    let client = reqwest::Client::new();
    if let Ok(photo_urls) = sync_user_profile_photo(&client, access_token, ms_user, &utils::uuid_to_string(&existing_user.uuid)).await {
        trace!(user_name = %existing_user.name, avatar_url = ?photo_urls.avatar_url, avatar_thumb = ?photo_urls.avatar_thumb, "sync_user_profile_photo returned URLs");
        if let Err(e) = update_user_avatar_by_id(conn, &existing_user.uuid, photo_urls.avatar_url, photo_urls.avatar_thumb).await {
            warn!(user_name = %existing_user.name, error = %e, "Failed to update avatar for user");
        } else {
            debug!(user_name = %existing_user.name, "Successfully updated avatar for user");
        }
    } else {
        debug!(user_name = %existing_user.name, "No profile photo available for user");
    }

    stats.identities_linked += 1;
    Ok(())
}

/// Create new user from Microsoft Graph data
#[allow(dead_code)]
async fn create_new_user_from_microsoft(
    conn: &mut DbConnection,
    _provider_id: i32,
    ms_user: &MicrosoftGraphUser,
    stats: &mut UserSyncStats,
    access_token: &str,
) -> Result<(), String> {
    debug!(user_principal_name = %ms_user.user_principal_name, "Creating new user from Microsoft");

    // Generate UUID for new user
    let user_uuid = Uuid::now_v7();
    
    // Determine name (prefer displayName, fallback to givenName + surname, fallback to userPrincipalName)
    let name = ms_user.display_name.clone()
        .or_else(|| {
            match (&ms_user.given_name, &ms_user.surname) {
                (Some(first), Some(last)) => Some(format!("{first} {last}")),
                (Some(first), None) => Some(first.clone()),
                (None, Some(last)) => Some(last.clone()),
                _ => None,
            }
        })
        .unwrap_or_else(|| ms_user.user_principal_name.clone());

    // Use mail if available, otherwise use userPrincipalName
    let email = ms_user.mail.as_ref().unwrap_or(&ms_user.user_principal_name);

    // Create new user with default role 'user'
    // Create OAuth user for Microsoft Graph integration
    let new_user = utils::NewUserBuilder::oauth_user(name.clone(), email.clone(), crate::models::UserRole::User)
        .with_uuid(user_uuid)
        .build();

    let created_user = user_repo::create_user(new_user, conn)
        .map_err(|e| format!("Failed to create user: {e}"))?;

    // Create Microsoft identity for the new user
    let identity_data = serde_json::to_value(ms_user)
        .map_err(|e| format!("Failed to serialize Microsoft user data: {e}"))?;

    let new_identity = NewUserAuthIdentity {
        user_uuid: created_user.uuid,
        provider_type: "microsoft".to_string(),
        external_id: ms_user.id.clone(),
        email: ms_user.mail.clone(),
        metadata: Some(identity_data),
        password_hash: None,
    };

    identity_repo::create_identity(new_identity, conn)
        .map_err(|e| format!("Failed to create Microsoft identity: {e}"))?;

    // Sync profile photo
    let client = reqwest::Client::new();
    if let Ok(photo_urls) = sync_user_profile_photo(&client, access_token, ms_user, &utils::uuid_to_string(&user_uuid)).await {
        trace!(user_name = %name, avatar_url = ?photo_urls.avatar_url, avatar_thumb = ?photo_urls.avatar_thumb, "sync_user_profile_photo returned URLs for new user");
        if let Err(e) = update_user_avatar_by_id(conn, &created_user.uuid, photo_urls.avatar_url, photo_urls.avatar_thumb).await {
            warn!(user_name = %name, error = %e, "Failed to update avatar for user");
        } else {
            debug!(user_name = %name, "Successfully updated avatar for new user");
        }
    } else {
        debug!(user_name = %name, "No profile photo available for user");
    }

    info!(user_name = %name, email = %email, "Created new user");
    stats.new_users_created += 1;
    Ok(())
}

/// Update existing user who already has Microsoft identity (optimized version)
#[instrument(level = "debug", skip(conn, ms_user, stats, access_token, client), fields(user_principal_name = %ms_user.user_principal_name, user_uuid = %existing_identity.user_uuid))]
async fn update_existing_microsoft_user_optimized(
    conn: &mut DbConnection,
    ms_user: &MicrosoftGraphUser,
    existing_identity: UserAuthIdentity,
    stats: &mut UserSyncStats,
    access_token: &str,
    client: &reqwest::Client,
) -> Result<(), String> {
    // User info is in the span context

    // Get the associated user
    let user = user_repo::get_user_by_uuid(&existing_identity.user_uuid, conn)
        .map_err(|e| format!("Failed to get user by UUID {}: {}", existing_identity.user_uuid, e))?;

    // Extract all email addresses from Microsoft Graph
    let emails = extract_user_emails(ms_user);
    let _primary_email = emails.first().map(|(email, _, _)| email.clone())
        .unwrap_or_else(|| ms_user.user_principal_name.clone());

    // Update user information with latest from Microsoft Graph
    let updated_name = ms_user.display_name.as_ref().unwrap_or(&user.name);

    // Only update core fields, preserve role/pronouns/avatars, but update timestamp and Microsoft UUID
    let user_update = crate::models::UserUpdate {
        name: if updated_name != &user.name { Some(updated_name.clone()) } else { None },
        role: None, // Don't change role during sync
        pronouns: None, // Preserve pronouns
        avatar_url: None, // Preserve avatar
        banner_url: None, // Preserve banner
        avatar_thumb: None, // Preserve avatar thumb
        theme: None, // Preserve user's theme preference
        microsoft_uuid: Some(utils::parse_uuid(&ms_user.id).map_err(|_| "Invalid Microsoft UUID format")?), // Always update Microsoft UUID with proper conversion
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    // Update user if there are changes
    if user_update.name.is_some() || false /* email removed */ || user_update.microsoft_uuid.is_some() {
        user_repo::update_user(&user.uuid, user_update, conn)
            .map_err(|e| format!("Failed to update user: {e}"))?;
        debug!(user_name = %user.name, "Updated user information");
    }

    // Store all email addresses
    let email_data: Vec<(String, String, bool, String)> = emails
        .into_iter()
        .map(|(email, email_type, verified)| (email, email_type, verified, "microsoft".to_string()))
        .collect();

    if !email_data.is_empty() {
        debug!("Storing {} email addresses for user: {}", email_data.len(), user.name);

        match user_emails_repo::add_multiple_emails(conn, &user.uuid, email_data.clone()) {
            Ok(stored_emails) => {
                let added_count = stored_emails.len();
                debug!("Successfully stored {} email addresses for user: {}", added_count, user.name);

                // Clean up any Microsoft emails that are no longer present
                let current_emails: Vec<String> = email_data.iter().map(|(email, _, _, _)| email.clone()).collect();
                match user_emails_repo::cleanup_obsolete_emails(conn, &user.uuid, &current_emails, "microsoft") {
                    Ok(cleaned_count) => {
                        if cleaned_count > 0 {
                            debug!("Cleaned up {} obsolete Microsoft email addresses for user: {}", cleaned_count, user.name);
                        }
                    },
                    Err(e) => {
                        warn!("Failed to cleanup obsolete emails for user {}: {}", user.name, e);
                    }
                }
            },
            Err(e) => {
                error!("Failed to store email addresses for user {}: {}", user.name, e);
                stats.errors.push(format!("Failed to store emails for user {}: {}", user.name, e));
            }
        }
    } else {
        trace!("No email addresses to store for user: {}", user.name);
    }

    // Update identity data with latest from Microsoft Graph
    let identity_data = serde_json::to_value(ms_user)
        .map_err(|e| format!("Failed to serialize Microsoft user data: {e}"))?;

    update_identity_data(conn, existing_identity.id, Some(identity_data))
        .map_err(|e| format!("Failed to update identity data: {e}"))?;

    // Sync profile photo using optimized client
    if let Ok(photo_urls) = sync_user_profile_photo(client, access_token, ms_user, &utils::uuid_to_string(&user.uuid)).await {
        if let Err(e) = update_user_avatar_by_id(conn, &user.uuid, photo_urls.avatar_url, photo_urls.avatar_thumb).await {
            warn!(user_name = %user.name, error = %e, "Failed to update avatar for user");
        }
    }

    stats.existing_users_updated += 1;
    Ok(())
}

/// Link existing local user to Microsoft identity (optimized version)
#[instrument(level = "debug", skip(conn, ms_user, stats, access_token, client), fields(existing_user_email = %existing_user.name, user_principal_name = %ms_user.user_principal_name, provider_id = provider_id))]
async fn link_existing_user_to_microsoft_optimized(
    conn: &mut DbConnection,
    provider_id: i32,
    ms_user: &MicrosoftGraphUser,
    existing_user: User,
    stats: &mut UserSyncStats,
    access_token: &str,
    client: &reqwest::Client,
) -> Result<(), String> {
    // Linking info is in the span context

    // Create Microsoft identity for existing user
    let identity_data = serde_json::to_value(ms_user)
        .map_err(|e| format!("Failed to serialize Microsoft user data: {e}"))?;

    let new_identity = NewUserAuthIdentity {
        user_uuid: existing_user.uuid,
        provider_type: "microsoft".to_string(),
        external_id: ms_user.id.clone(),
        email: ms_user.mail.clone(),
        metadata: Some(identity_data),
        password_hash: None,
    };

    identity_repo::create_identity(new_identity, conn)
        .map_err(|e| format!("Failed to create Microsoft identity: {e}"))?;

    // Extract all email addresses from Microsoft Graph
    let emails = extract_user_emails(ms_user);
    let _primary_email = emails.first().map(|(email, _, _)| email.clone())
        .unwrap_or_else(|| ms_user.user_principal_name.clone());

    // Update user information with Microsoft data and store Microsoft UUID
    let updated_name = ms_user.display_name.as_ref().unwrap_or(&existing_user.name);
    let user_update = crate::models::UserUpdate {
        name: if updated_name != &existing_user.name { Some(updated_name.clone()) } else { None },
        role: None,
        pronouns: None,
        avatar_url: None,
        banner_url: None,
        avatar_thumb: None,
        theme: None, // Preserve user's theme preference
        microsoft_uuid: Some(utils::parse_uuid(&ms_user.id).map_err(|_| "Invalid Microsoft UUID format")?), // Store Microsoft UUID with proper conversion
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    // Always update to store the Microsoft UUID
    user_repo::update_user(&existing_user.uuid, user_update, conn)
        .map_err(|e| format!("Failed to update user with Microsoft UUID: {e}"))?;

    // Store all email addresses
    let email_data: Vec<(String, String, bool, String)> = emails
        .into_iter()
        .map(|(email, email_type, verified)| (email, email_type, verified, "microsoft".to_string()))
        .collect();

    if !email_data.is_empty() {
        debug!("Storing {} email addresses for linked user: {}", email_data.len(), existing_user.name);

        match user_emails_repo::add_multiple_emails(conn, &existing_user.uuid, email_data.clone()) {
            Ok(stored_emails) => {
                let added_count = stored_emails.len();
                debug!("Successfully stored {} email addresses for linked user: {}", added_count, existing_user.name);
            },
            Err(e) => {
                error!("Failed to store email addresses for linked user {}: {}", existing_user.name, e);
                stats.errors.push(format!("Failed to store emails for linked user {}: {}", existing_user.name, e));
            }
        }
    } else {
        trace!("No email addresses to store for linked user: {}", existing_user.name);
    }

    // Sync profile photo using optimized client
    if let Ok(photo_urls) = sync_user_profile_photo(client, access_token, ms_user, &utils::uuid_to_string(&existing_user.uuid)).await {
        if let Err(e) = update_user_avatar_by_id(conn, &existing_user.uuid, photo_urls.avatar_url, photo_urls.avatar_thumb).await {
            warn!("Failed to update avatar for user {}: {}", existing_user.name, e);
        }
    }

    stats.identities_linked += 1;
    Ok(())
}

/// Create new user from Microsoft Graph data (optimized version)
#[instrument(level = "debug", skip(conn, ms_user, stats, access_token, client), fields(user_principal_name = %ms_user.user_principal_name, provider_id = provider_id))]
async fn create_new_user_from_microsoft_optimized(
    conn: &mut DbConnection,
    provider_id: i32,
    ms_user: &MicrosoftGraphUser,
    stats: &mut UserSyncStats,
    access_token: &str,
    client: &reqwest::Client,
) -> Result<(), String> {
    // User info is in the span context

    // Generate UUID for new user (this is our local UUID, different from Microsoft's)
    let _user_uuid = Uuid::now_v7().to_string();
    
    // Extract all email addresses from Microsoft Graph
    let emails = extract_user_emails(ms_user);
    let primary_email = emails.first().map(|(email, _, _)| email.clone())
        .unwrap_or_else(|| ms_user.user_principal_name.clone());

    // Determine name (prefer displayName, fallback to givenName + surname, fallback to userPrincipalName)
    let name = ms_user.display_name.clone()
        .or_else(|| {
            match (&ms_user.given_name, &ms_user.surname) {
                (Some(first), Some(last)) => Some(format!("{first} {last}")),
                (Some(first), None) => Some(first.clone()),
                (None, Some(last)) => Some(last.clone()),
                _ => None,
            }
        })
        .unwrap_or_else(|| ms_user.user_principal_name.clone());

    // Create new user with default role 'user' and store Microsoft UUID
    let user_uuid = Uuid::now_v7();
    // Create Microsoft user with UUID
    let microsoft_uuid = Some(utils::parse_uuid(&ms_user.id).map_err(|_| "Invalid Microsoft UUID format")?);
    let new_user = utils::NewUserBuilder::microsoft_user(
        name.clone(),
        primary_email.clone(),
        crate::models::UserRole::User,
        microsoft_uuid
    )
    .with_uuid(user_uuid)
    .build();

    let created_user = user_repo::create_user(new_user, conn)
        .map_err(|e| format!("Failed to create user: {e}"))?;

    // Store all email addresses
    let email_data: Vec<(String, String, bool, String)> = emails
        .into_iter()
        .map(|(email, email_type, verified)| (email, email_type, verified, "microsoft".to_string()))
        .collect();

    if !email_data.is_empty() {
        debug!("Storing {} email addresses for new user: {}", email_data.len(), name);

        match user_emails_repo::add_multiple_emails(conn, &created_user.uuid, email_data.clone()) {
            Ok(stored_emails) => {
                let added_count = stored_emails.len();
                debug!("Successfully stored {} email addresses for new user: {}", added_count, name);
            },
            Err(e) => {
                error!("Failed to store email addresses for new user {}: {}", name, e);
                stats.errors.push(format!("Failed to store emails for new user {name}: {e}"));
            }
        }
    } else {
        trace!("No email addresses to store for new user: {}", name);
    }

    // Create Microsoft identity for the new user
    let identity_data = serde_json::to_value(ms_user)
        .map_err(|e| format!("Failed to serialize Microsoft user data: {e}"))?;

    let new_identity = NewUserAuthIdentity {
        user_uuid: created_user.uuid,
        provider_type: "microsoft".to_string(),
        external_id: ms_user.id.clone(),
        email: ms_user.mail.clone(),
        metadata: Some(identity_data),
        password_hash: None,
    };

    identity_repo::create_identity(new_identity, conn)
        .map_err(|e| format!("Failed to create Microsoft identity: {e}"))?;

    // Sync profile photo using optimized client
    if let Ok(photo_urls) = sync_user_profile_photo(client, access_token, ms_user, &utils::uuid_to_string(&user_uuid)).await {
        if let Err(e) = update_user_avatar_by_id(conn, &created_user.uuid, photo_urls.avatar_url, photo_urls.avatar_thumb).await {
            warn!("Failed to update avatar for user {}: {}", name, e);
        }
    }

    info!("Created new user: {} with {} email addresses", name, email_data.len());
    stats.new_users_created += 1;
    Ok(())
}

/// Sync devices from Microsoft Graph (Intune managed devices)
#[instrument(level = "info", skip(conn), fields(provider_id = provider_id, session_id = session_id, use_delta = use_delta))]
async fn sync_devices(
    conn: &mut DbConnection,
    provider_id: i32,
    session_id: &str,
    use_delta: bool,
) -> SyncProgress {
    let mut stats = DeviceSyncStats {
        new_devices_created: 0,
        existing_devices_updated: 0,
        devices_assigned: 0,
        errors: Vec::new(),
    };

    let sync_mode_msg = if use_delta { "delta" } else { "full" };
    update_sync_progress_with_type(session_id, "devices", 0, 0, "running", &format!("Fetching devices from Microsoft Graph ({sync_mode_msg})"), "devices", None);

    // Step 1: Fetch devices from Microsoft Graph using delta query
    let delta_result = match fetch_microsoft_graph_devices_delta(conn, use_delta).await {
        Ok(result) => result,
        Err(error) => {
            update_sync_progress_with_type(session_id, "devices", 0, 0, "error", &format!("Failed to fetch devices: {error}"), "devices", None);
            return SyncProgress {
                entity: "devices".to_string(),
                processed: 0,
                total: 0,
                status: "error".to_string(),
                errors: vec![format!("Failed to fetch Microsoft Graph devices: {}", error)],
            };
        }
    };

    let entra_devices = delta_result.devices;
    let removed_device_ids = delta_result.removed_device_ids;
    let _access_token = delta_result.access_token;

    // Handle removed devices first (if delta sync returned any)
    if !removed_device_ids.is_empty() {
        info!(count = removed_device_ids.len(), "Processing removed devices from delta response");
        for removed_id in &removed_device_ids {
            // Find the device by Entra ID and mark it as removed/inactive
            // The removed_id is the Entra Object ID from the /devices endpoint
            if let Ok(device) = device_repo::get_device_by_entra_id(conn, removed_id) {
                info!(device_id = %device.id, device_name = %device.name, entra_id = %removed_id, "Device removed from Entra ID - consider deactivating");
                // TODO: Implement device deactivation if desired
            }
        }
    }

    let total_devices = entra_devices.len();
    info!(device_count = total_devices, was_delta = !delta_result.was_full_sync, "Fetched devices from Entra ID");

    if total_devices == 0 {
        update_sync_progress_with_type(session_id, "devices", 0, 0, "completed", "No devices found to sync", "devices", None);
        return SyncProgress {
        entity: "devices".to_string(),
        processed: 0,
        total: 0,
        status: "completed".to_string(),
            errors: Vec::new(),
        };
    }

    // Note: No need to resolve Entra Object IDs - the /devices endpoint already returns them as the `id` field
    update_sync_progress_with_type(session_id, "devices", 0, total_devices, "running", &format!("Processing {total_devices} Entra devices"), "devices", None);

    // Step 2: Process Entra devices
    let mut processed_count = 0;

    for entra_device in entra_devices {
        // Check for cancellation before processing each device
        if is_sync_cancelled(session_id) {
            let processed = stats.new_devices_created + stats.existing_devices_updated;
            let cancel_message = format!("Sync was cancelled by user request. Processed {} of {} devices ({} created, {} updated)",
                processed_count, total_devices, stats.new_devices_created, stats.existing_devices_updated);

            update_sync_progress_with_type(
                session_id,
                "devices",
                processed_count,
                total_devices,
                "cancelled",
                &cancel_message,
                "devices",
                None
            );

            return SyncProgress {
                entity: "devices".to_string(),
                processed,
                total: total_devices,
                status: "cancelled".to_string(),
                errors: stats.errors,
            };
        }

        processed_count += 1;

        let device_name = entra_device.display_name.as_deref().unwrap_or(&entra_device.id);

        update_sync_progress_with_type(
            session_id,
            "devices",
            processed_count - 1,
            total_devices,
            "running",
            &format!("Processing device: {device_name}"),
            "devices",
            None
        );

        match process_entra_device(conn, provider_id, &entra_device, &mut stats).await {
            Ok(_) => {
                debug!(device_name = %device_name, "Successfully processed Entra device");
            },
            Err(error) => {
                let error_msg = format!("Failed to process device {device_name}: {error}");
                error!(device_name = %device_name, error = %error, "Failed to process Entra device");
                stats.errors.push(error_msg);
            }
        }

        // Update progress more frequently
        if processed_count % 5 == 0 || processed_count == total_devices {
            let _processed = stats.new_devices_created + stats.existing_devices_updated;
            update_sync_progress_with_type(
                session_id,
                "devices",
                processed_count,
                total_devices,
                "running",
                &format!("Processed {}/{} devices ({} created, {} updated, {} assigned, {} errors)",
                    processed_count, total_devices, stats.new_devices_created, stats.existing_devices_updated,
                    stats.devices_assigned, stats.errors.len()),
                "devices",
                None
            );
        }

        // Small delay between devices to be respectful to the database
        if processed_count < total_devices {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    let processed = stats.new_devices_created + stats.existing_devices_updated;

    update_sync_progress_with_type(
        session_id,
        "devices",
        total_devices,
        total_devices,
        "completed",
        &format!("Completed: {} created, {} updated, {} assigned, {} errors",
            stats.new_devices_created, stats.existing_devices_updated, stats.devices_assigned, stats.errors.len()),
        "devices",
        None
    );

    SyncProgress {
        entity: "devices".to_string(),
        processed,
        total: total_devices,
        status: if stats.errors.is_empty() { "completed".to_string() } else { "completed_with_errors".to_string() },
        errors: stats.errors,
    }
}

/// Sync groups from Microsoft Graph
#[instrument(level = "info", skip(conn), fields(session_id = session_id, use_delta = use_delta))]
async fn sync_groups(
    conn: &mut DbConnection,
    _provider_id: i32,
    session_id: &str,
    use_delta: bool,
) -> SyncProgress {
    let mut stats = GroupSyncStats::default();

    let sync_mode_msg = if use_delta { "delta" } else { "full" };
    update_sync_progress_with_type(session_id, "groups", 0, 0, "running", &format!("Fetching groups from Microsoft Graph ({sync_mode_msg})"), "groups", None);

    // Load sync configuration
    let config = GroupSyncConfig::from_env();

    // Step 1: Fetch groups from Microsoft Graph using delta query
    let delta_result = match fetch_microsoft_graph_groups_delta(conn, use_delta).await {
        Ok(result) => result,
        Err(error) => {
            update_sync_progress_with_type(session_id, "groups", 0, 0, "error", &format!("Failed to fetch groups: {error}"), "groups", None);
            return SyncProgress {
                entity: "groups".to_string(),
                processed: 0,
                total: 0,
                status: "error".to_string(),
                errors: vec![format!("Failed to fetch Microsoft Graph groups: {}", error)],
            };
        }
    };

    let access_token = delta_result.access_token;
    let was_full_sync = delta_result.was_full_sync;

    // Filter groups based on configuration (only for non-removed groups)
    let groups_to_sync: Vec<_> = delta_result.groups
        .into_iter()
        .filter(|item| {
            // Always include removed groups for cleanup
            if item.is_removed {
                return true;
            }
            // Filter non-removed groups based on configuration
            if let Some(ref g) = item.group {
                config.should_sync_group(g)
            } else {
                false
            }
        })
        .collect();

    let total_groups = groups_to_sync.len();
    let removed_count = groups_to_sync.iter().filter(|g| g.is_removed).count();
    info!(
        group_count = total_groups,
        removed_count = removed_count,
        was_delta = !was_full_sync,
        "Fetched and filtered groups from Microsoft Graph"
    );

    if total_groups == 0 {
        update_sync_progress_with_type(session_id, "groups", 0, 0, "completed", "No groups found to sync (check filter settings)", "groups", None);
        return SyncProgress {
            entity: "groups".to_string(),
            processed: 0,
            total: 0,
            status: "completed".to_string(),
            errors: Vec::new(),
        };
    }

    update_sync_progress_with_type(session_id, "groups", 0, total_groups, "running", &format!("Processing {total_groups} groups"), "groups", None);

    // Create HTTP client for member fetches (needed for full sync or fallback)
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .pool_max_idle_per_host(10)
        .pool_idle_timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_default();

    // Step 2: Process each group
    let mut processed_count = 0;
    let mut synced_external_ids: Vec<String> = Vec::new();

    for group_item in groups_to_sync {
        // Check for cancellation
        if is_sync_cancelled(session_id) {
            let cancel_message = format!(
                "Sync cancelled. Processed {} of {} groups ({} created, {} updated)",
                processed_count, total_groups, stats.groups_created, stats.groups_updated
            );
            update_sync_progress_with_type(session_id, "groups", processed_count, total_groups, "cancelled", &cancel_message, "groups", None);
            return SyncProgress {
                entity: "groups".to_string(),
                processed: processed_count,
                total: total_groups,
                status: "cancelled".to_string(),
                errors: stats.errors,
            };
        }

        processed_count += 1;

        // Handle removed groups
        if group_item.is_removed {
            info!(group_id = %group_item.group_id, "Group removed from Microsoft - consider deactivating");
            // TODO: Implement group deactivation if desired
            // For now, we skip removed groups
            continue;
        }

        // Get the actual group data
        let ms_group = match &group_item.group {
            Some(g) => g,
            None => {
                warn!(group_id = %group_item.group_id, "Skipping group without data");
                continue;
            }
        };

        let group_name = ms_group.display_name.as_deref().unwrap_or(&ms_group.id);

        update_sync_progress_with_type(
            session_id,
            "groups",
            processed_count - 1,
            total_groups,
            "running",
            &format!("Processing group: {group_name}"),
            "groups",
            None
        );

        // Upsert the group
        match groups_repo::upsert_external_group(
            conn,
            &ms_group.id,
            "microsoft",
            group_name,
            ms_group.description.as_deref(),
            Some(ms_group.get_group_type()),
            ms_group.mail_enabled.unwrap_or(false),
            ms_group.security_enabled.unwrap_or(false),
        ) {
            Ok((group, was_created)) => {
                if was_created {
                    stats.groups_created += 1;
                    debug!(group_name = %group_name, group_type = %ms_group.get_group_type(), "Created new group");
                } else {
                    stats.groups_updated += 1;
                    trace!(group_name = %group_name, "Updated existing group");
                }
                synced_external_ids.push(ms_group.id.clone());

                // Sync membership based on whether we have delta changes or need full fetch
                if !was_full_sync && (!group_item.members_added.is_empty() || !group_item.members_removed.is_empty()) {
                    // Delta sync: apply incremental membership changes
                    match apply_delta_group_membership(conn, group.id, &group_item.members_added, &group_item.members_removed).await {
                        Ok(changes) => {
                            stats.user_membership_changes += changes;
                            if changes > 0 {
                                debug!(group_name = %group_name, changes = changes, "Applied delta membership changes");
                            }
                        }
                        Err(e) => {
                            let error_msg = format!("Failed to apply delta membership for group {group_name}: {e}");
                            warn!("{}", error_msg);
                            stats.errors.push(error_msg);
                        }
                    }
                } else {
                    // Full sync or no delta changes: fetch all members
                    // Sync user group membership
                    match sync_group_membership(conn, &client, &access_token, &ms_group.id, group.id).await {
                        Ok(changes) => {
                            stats.user_membership_changes += changes;
                            if changes > 0 {
                                debug!(group_name = %group_name, changes = changes, "Synced user membership changes");
                            }
                        }
                        Err(e) => {
                            let error_msg = format!("Failed to sync user membership for group {group_name}: {e}");
                            warn!("{}", error_msg);
                            stats.errors.push(error_msg);
                        }
                    }

                    // Sync device group membership
                    match sync_device_group_membership(conn, &client, &access_token, &ms_group.id, group.id).await {
                        Ok(changes) => {
                            stats.device_membership_changes += changes;
                            if changes > 0 {
                                debug!(group_name = %group_name, changes = changes, "Synced device membership changes");
                            }
                        }
                        Err(e) => {
                            let error_msg = format!("Failed to sync device membership for group {group_name}: {e}");
                            warn!("{}", error_msg);
                            stats.errors.push(error_msg);
                        }
                    }
                }
            }
            Err(e) => {
                let error_msg = format!("Failed to upsert group {group_name}: {e}");
                error!(group_name = %group_name, error = %e, "Failed to upsert group");
                stats.errors.push(error_msg);
            }
        }

        // Progress update every 5 groups
        if processed_count % 5 == 0 || processed_count == total_groups {
            update_sync_progress_with_type(
                session_id,
                "groups",
                processed_count,
                total_groups,
                "running",
                &format!(
                    "Processed {}/{} groups ({} created, {} updated, {} errors)",
                    processed_count, total_groups, stats.groups_created, stats.groups_updated, stats.errors.len()
                ),
                "groups",
                None
            );
        }

        // Small delay between groups
        if processed_count < total_groups {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    // Step 3: Mark groups not seen in this sync as potentially stale
    let external_id_refs: Vec<&str> = synced_external_ids.iter().map(|s| s.as_str()).collect();
    if let Err(e) = groups_repo::mark_groups_not_synced(conn, "microsoft", &external_id_refs) {
        warn!(error = %e, "Failed to mark stale groups");
    }

    let processed = stats.groups_created + stats.groups_updated;
    let final_message = format!(
        "Completed: {} created, {} updated, {} user memberships, {} device memberships, {} errors",
        stats.groups_created, stats.groups_updated, stats.user_membership_changes, stats.device_membership_changes, stats.errors.len()
    );

    update_sync_progress_with_type(
        session_id,
        "groups",
        total_groups,
        total_groups,
        "completed",
        &final_message,
        "groups",
        None
    );

    SyncProgress {
        entity: "groups".to_string(),
        processed,
        total: total_groups,
        status: if stats.errors.is_empty() { "completed".to_string() } else { "completed_with_errors".to_string() },
        errors: stats.errors,
    }
}

/// Delta response for a group, including membership changes
#[derive(Debug)]
struct GroupDeltaItem {
    /// The group data (None if this is a removal notification only)
    group: Option<MicrosoftGraphGroup>,
    /// Group ID (always present)
    group_id: String,
    /// Whether this group was removed
    is_removed: bool,
    /// Members added (from members@delta without @removed)
    members_added: Vec<MicrosoftGraphGroupMember>,
    /// Member IDs removed (from members@delta with @removed)
    members_removed: Vec<String>,
}

/// Result of a delta sync fetch operation for groups
#[allow(dead_code)]
struct GroupDeltaFetchResult {
    /// Groups with their membership changes
    groups: Vec<GroupDeltaItem>,
    /// The new delta link to store for next sync (if any)
    new_delta_link: Option<String>,
    /// Whether this was a full sync (no delta token or token expired)
    was_full_sync: bool,
    /// Access token for any additional API calls
    access_token: String,
}

/// Fetch groups from Microsoft Graph API with delta sync support
///
/// Delta queries return only changes since the last sync, including membership changes
/// via the members@delta property. This is much more efficient than fetching all groups
/// and their members on each sync.
#[instrument(level = "info", skip(conn), fields(use_delta = use_delta))]
async fn fetch_microsoft_graph_groups_delta(
    conn: &mut DbConnection,
    use_delta: bool,
) -> Result<GroupDeltaFetchResult, String> {
    let (client, access_token) = get_msgraph_client_and_token().await?;

    // Select fields for groups - include members to track membership changes
    let select_fields = "id,displayName,description,mailEnabled,securityEnabled,groupTypes,mail,members";

    // Check for existing delta token
    let delta_token = if use_delta {
        match crate::repository::sync_history::get_delta_token(conn, "microsoft", "groups") {
            Ok(token) => {
                info!("Found existing delta token for groups, using incremental sync");
                Some(token.delta_link)
            }
            Err(diesel::result::Error::NotFound) => {
                info!("No delta token found for groups, performing initial delta sync");
                None
            }
            Err(e) => {
                warn!(error = %e, "Error retrieving delta token for groups, performing full sync");
                None
            }
        }
    } else {
        info!("Full sync requested for groups, ignoring any existing delta token");
        // Clear existing delta token when doing a full sync
        if let Err(e) = crate::repository::sync_history::delete_delta_token(conn, "microsoft", "groups") {
            if !matches!(e, diesel::result::Error::NotFound) {
                warn!(error = %e, "Failed to clear delta token for groups");
            }
        }
        None
    };

    // Build initial URL
    let mut url = match &delta_token {
        Some(link) => {
            // Use the stored delta link for incremental sync
            link.clone()
        }
        None => {
            // Start fresh delta sync
            format!(
                "https://graph.microsoft.com/v1.0/groups/delta?$select={}",
                urlencoding::encode(select_fields)
            )
        }
    };

    let mut was_full_sync = delta_token.is_none();

    debug!(url = %url, was_full_sync = was_full_sync, "Microsoft Graph group delta query URL");

    let mut all_groups: Vec<GroupDeltaItem> = Vec::new();
    let mut page_count = 0;
    let mut new_delta_link = None;

    loop {
        page_count += 1;
        debug!("Fetching group delta page {} from Microsoft Graph", page_count);

        let graph_response = client
            .get(&url)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| format!("Failed to send Microsoft Graph group delta request (page {page_count}): {e}"))?;

        let status = graph_response.status();

        // Handle 410 Gone - delta token expired, need to do full sync
        if status == reqwest::StatusCode::GONE {
            warn!("Group delta token expired (410 Gone), falling back to full sync");

            // Clear the expired token
            let _ = crate::repository::sync_history::delete_delta_token(conn, "microsoft", "groups");

            // Reset to full sync - rebuild the initial URL without delta token
            url = format!(
                "https://graph.microsoft.com/v1.0/groups/delta?$select={}",
                urlencoding::encode(select_fields)
            );
            was_full_sync = true;
            all_groups.clear();
            page_count = 0;
            continue;
        }

        let response_data: serde_json::Value = graph_response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Microsoft Graph group delta response (page {page_count}): {e}"))?;

        if !status.is_success() {
            let error_msg = response_data
                .get("error")
                .and_then(|err| err.get("message"))
                .and_then(|msg| msg.as_str())
                .unwrap_or("Unknown Microsoft Graph error");
            return Err(format!("Microsoft Graph API error (page {page_count}, {status}): {error_msg}"));
        }

        // Parse groups from this page
        let groups_array = response_data
            .get("value")
            .and_then(|v| v.as_array())
            .ok_or_else(|| format!("Microsoft Graph group delta response missing 'value' array (page {page_count})"))?;

        for group_value in groups_array {
            let group_id = group_value
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if group_id.is_empty() {
                continue;
            }

            // Check if this is a removed group
            let is_removed = group_value.get("@removed").is_some();

            // Parse membership changes from members@delta
            let mut members_added = Vec::new();
            let mut members_removed = Vec::new();

            if let Some(members_delta) = group_value.get("members@delta").and_then(|v| v.as_array()) {
                for member_value in members_delta {
                    // Check if this member was removed
                    if member_value.get("@removed").is_some() {
                        if let Some(member_id) = member_value.get("id").and_then(|v| v.as_str()) {
                            members_removed.push(member_id.to_string());
                        }
                    } else {
                        // Parse the member
                        if let Ok(member) = serde_json::from_value::<MicrosoftGraphGroupMember>(member_value.clone()) {
                            members_added.push(member);
                        }
                    }
                }
            }

            // Parse the group data (may be None for removal-only notifications)
            let group = if is_removed {
                None
            } else {
                match serde_json::from_value::<MicrosoftGraphGroup>(group_value.clone()) {
                    Ok(g) => Some(g),
                    Err(e) => {
                        warn!(page = page_count, error = %e, group_id = %group_id, "Failed to parse group from delta response");
                        None
                    }
                }
            };

            all_groups.push(GroupDeltaItem {
                group,
                group_id,
                is_removed,
                members_added,
                members_removed,
            });
        }

        debug!(
            "Group delta page {}: {} groups processed",
            page_count,
            all_groups.len()
        );

        // Check for deltaLink (end of changes) or nextLink (more pages)
        if let Some(delta_link) = response_data.get("@odata.deltaLink").and_then(|v| v.as_str()) {
            new_delta_link = Some(delta_link.to_string());
            debug!("Received group deltaLink, finished fetching changes");
            break;
        } else if let Some(next_link) = response_data.get("@odata.nextLink").and_then(|v| v.as_str()) {
            url = next_link.to_string();
            trace!("Found nextLink, continuing to page {}...", page_count + 1);
        } else {
            warn!("No deltaLink or nextLink found in group delta response");
            break;
        }

        // Small delay between requests
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Store the new delta link for next sync
    if let Some(ref delta_link) = new_delta_link {
        match crate::repository::sync_history::upsert_delta_token(conn, "microsoft", "groups", delta_link) {
            Ok(_) => info!("Saved delta token for groups"),
            Err(e) => warn!(error = %e, "Failed to save delta token for groups"),
        }
    }

    let removed_count = all_groups.iter().filter(|g| g.is_removed).count();
    let updated_count = all_groups.len() - removed_count;

    info!(
        groups_updated = updated_count,
        groups_removed = removed_count,
        pages = page_count,
        was_full_sync = was_full_sync,
        "Group delta fetch completed"
    );

    Ok(GroupDeltaFetchResult {
        groups: all_groups,
        new_delta_link,
        was_full_sync,
        access_token: access_token.to_string(),
    })
}

/// Fetch members of a specific group
async fn fetch_group_members(
    client: &reqwest::Client,
    access_token: &str,
    group_id: &str,
) -> Result<Vec<MicrosoftGraphGroupMember>, String> {
    let mut all_members: Vec<MicrosoftGraphGroupMember> = Vec::new();
    let mut next_link: Option<String> = Some(format!(
        "https://graph.microsoft.com/v1.0/groups/{group_id}/members?$select=id,displayName,userPrincipalName&$top=999"
    ));

    while let Some(url) = next_link {
        let response = client
            .get(&url)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch group members: {e}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(format!("Failed to fetch members ({status}): {error_text}"));
        }

        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse members response: {e}"))?;

        if let Some(members) = data["value"].as_array() {
            for member_value in members {
                if let Ok(member) = serde_json::from_value::<MicrosoftGraphGroupMember>(member_value.clone()) {
                    // Include user and device members
                    if member.is_user() || member.is_device() {
                        all_members.push(member);
                    }
                }
            }
        }

        next_link = data["@odata.nextLink"]
            .as_str()
            .map(String::from);
    }

    Ok(all_members)
}

/// Sync group membership (adds/removes local users from local group)
async fn sync_group_membership(
    conn: &mut DbConnection,
    client: &reqwest::Client,
    access_token: &str,
    graph_group_id: &str,
    local_group_id: i32,
) -> Result<usize, String> {
    use std::collections::HashSet;

    // Fetch current members from Microsoft Graph
    let graph_members = fetch_group_members(client, access_token, graph_group_id).await?;

    // Filter to only user members (not devices or nested groups)
    let user_members: Vec<_> = graph_members.iter().filter(|m| m.is_user()).collect();

    // Get current local membership
    let current_local_members = groups_repo::get_member_uuids_for_group(conn, local_group_id)
        .map_err(|e| format!("Failed to get local members: {e}"))?;
    let current_local_set: HashSet<_> = current_local_members.into_iter().collect();

    // Map Graph user IDs to local user UUIDs
    let graph_member_ids: Vec<&str> = user_members.iter().map(|m| m.id.as_str()).collect();
    let user_mappings = identity_repo::get_user_uuids_by_external_ids(&graph_member_ids, "microsoft", conn)
        .map_err(|e| format!("Failed to lookup users: {e}"))?;

    let new_member_uuids: HashSet<_> = user_mappings.into_iter().map(|(_, uuid)| uuid).collect();

    // Calculate additions and removals
    let to_add: Vec<_> = new_member_uuids.difference(&current_local_set).cloned().collect();
    let to_remove: Vec<_> = current_local_set.difference(&new_member_uuids).cloned().collect();

    let mut changes = 0;

    // Add new members
    for user_uuid in &to_add {
        if let Err(e) = groups_repo::add_user_to_group(conn, *user_uuid, local_group_id, None) {
            warn!(user_uuid = %user_uuid, group_id = local_group_id, error = %e, "Failed to add user to group");
        } else {
            changes += 1;
        }
    }

    // Remove old members (only for users synced from Microsoft - preserve manually added users)
    for user_uuid in &to_remove {
        // Check if this user has a Microsoft identity - only remove if they were synced from Microsoft
        let has_microsoft_identity = identity_repo::get_user_identities(user_uuid, conn)
            .map(|identities| identities.iter().any(|i| i.provider_type == "microsoft"))
            .unwrap_or(false);

        if has_microsoft_identity {
            if let Err(e) = groups_repo::remove_user_from_group(conn, user_uuid, local_group_id) {
                warn!(user_uuid = %user_uuid, group_id = local_group_id, error = %e, "Failed to remove user from group");
            } else {
                changes += 1;
            }
        } else {
            debug!(user_uuid = %user_uuid, group_id = local_group_id, "Preserving manually added user in group");
        }
    }

    Ok(changes)
}

/// Apply delta membership changes to a group (from Microsoft Graph delta query)
///
/// This function processes incremental membership changes instead of fetching
/// all members. It's much more efficient for subsequent syncs.
async fn apply_delta_group_membership(
    conn: &mut DbConnection,
    local_group_id: i32,
    members_added: &[MicrosoftGraphGroupMember],
    members_removed: &[String],
) -> Result<usize, String> {
    let mut changes = 0;

    // Process added members (users only for now)
    let user_members_added: Vec<_> = members_added.iter().filter(|m| m.is_user()).collect();
    if !user_members_added.is_empty() {
        let external_ids: Vec<&str> = user_members_added.iter().map(|m| m.id.as_str()).collect();
        let user_mappings = identity_repo::get_user_uuids_by_external_ids(&external_ids, "microsoft", conn)
            .map_err(|e| format!("Failed to lookup users for adding: {e}"))?;

        for (external_id, user_uuid) in user_mappings {
            // Check if user is already a member
            let current_members = groups_repo::get_member_uuids_for_group(conn, local_group_id)
                .unwrap_or_default();

            if !current_members.contains(&user_uuid) {
                if let Err(e) = groups_repo::add_user_to_group(conn, user_uuid, local_group_id, None) {
                    warn!(user_uuid = %user_uuid, group_id = local_group_id, external_id = %external_id, error = %e, "Failed to add user to group from delta");
                } else {
                    debug!(user_uuid = %user_uuid, group_id = local_group_id, "Added user to group from delta");
                    changes += 1;
                }
            }
        }
    }

    // Process removed members
    if !members_removed.is_empty() {
        let external_ids_refs: Vec<&str> = members_removed.iter().map(|s| s.as_str()).collect();
        let user_mappings = identity_repo::get_user_uuids_by_external_ids(&external_ids_refs, "microsoft", conn)
            .map_err(|e| format!("Failed to lookup users for removal: {e}"))?;

        for (external_id, user_uuid) in user_mappings {
            if let Err(e) = groups_repo::remove_user_from_group(conn, &user_uuid, local_group_id) {
                warn!(user_uuid = %user_uuid, group_id = local_group_id, external_id = %external_id, error = %e, "Failed to remove user from group from delta");
            } else {
                debug!(user_uuid = %user_uuid, group_id = local_group_id, "Removed user from group from delta");
                changes += 1;
            }
        }
    }

    // Process added device members
    let device_members_added: Vec<_> = members_added.iter().filter(|m| m.is_device()).collect();
    if !device_members_added.is_empty() {
        for device_member in device_members_added {
            // Try to find the device by Entra Object ID
            if let Ok(device) = device_repo::get_device_by_entra_id(conn, &device_member.id) {
                // Check if device is already a member
                let current_devices = groups_repo::get_device_ids_for_group(conn, local_group_id)
                    .unwrap_or_default();

                if !current_devices.contains(&device.id) {
                    if let Err(e) = groups_repo::add_device_to_group(conn, device.id, local_group_id, None, Some("microsoft")) {
                        warn!(device_id = device.id, group_id = local_group_id, error = %e, "Failed to add device to group from delta");
                    } else {
                        debug!(device_id = device.id, group_id = local_group_id, "Added device to group from delta");
                        changes += 1;
                    }
                }
            }
        }
    }

    // Process removed device members
    for removed_id in members_removed {
        // Try to find the device by Entra Object ID
        if let Ok(device) = device_repo::get_device_by_entra_id(conn, removed_id) {
            if let Err(e) = groups_repo::remove_device_from_group(conn, device.id, local_group_id) {
                warn!(device_id = device.id, group_id = local_group_id, error = %e, "Failed to remove device from group from delta");
            } else {
                debug!(device_id = device.id, group_id = local_group_id, "Removed device from group from delta");
                changes += 1;
            }
        }
    }

    Ok(changes)
}

/// Sync device group membership (adds/removes local devices from local group)
async fn sync_device_group_membership(
    conn: &mut DbConnection,
    client: &reqwest::Client,
    access_token: &str,
    graph_group_id: &str,
    local_group_id: i32,
) -> Result<usize, String> {
    use std::collections::HashSet;

    // Fetch current members from Microsoft Graph
    let graph_members = fetch_group_members(client, access_token, graph_group_id).await?;

    debug!(
        group_id = graph_group_id,
        total_members = graph_members.len(),
        "Fetched group members for device sync"
    );

    // Filter to only device members
    let device_members: Vec<_> = graph_members.iter().filter(|m| m.is_device()).collect();

    debug!(
        group_id = graph_group_id,
        device_count = device_members.len(),
        "Filtered to device members"
    );

    if device_members.is_empty() {
        debug!(group_id = graph_group_id, "No device members in group");
        return Ok(0);
    }

    // Get current local device membership (only synced ones from Microsoft)
    let current_synced_devices = groups_repo::get_synced_device_ids_for_group(conn, local_group_id, "microsoft")
        .map_err(|e| format!("Failed to get local device members: {e}"))?;
    let current_local_set: HashSet<_> = current_synced_devices.into_iter().collect();

    // Map Graph device IDs (Entra Object IDs) to local device IDs
    let graph_device_ids: Vec<&str> = device_members.iter().map(|m| m.id.as_str()).collect();

    debug!(
        group_id = graph_group_id,
        graph_device_ids = ?graph_device_ids,
        "Looking up local devices by Entra IDs"
    );

    let device_mappings = device_repo::get_devices_by_entra_ids(conn, &graph_device_ids)
        .map_err(|e| format!("Failed to lookup devices: {e}"))?;

    debug!(
        group_id = graph_group_id,
        mapped_count = device_mappings.len(),
        "Mapped Graph device IDs to local device IDs"
    );

    let new_device_ids: HashSet<_> = device_mappings.into_iter().map(|(_, id)| id).collect();

    // Calculate additions and removals
    let to_add: Vec<_> = new_device_ids.difference(&current_local_set).cloned().collect();
    let to_remove: Vec<_> = current_local_set.difference(&new_device_ids).cloned().collect();

    let mut changes = 0;

    // Add new device members
    for device_id in &to_add {
        if let Err(e) = groups_repo::add_device_to_group(conn, *device_id, local_group_id, None, Some("microsoft")) {
            warn!(device_id = %device_id, group_id = local_group_id, error = %e, "Failed to add device to group");
        } else {
            changes += 1;
        }
    }

    // Remove old device members (only those that were synced from Microsoft)
    // This preserves manually-added device group memberships
    for device_id in &to_remove {
        if let Err(e) = groups_repo::remove_device_from_group(conn, *device_id, local_group_id) {
            warn!(device_id = %device_id, group_id = local_group_id, error = %e, "Failed to remove device from group");
        } else {
            changes += 1;
        }
    }

    Ok(changes)
}

/// Result struct for photo sync containing both avatar sizes
#[derive(Debug)]
pub struct PhotoSyncUrls {
    pub avatar_url: Option<String>,      // 120x120 or fallback
    pub avatar_thumb: Option<String>,    // 48x48 thumbnail
}

/// Fetch and save user profile photo from Microsoft Graph (updated to download 120x120 and generate thumbnail)
async fn sync_user_profile_photo(
    client: &reqwest::Client,
    access_token: &str,
    user: &MicrosoftGraphUser,
    local_user_uuid: &str,
) -> Result<PhotoSyncUrls, String> {
    debug!("Fetching profile photo for user: {}", user.user_principal_name);

    let mut avatar_url = None;
    let mut avatar_thumb = None;
    
    // Download 120x120 for profile views (main avatar) and generate thumbnail from it
    match download_profile_photo_size(client, access_token, user, local_user_uuid, "120x120").await {
        Ok(Some(url)) => {
            debug!("Successfully downloaded 120x120 photo for user: {}", user.user_principal_name);
            avatar_url = Some(url.clone());
            
            // Generate 48x48 WebP thumbnail from the 120x120 image
            match crate::utils::generate_user_avatar_thumbnail(&url, local_user_uuid).await {
                Ok(Some(thumb_url)) => {
                    debug!("Successfully generated thumbnail for user: {}", user.user_principal_name);
                    avatar_thumb = Some(thumb_url);
                },
                Ok(None) => debug!("Failed to generate thumbnail for user: {}", user.user_principal_name),
                Err(e) => warn!("Error generating thumbnail for user {}: {}", user.user_principal_name, e),
            }
        },
        Ok(None) => trace!("No 120x120 photo available for user: {}", user.user_principal_name),
        Err(e) => debug!("Failed to download 120x120 photo for user {}: {}", user.user_principal_name, e),
    }
    
    // If no 120x120 photo was available, try the default size as fallback
    if avatar_url.is_none() {
        debug!(user_principal_name = %user.user_principal_name, "No 120x120 photo available, trying default size");
        match sync_user_profile_photo_fallback(client, access_token, user, local_user_uuid).await {
            Ok(Some(url)) => {
                debug!(user_principal_name = %user.user_principal_name, "Successfully downloaded default photo");
                avatar_url = Some(url.clone());

                // Generate thumbnail from the default size image
                match crate::utils::generate_user_avatar_thumbnail(&url, local_user_uuid).await {
                    Ok(Some(thumb_url)) => {
                        debug!(user_principal_name = %user.user_principal_name, "Successfully generated thumbnail from default photo");
                        avatar_thumb = Some(thumb_url);
                    },
                    Ok(None) => debug!(user_principal_name = %user.user_principal_name, "Failed to generate thumbnail from default photo"),
                    Err(e) => warn!(user_principal_name = %user.user_principal_name, error = %e, "Error generating thumbnail from default photo"),
                }
            },
            Ok(None) => debug!(user_principal_name = %user.user_principal_name, "No default photo available"),
            Err(e) => warn!(user_principal_name = %user.user_principal_name, error = %e, "Failed to download default photo"),
        }
    }
    
    Ok(PhotoSyncUrls {
        avatar_url,
        avatar_thumb,
    })
}

/// Download a specific size of profile photo
async fn download_profile_photo_size(
    client: &reqwest::Client,
    access_token: &str,
    user: &MicrosoftGraphUser,
    local_user_uuid: &str,
    size: &str,
) -> Result<Option<String>, String> {
    trace!(size = size, user_principal_name = %user.user_principal_name, "Fetching profile photo");

    let photo_url = format!("https://graph.microsoft.com/v1.0/users/{}/photos/{}/$value", user.id, size);

    let photo_response = client
        .get(&photo_url)
        .header("Authorization", format!("Bearer {access_token}"))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch {size} profile photo: {e}"))?;

    let status = photo_response.status();
    trace!(size = size, user_principal_name = %user.user_principal_name, status = %status, "Photo request status");

    if !status.is_success() {
        if status.as_u16() == 404 {
            trace!(size = size, user_principal_name = %user.user_principal_name, "Profile photo not found");
            return Ok(None);
        } else if status.as_u16() == 400 {
            debug!(size = size, user_principal_name = %user.user_principal_name, "Profile photo request returned 400 Bad Request");
            return Ok(None);
        } else if status.as_u16() == 403 {
            warn!(size = size, user_principal_name = %user.user_principal_name, "Access denied to profile photo - insufficient permissions");
            return Ok(None);
        } else {
            return Err(format!("Failed to fetch {size} profile photo, status: {status}"));
        }
    }

    // Get the photo data
    let photo_bytes = photo_response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read {size} photo data: {e}"))?;

    if photo_bytes.is_empty() {
        debug!(size = size, user_principal_name = %user.user_principal_name, "Empty profile photo");
        return Ok(None);
    }

    debug!(size = size, user_principal_name = %user.user_principal_name, bytes = photo_bytes.len(), "Successfully downloaded profile photo");

    // Save the photo to the filesystem
    save_profile_photo_to_disk(&photo_bytes, local_user_uuid, size).await
}

/// Save profile photo bytes to disk and return the URL
async fn save_profile_photo_to_disk(
    photo_bytes: &[u8],
    local_user_uuid: &str,
    size_label: &str,
) -> Result<Option<String>, String> {
    trace!(user_uuid = local_user_uuid, size = size_label, "Processing Microsoft Graph profile photo");

    // Use the shared image processing function to convert to WebP with size constraints
    let max_size = match size_label {
        "120x120" => 120,
        "default" => 200, // Default gets processed to 200px max
        _ => 200, // Fallback to 200px
    };

    match crate::utils::image::process_avatar_image(photo_bytes, local_user_uuid, max_size).await {
        Ok(Some(avatar_url)) => {
            debug!(user_uuid = local_user_uuid, avatar_url = %avatar_url, "Successfully processed Microsoft Graph photo");
            Ok(Some(avatar_url))
        },
        Ok(None) => {
            debug!(user_uuid = local_user_uuid, "Failed to process Microsoft Graph photo");
            Ok(None)
        },
        Err(e) => {
            error!(user_uuid = local_user_uuid, error = %e, "Error processing Microsoft Graph photo");
            Err(e)
        }
    }
}



/// Update user avatar URLs in the database
async fn update_user_avatar_by_id(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    avatar_url: Option<String>,
    avatar_thumb: Option<String>,
) -> Result<(), String> {
    trace!(user_uuid = %user_uuid, avatar_url = ?avatar_url, avatar_thumb = ?avatar_thumb, "update_user_avatar_by_id called");

    if avatar_url.is_some() || avatar_thumb.is_some() {
        trace!(user_uuid = %user_uuid, avatar_url = ?avatar_url, avatar_thumb = ?avatar_thumb, "Updating avatar URLs");

        let user_update = crate::models::UserUpdate {
            name: None,
            role: None,
            pronouns: None,
            avatar_url: avatar_url.clone(),
            banner_url: None,
            avatar_thumb: avatar_thumb.clone(),
            theme: None, // Preserve user's theme preference
            microsoft_uuid: None, // Don't update Microsoft UUID when updating avatar
            updated_at: Some(chrono::Utc::now().naive_utc()),
        };

        match user_repo::update_user(user_uuid, user_update, conn) {
            Ok(updated_user) => {
                debug!(user_uuid = %user_uuid, avatar_url = ?updated_user.avatar_url, avatar_thumb = ?updated_user.avatar_thumb, "Successfully updated avatar URLs");
            },
            Err(e) => {
                let error_msg = format!("Failed to update user avatar: {e}");
                error!(user_uuid = %user_uuid, error = %e, "Failed to update user avatar");
                return Err(error_msg);
            }
        }
    } else {
        trace!(user_uuid = %user_uuid, "No avatar URLs provided, skipping update");
    }

    Ok(())
}

/// Fallback function to get profile photo in default size if 120x120 is not available
async fn sync_user_profile_photo_fallback(
    client: &reqwest::Client,
    access_token: &str,
    user: &MicrosoftGraphUser,
    local_user_uuid: &str,
) -> Result<Option<String>, String> {
    trace!(user_principal_name = %user.user_principal_name, "Fetching default size profile photo");

    // Try to get the user's profile photo in default size
    let photo_url = format!("https://graph.microsoft.com/v1.0/users/{}/photo/$value", user.id);

    let photo_response = client
        .get(&photo_url)
        .header("Authorization", format!("Bearer {access_token}"))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch default profile photo: {e}"))?;

    let status = photo_response.status();
    trace!(user_principal_name = %user.user_principal_name, status = %status, "Default photo request status");

    if !status.is_success() {
        // User likely doesn't have a profile photo
        if status.as_u16() == 404 {
            trace!(user_principal_name = %user.user_principal_name, "No profile photo found");
            return Ok(None);
        } else if status.as_u16() == 400 {
            debug!(user_principal_name = %user.user_principal_name, "Profile photo request returned 400 Bad Request - user may not have a photo");
            return Ok(None);
        } else if status.as_u16() == 403 {
            warn!(user_principal_name = %user.user_principal_name, "Access denied to profile photo - insufficient permissions");
            return Ok(None);
        } else {
            debug!(user_principal_name = %user.user_principal_name, status = %status, "Failed to fetch profile photo - skipping");
            return Ok(None);
        }
    }

    // Get the photo data
    let photo_bytes = photo_response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read photo data: {e}"))?;

    if photo_bytes.is_empty() {
        debug!(user_principal_name = %user.user_principal_name, "Empty profile photo");
        return Ok(None);
    }

    debug!(user_principal_name = %user.user_principal_name, bytes = photo_bytes.len(), "Successfully downloaded default profile photo");

    // Save the photo to the filesystem
    save_profile_photo_to_disk(&photo_bytes, local_user_uuid, "default").await
}

/// Result of a delta sync fetch operation for Entra ID devices
#[allow(dead_code)]
struct DeviceDeltaFetchResult {
    /// Entra ID devices to create or update
    devices: Vec<EntraDevice>,
    /// IDs of devices that were removed (from @removed marker in delta response)
    removed_device_ids: Vec<String>,
    /// The new delta link to store for next sync (if any)
    new_delta_link: Option<String>,
    /// Whether this was a full sync (no delta token or token expired)
    was_full_sync: bool,
    /// Access token for any additional API calls
    access_token: String,
}

/// Fetch Entra ID devices from Microsoft Graph API with delta sync support
///
/// Delta queries return only changes since the last sync, making subsequent syncs much faster.
/// Uses the /devices/delta endpoint for Entra ID device identity.
/// Note: This is separate from Intune managed devices - Entra ID handles device identity,
/// while Intune handles device configuration and compliance.
#[instrument(level = "info", skip(conn), fields(use_delta = use_delta))]
async fn fetch_microsoft_graph_devices_delta(
    conn: &mut DbConnection,
    use_delta: bool,
) -> Result<DeviceDeltaFetchResult, String> {
    let (client, access_token) = get_msgraph_client_and_token().await?;

    // Select fields for EntraDevice struct
    let select_fields = "id,deviceId,displayName,operatingSystem,operatingSystemVersion,trustType,isManaged,isCompliant,accountEnabled,approximateLastSignInDateTime,manufacturer,model,profileType,registrationDateTime";

    // Check for existing delta token
    let delta_token = if use_delta {
        match crate::repository::sync_history::get_delta_token(conn, "microsoft", "devices") {
            Ok(token) => {
                info!("Found existing delta token for devices, using incremental sync");
                Some(token.delta_link)
            }
            Err(diesel::result::Error::NotFound) => {
                info!("No delta token found for devices, performing initial delta sync");
                None
            }
            Err(e) => {
                warn!(error = %e, "Error retrieving delta token for devices, performing full sync");
                None
            }
        }
    } else {
        info!("Full sync requested for devices, ignoring any existing delta token");
        // Clear existing delta token when doing a full sync
        if let Err(e) = crate::repository::sync_history::delete_delta_token(conn, "microsoft", "devices") {
            if !matches!(e, diesel::result::Error::NotFound) {
                warn!(error = %e, "Failed to clear delta token for devices");
            }
        }
        None
    };

    // Build initial URL - use Entra ID /devices/delta endpoint
    let mut url = match &delta_token {
        Some(link) => {
            // Use the stored delta link for incremental sync
            link.clone()
        }
        None => {
            // Start fresh delta sync with Entra ID devices
            format!(
                "https://graph.microsoft.com/v1.0/devices/delta?$select={}",
                urlencoding::encode(select_fields)
            )
        }
    };

    let mut was_full_sync = delta_token.is_none();

    debug!(url = %url, was_full_sync = was_full_sync, "Microsoft Graph device delta query URL");

    let mut all_devices = Vec::new();
    let mut removed_device_ids = Vec::new();
    let mut page_count = 0;
    let mut new_delta_link = None;

    loop {
        page_count += 1;
        debug!("Fetching device delta page {} from Microsoft Graph", page_count);

        let graph_response = client
            .get(&url)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| format!("Failed to send Microsoft Graph device delta request (page {page_count}): {e}"))?;

        let status = graph_response.status();

        // Handle 410 Gone - delta token expired, need to do full sync
        if status == reqwest::StatusCode::GONE {
            warn!("Device delta token expired (410 Gone), falling back to full sync");

            // Clear the expired token
            let _ = crate::repository::sync_history::delete_delta_token(conn, "microsoft", "devices");

            // Reset to full sync - rebuild the initial URL without delta token
            url = format!(
                "https://graph.microsoft.com/v1.0/devices/delta?$select={}",
                urlencoding::encode(select_fields)
            );
            was_full_sync = true;
            all_devices.clear();
            removed_device_ids.clear();
            page_count = 0;
            continue;
        }

        let response_data: serde_json::Value = graph_response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Microsoft Graph device delta response (page {page_count}): {e}"))?;

        if !status.is_success() {
            let error_msg = response_data
                .get("error")
                .and_then(|err| err.get("message"))
                .and_then(|msg| msg.as_str())
                .unwrap_or("Unknown Microsoft Graph error");
            return Err(format!("Microsoft Graph API error (page {page_count}, {status}): {error_msg}"));
        }

        // Parse devices from this page
        let devices_array = response_data
            .get("value")
            .and_then(|v| v.as_array())
            .ok_or_else(|| format!("Microsoft Graph device delta response missing 'value' array (page {page_count})"))?;

        for device_value in devices_array {
            // Check if this is a removed device
            if device_value.get("@removed").is_some() {
                if let Some(id) = device_value.get("id").and_then(|v| v.as_str()) {
                    debug!(device_id = %id, "Device marked as removed in delta response");
                    removed_device_ids.push(id.to_string());
                }
                continue;
            }

            // Parse Entra ID device
            match serde_json::from_value::<EntraDevice>(device_value.clone()) {
                Ok(device) => all_devices.push(device),
                Err(e) => {
                    warn!(page = page_count, error = %e, data = %device_value, "Failed to parse Entra device from delta response");
                }
            }
        }

        debug!("Entra device delta page {}: {} devices, {} removed", page_count, all_devices.len(), removed_device_ids.len());

        // Check for deltaLink (end of changes) or nextLink (more pages)
        if let Some(delta_link) = response_data.get("@odata.deltaLink").and_then(|v| v.as_str()) {
            new_delta_link = Some(delta_link.to_string());
            debug!("Received device deltaLink, finished fetching changes");
            break;
        } else if let Some(next_link) = response_data.get("@odata.nextLink").and_then(|v| v.as_str()) {
            url = next_link.to_string();
            trace!("Found nextLink, continuing to page {}...", page_count + 1);
        } else {
            warn!("No deltaLink or nextLink found in device delta response");
            break;
        }

        // Small delay between requests
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Store the new delta link for next sync
    if let Some(ref delta_link) = new_delta_link {
        match crate::repository::sync_history::upsert_delta_token(conn, "microsoft", "devices", delta_link) {
            Ok(_) => info!("Saved delta token for devices"),
            Err(e) => warn!(error = %e, "Failed to save delta token for devices"),
        }
    }

    info!(
        devices = all_devices.len(),
        removed = removed_device_ids.len(),
        pages = page_count,
        was_full_sync = was_full_sync,
        "Device delta fetch completed"
    );

    Ok(DeviceDeltaFetchResult {
        devices: all_devices,
        removed_device_ids,
        new_delta_link,
        was_full_sync,
        access_token: access_token.to_string(),
    })
}


/// Process a single Entra ID device (from /devices endpoint)
/// This handles device identity from Entra ID using delta sync.
/// Entra ID devices provide identity info; Intune provides management/compliance data.
async fn process_entra_device(
    conn: &mut DbConnection,
    _provider_id: i32,
    entra_device: &EntraDevice,
    stats: &mut DeviceSyncStats,
) -> Result<(), String> {
    let device_name = entra_device.display_name.as_deref().unwrap_or(&entra_device.id);
    debug!(device_name = %device_name, entra_id = %entra_device.id, "Processing Entra device");

    // Step 1: Check if this device already exists by Entra Object ID
    // The `id` field from /devices IS the Entra Object ID
    let existing_device = device_repo::get_device_by_entra_id(conn, &entra_device.id).ok();

    // Step 2: If not found by Entra ID, try by Microsoft Device ID (the deviceId field)
    let existing_device = if existing_device.is_none() {
        if let Some(microsoft_device_id) = &entra_device.device_id {
            device_repo::get_device_by_microsoft_id(conn, microsoft_device_id).ok()
        } else {
            None
        }
    } else {
        existing_device
    };

    // Step 3: Prepare device data
    let device_display_name = entra_device.display_name
        .as_ref()
        .cloned()
        .unwrap_or_else(|| format!("Device-{}", entra_device.id));

    let hostname = device_display_name.clone();

    let model = entra_device.model
        .as_ref()
        .cloned()
        .unwrap_or_else(|| "Unknown Model".to_string());

    let manufacturer = entra_device.manufacturer
        .as_ref()
        .cloned()
        .unwrap_or_else(|| "Unknown Manufacturer".to_string());

    // Map compliance state from isCompliant boolean
    let compliance_state = match entra_device.is_compliant {
        Some(true) => Some("compliant".to_string()),
        Some(false) => Some("noncompliant".to_string()),
        None => None,
    };

    // Parse registration date time
    let registration_date = parse_microsoft_datetime(&entra_device.registration_date_time);

    // Parse last sign in time for last_sync_time
    let last_sign_in = parse_microsoft_datetime(&entra_device.approximate_last_sign_in_date_time);

    if let Some(existing) = existing_device {
        // Update existing device
        let device_update = crate::models::DeviceUpdate {
            name: Some(device_display_name.clone()),
            hostname: Some(hostname),
            serial_number: None, // Entra doesn't provide serial number
            model: Some(model),
            warranty_status: None, // Keep existing
            manufacturer: Some(manufacturer),
            primary_user_uuid: None, // Entra /devices doesn't provide user info; keep existing
            intune_device_id: None, // Keep existing if set by Intune sync
            entra_device_id: Some(entra_device.id.clone()), // The Object ID
            device_type: None, // Keep existing device type
            location: None, // Keep existing location
            notes: None, // Keep existing notes
            microsoft_device_id: entra_device.device_id.clone(), // The deviceId field
            compliance_state,
            last_sync_time: last_sign_in,
            operating_system: entra_device.operating_system.clone(),
            os_version: entra_device.operating_system_version.clone(),
            is_managed: entra_device.is_managed,
            enrollment_date: registration_date,
            updated_at: Some(chrono::Utc::now().naive_utc()),
        };

        device_repo::update_device(conn, existing.id, device_update)
            .map_err(|e| format!("Failed to update device: {e}"))?;

        debug!(device_name = %device_display_name, "Updated existing Entra device");
        stats.existing_devices_updated += 1;
    } else {
        // Create new device
        let new_device = crate::models::NewDevice {
            name: device_display_name.clone(),
            hostname: Some(hostname),
            serial_number: None, // Entra doesn't provide serial number
            model: Some(model),
            warranty_status: Some("Unknown".to_string()),
            manufacturer: Some(manufacturer),
            primary_user_uuid: None, // Entra /devices doesn't provide user info
            intune_device_id: None, // Not from Intune
            entra_device_id: Some(entra_device.id.clone()), // The Object ID
            device_type: Some("Computer".to_string()), // Default type
            location: None,
            notes: None,
            microsoft_device_id: entra_device.device_id.clone(), // The deviceId field
            compliance_state,
            last_sync_time: last_sign_in,
            operating_system: entra_device.operating_system.clone(),
            os_version: entra_device.operating_system_version.clone(),
            is_managed: entra_device.is_managed,
            enrollment_date: registration_date,
        };

        device_repo::create_device(conn, new_device)
            .map_err(|e| format!("Failed to create device: {e}"))?;

        info!(device_name = %device_display_name, "Created new Entra device");
        stats.new_devices_created += 1;
    }

    Ok(())
}

/// Get Entra Object ID from Azure AD Device ID
pub async fn get_entra_object_id(
    req: actix_web::HttpRequest,
    db_pool: web::Data<Pool>,
    path: web::Path<String>,
) -> impl Responder {
    let _conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Database connection failed"
        })),
    };
    // Extract claims from cookie auth middleware
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Get Microsoft provider
    let provider = match get_default_microsoft_provider() {
        Ok(provider) => provider,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Microsoft auth provider not found"
        })),
    };

    let azure_ad_device_id = path.into_inner();
    
    // Fetch the Object ID from Microsoft Graph
    match fetch_entra_object_id_from_graph(provider.id, &azure_ad_device_id).await {
        Ok(object_id) => HttpResponse::Ok().json(json!({
            "success": true,
            "azure_ad_device_id": azure_ad_device_id,
            "object_id": object_id,
            "entra_url": format!("https://entra.microsoft.com/#view/Microsoft_AAD_Devices/DeviceDetailsMenuBlade/~/Properties/objectId/{}", object_id)
        })),
        Err(error) => HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": format!("Failed to fetch Object ID: {}", error)
        }))
    }
}

/// Fetch Entra Object ID from Microsoft Graph using Azure AD Device ID
async fn fetch_entra_object_id_from_graph(_provider_id: i32, azure_ad_device_id: &str) -> Result<String, String> {
    let (client, access_token) = get_msgraph_client_and_token().await?;

    // Query Microsoft Graph for the device using the Azure AD Device ID
    // Filter by deviceId (Azure AD Device ID) to get the Object ID (id field)
    let url = format!(
        "https://graph.microsoft.com/v1.0/devices?$filter=deviceId eq '{azure_ad_device_id}'&$select=id,deviceId"
    );

    debug!(azure_ad_device_id = %azure_ad_device_id, "Fetching Entra Object ID for Azure AD Device ID");

    let graph_response = client
        .get(&url)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| format!("Failed to send Microsoft Graph request: {e}"))?;

    let status = graph_response.status();
    let response_data: serde_json::Value = graph_response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Microsoft Graph response: {e}"))?;

    if !status.is_success() {
        let error_msg = response_data
            .get("error")
            .and_then(|err| err.get("message"))
            .and_then(|msg| msg.as_str())
            .unwrap_or("Unknown Microsoft Graph error");
        return Err(format!("Microsoft Graph API error ({status}): {error_msg}"));
    }

    // Parse the response to get the Object ID
    let devices_array = response_data
        .get("value")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Microsoft Graph response missing 'value' array".to_string())?;

    if devices_array.is_empty() {
        return Err(format!("No device found with Azure AD Device ID: {azure_ad_device_id}"));
    }

    // Get the first (and should be only) device
    let device = &devices_array[0];
    let object_id = device
        .get("id")
        .and_then(|id| id.as_str())
        .ok_or_else(|| "Device Object ID not found in response".to_string())?;

    debug!(object_id = %object_id, azure_ad_device_id = %azure_ad_device_id, "Successfully found Object ID for Azure AD Device ID");
    
    Ok(object_id.to_string())
}

/// Extract all email addresses from Microsoft Graph user data
fn extract_user_emails(ms_user: &MicrosoftGraphUser) -> Vec<(String, String, bool)> {
    let mut emails = Vec::new();
    
    debug!("Extracting emails for user: {} (ID: {})", ms_user.user_principal_name, ms_user.id);
    
    // Primary email (mail field)
    if let Some(mail) = &ms_user.mail {
        if !mail.is_empty() && mail.contains('@') {
            emails.push((mail.clone(), "primary".to_string(), true));
            trace!("Added primary email: {}", mail);
        }
    }
    
    // User Principal Name (if different from mail)
    if !ms_user.user_principal_name.is_empty() && 
       ms_user.user_principal_name.contains('@') &&
       !emails.iter().any(|(e, _, _)| e == &ms_user.user_principal_name) {
        let email_type = if emails.is_empty() { "primary".to_string() } else { "upn".to_string() };
        emails.push((ms_user.user_principal_name.clone(), email_type.clone(), true));
        trace!("Added UPN email: {} (type: {})", ms_user.user_principal_name, email_type);
    }
    
    // Proxy addresses (SMTP addresses from Exchange)
    if let Some(proxy_addresses) = &ms_user.proxy_addresses {
        trace!("Processing {} proxy addresses", proxy_addresses.len());
        for proxy in proxy_addresses {
            if let Some(email) = extract_smtp_address(proxy) {
                if !emails.iter().any(|(e, _, _)| e == &email) {
                    let email_type = if proxy.starts_with("SMTP:") {
                        "primary".to_string() // SMTP: (uppercase) indicates primary
                    } else {
                        "alias".to_string() // smtp: (lowercase) indicates alias
                    };
                    emails.push((email.clone(), email_type.clone(), true));
                    trace!("Added proxy email: {} (type: {})", email, email_type);
                } else {
                    trace!("Skipped duplicate proxy email: {}", email);
                }
            } else {
                trace!("Failed to extract email from proxy address: {}", proxy);
            }
        }
    }
    
    // Other mail addresses
    if let Some(other_mails) = &ms_user.other_mails {
        trace!("Processing {} other mail addresses", other_mails.len());
        for email in other_mails {
            if !email.is_empty() && 
               email.contains('@') && 
               !emails.iter().any(|(e, _, _)| e == email) {
                emails.push((email.clone(), "other".to_string(), true));
                trace!("Added other email: {}", email);
            } else {
                trace!("Skipped invalid or duplicate other email: {}", email);
            }
        }
    }
    
    // If no emails found, use the userPrincipalName as a fallback
    if emails.is_empty() && !ms_user.user_principal_name.is_empty() {
        emails.push((ms_user.user_principal_name.clone(), "primary".to_string(), true));
        debug!("Added fallback UPN email: {}", ms_user.user_principal_name);
    }
    
    debug!("Extracted {} emails for user {}", emails.len(), ms_user.user_principal_name);
    
    emails
}

/// Extract email address from Exchange proxy address format
fn extract_smtp_address(proxy_address: &str) -> Option<String> {
    if proxy_address.starts_with("SMTP:") {
        Some(proxy_address[5..].to_string())
    } else if proxy_address.starts_with("smtp:") {
        Some(proxy_address[5..].to_string())
    } else if proxy_address.contains('@') {
        // Sometimes proxy addresses don't have the SMTP: prefix
        Some(proxy_address.to_string())
    } else {
        None
    }
}

/// Parse Microsoft Graph datetime string to NaiveDateTime
fn parse_microsoft_datetime(datetime_str: &Option<String>) -> Option<chrono::NaiveDateTime> {
    datetime_str.as_ref().and_then(|s| {
        // Microsoft Graph typically returns ISO 8601 format: "2024-01-15T10:30:00Z"
        chrono::DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|dt| dt.naive_utc())
            .or_else(|| {
                // Fallback: try parsing without timezone
                chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
                    .ok()
                    .or_else(|| {
                        // Another fallback: try with milliseconds
                        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
                            .ok()
                    })
            })
    })
}

/// Process a Microsoft user without fetching profile photos (for fast sync)
#[instrument(level = "debug", skip(conn, stats), fields(user_principal_name = %ms_user.user_principal_name, provider_id))]
async fn process_microsoft_user_no_photos(
    conn: &mut DbConnection,
    provider_id: i32,
    ms_user: &MicrosoftGraphUser,
    stats: &mut UserSyncStats,
) -> Result<(), String> {
    // Check if identity already exists
    match find_identity_by_provider_user_id(conn, provider_id, &ms_user.id) {
        Ok(existing_identity) => {
            // Identity exists - update the Microsoft user without photos
            update_existing_microsoft_user_no_photos(conn, ms_user, existing_identity, stats).await
        }
        Err(diesel::result::Error::NotFound) => {
            // No identity found, check if linking to an existing user by email is possible
            let emails = extract_user_emails(ms_user);
            
            if let Some(existing_user) = find_existing_user_by_emails(conn, &emails) {
                // Link existing user to Microsoft account without photos
                link_existing_user_to_microsoft_no_photos(conn, provider_id, ms_user, existing_user, stats).await
            } else {
                // Create new user without photos
                create_new_user_from_microsoft_no_photos(conn, provider_id, ms_user, stats).await
            }
        }
        Err(e) => {
            Err(format!("Database error checking for existing identity: {e}"))
        }
    }
}

/// Background photo sync task (simplified)
async fn background_photo_sync_task(
    db_pool: web::Data<Pool>,
    provider_id: i32,
    session_id: String,
    access_token: String,
) -> Result<(), String> {
    info!("Starting background photo sync for provider {}", provider_id);
    
    update_sync_progress_with_type(
        &session_id,
        "photos",
        0,
        0,
        "starting",
        "Finding users without profile photos...",
        "photos",
        None
    );

    // Get database connection
    let mut conn = db_pool.get().map_err(|e| format!("Database connection failed: {e}"))?;

    // Find users that need photo sync using SQL query
    let users_needing_photos = find_users_without_photos(&mut conn, provider_id)?;

    let total_users = users_needing_photos.len();
    if total_users == 0 {
        info!("No users need photo sync");
        update_sync_progress_with_type(
            &session_id,
            "photos",
            0,
            0,
            "completed",
            "No users found needing photo sync",
            "photos",
            None
        );
        return Ok(());
    }

    info!("Found {} users needing photo sync", total_users);

    // Create HTTP client for photo downloads
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let mut processed = 0;
    let mut success_count = 0;

    // Process photos sequentially (simple approach)
    for (_user_id, user_uuid_str, ms_user_id) in users_needing_photos {
        // Parse UUID string to Uuid type
        let user_uuid = match uuid::Uuid::parse_str(&user_uuid_str) {
            Ok(uuid) => uuid,
            Err(e) => {
                debug!("Failed to parse UUID {}: {}", user_uuid_str, e);
                processed += 1;
                continue;
            }
        };

        match sync_user_photo_by_id(&client, &access_token, &ms_user_id, &user_uuid_str).await {
            Ok(photo_urls) => {
                if let Err(e) = update_user_avatar_by_id(&mut conn, &user_uuid, photo_urls.avatar_url, photo_urls.avatar_thumb).await {
                    debug!("Failed to update user avatar: {}", e);
                } else {
                    success_count += 1;
                }
            }
            Err(e) => {
                debug!("Photo sync error for user {}: {}", user_uuid_str, e);
            }
        }

        processed += 1;

        // Update progress every 10 photos
        if processed % 10 == 0 || processed == total_users {
            update_sync_progress_with_type(
                &session_id,
                "photos",
                processed,
                total_users,
                "running",
                &format!("Processed {processed}/{total_users} photos ({success_count} success)"),
                "photos",
                None
            );
        }
    }

    let final_message = format!("Background photo sync completed: {success_count}/{total_users} success");
    info!("{}", final_message);

    update_sync_progress_with_type(
        &session_id,
        "photos",
        total_users,
        total_users,
        "completed",
        &final_message,
        "photos",
        None
    );

    Ok(())
}

/// Update existing Microsoft user without photos (simplified)
async fn update_existing_microsoft_user_no_photos(
    conn: &mut DbConnection,
    ms_user: &MicrosoftGraphUser,
    existing_identity: UserAuthIdentity,
    stats: &mut UserSyncStats,
) -> Result<(), String> {
    // Get the associated user
    let user = user_repo::get_user_by_uuid(&existing_identity.user_uuid, conn)
        .map_err(|e| format!("Failed to get user by UUID {}: {}", existing_identity.user_uuid, e))?;

    // Extract emails and update user info
    let emails = extract_user_emails(ms_user);
    let _primary_email = emails.first().map(|(email, _, _)| email.clone())
        .unwrap_or_else(|| ms_user.user_principal_name.clone());

    let updated_name = ms_user.display_name.as_ref().unwrap_or(&user.name);

    // Update user if needed
    let user_update = crate::models::UserUpdate {
        name: if updated_name != &user.name { Some(updated_name.clone()) } else { None },
        role: None,
        pronouns: None,
        avatar_url: None,
        banner_url: None,
        avatar_thumb: None,
        theme: None, // Preserve user's theme preference
        microsoft_uuid: Some(utils::parse_uuid(&ms_user.id).map_err(|_| "Invalid Microsoft UUID format")?),
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    if user_update.name.is_some() || false /* email removed */ || user_update.microsoft_uuid.is_some() {
        user_repo::update_user(&user.uuid, user_update, conn)
            .map_err(|e| format!("Failed to update user: {e}"))?;
    }

    // Store emails if any
    if !emails.is_empty() {
        let email_data: Vec<(String, String, bool, String)> = emails
            .into_iter()
            .map(|(email, email_type, verified)| (email, email_type, verified, "microsoft".to_string()))
            .collect();

        let _ = user_emails_repo::add_multiple_emails(conn, &user.uuid, email_data);
    }

    // Update identity data
    let identity_data = serde_json::to_value(ms_user)
        .map_err(|e| format!("Failed to serialize Microsoft user data: {e}"))?;

    update_identity_data(conn, existing_identity.id, Some(identity_data))
        .map_err(|e| format!("Failed to update identity data: {e}"))?;

    stats.existing_users_updated += 1;
    Ok(())
}

/// Link existing user to Microsoft without photos (simplified)
async fn link_existing_user_to_microsoft_no_photos(
    conn: &mut DbConnection,
    _provider_id: i32,
    ms_user: &MicrosoftGraphUser,
    existing_user: User,
    stats: &mut UserSyncStats,
) -> Result<(), String> {
    // Create Microsoft identity
    let identity_data = serde_json::to_value(ms_user)
        .map_err(|e| format!("Failed to serialize Microsoft user data: {e}"))?;

    let new_identity = NewUserAuthIdentity {
        user_uuid: existing_user.uuid,
        provider_type: "microsoft".to_string(),
        external_id: ms_user.id.clone(),
        email: ms_user.mail.clone(),
        metadata: Some(identity_data),
        password_hash: None,
    };

    identity_repo::create_identity(new_identity, conn)
        .map_err(|e| format!("Failed to create Microsoft identity: {e}"))?;

    // Update user with Microsoft UUID
    let user_update = crate::models::UserUpdate {
        name: None,
        role: None,
        pronouns: None,
        avatar_url: None,
        banner_url: None,
        avatar_thumb: None,
        theme: None, // Preserve user's theme preference
        microsoft_uuid: Some(utils::parse_uuid(&ms_user.id).map_err(|_| "Invalid Microsoft UUID format")?),
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    user_repo::update_user(&existing_user.uuid, user_update, conn)
        .map_err(|e| format!("Failed to update user with Microsoft UUID: {e}"))?;

    stats.identities_linked += 1;
    Ok(())
}

/// Create new user from Microsoft without photos (simplified)
async fn create_new_user_from_microsoft_no_photos(
    conn: &mut DbConnection,
    _provider_id: i32,
    ms_user: &MicrosoftGraphUser,
    stats: &mut UserSyncStats,
) -> Result<(), String> {
    // Generate UUID for new user
    let user_uuid = Uuid::now_v7();

    // Determine name and email
    let name = ms_user.display_name.clone()
        .or_else(|| {
            match (&ms_user.given_name, &ms_user.surname) {
                (Some(first), Some(last)) => Some(format!("{first} {last}")),
                (Some(first), None) => Some(first.clone()),
                (None, Some(last)) => Some(last.clone()),
                _ => None,
            }
        })
        .unwrap_or_else(|| ms_user.user_principal_name.clone());

    let primary_email = ms_user.mail.as_ref().unwrap_or(&ms_user.user_principal_name).clone();

    // Create user with Microsoft UUID
    let microsoft_uuid = Some(utils::parse_uuid(&ms_user.id).map_err(|_| "Invalid Microsoft UUID format")?);
    let new_user = utils::NewUserBuilder::microsoft_user(
        name.clone(),
        primary_email,
        crate::models::UserRole::User,
        microsoft_uuid
    )
    .with_uuid(user_uuid)
    .build();

    let created_user = user_repo::create_user(new_user, conn)
        .map_err(|e| format!("Failed to create user: {e}"))?;

    // Create Microsoft identity
    let identity_data = serde_json::to_value(ms_user)
        .map_err(|e| format!("Failed to serialize Microsoft user data: {e}"))?;

    let new_identity = NewUserAuthIdentity {
        user_uuid: created_user.uuid,
        provider_type: "microsoft".to_string(),
        external_id: ms_user.id.clone(),
        email: ms_user.mail.clone(),
        metadata: Some(identity_data),
        password_hash: None,
    };

    identity_repo::create_identity(new_identity, conn)
        .map_err(|e| format!("Failed to create Microsoft identity: {e}"))?;

    // Store emails if any
    let emails = extract_user_emails(ms_user);
    if !emails.is_empty() {
        let email_data: Vec<(String, String, bool, String)> = emails
            .into_iter()
            .map(|(email, email_type, verified)| (email, email_type, verified, "microsoft".to_string()))
            .collect();

        let _ = user_emails_repo::add_multiple_emails(conn, &created_user.uuid, email_data);
    }

    info!("Created new user: {}", name);
    stats.new_users_created += 1;
    Ok(())
}

/// Find existing user by emails (simplified)
fn find_existing_user_by_emails(
    conn: &mut DbConnection,
    emails: &Vec<(String, String, bool)>,
) -> Option<User> {
    for (email, _, _) in emails {
        if let Ok(user) = user_repo::get_user_by_email(email, conn) {
            return Some(user);
        }
    }
    None
}

/// Find users without photos using SQL query (simplified)
fn find_users_without_photos(
    conn: &mut DbConnection,
    _provider_id: i32,
) -> Result<Vec<(i32, String, String)>, String> {
    use crate::schema::{users, user_auth_identities};
    use diesel::prelude::*;

    // Query for users without photos and get their data
    let results: Vec<(uuid::Uuid, String)> = users::table
        .inner_join(user_auth_identities::table.on(users::uuid.eq(user_auth_identities::user_uuid)))
        .filter(user_auth_identities::provider_type.eq("microsoft"))
        .filter(users::avatar_url.is_null())
        .select((users::uuid, user_auth_identities::external_id))
        .load(conn)
        .map_err(|e| format!("Failed to find users without photos: {e}"))?;

    // Convert UUID to String - no longer have an id field, but the function signature expects (i32, String, String)
    // Return type expects (i32, String, String), but using UUIDs now.
    // Return (uuid_string, uuid_string, external_id) and update the function signature
    let converted_results = results
        .into_iter()
        .map(|(uuid, external_id)| (0, uuid.to_string(), external_id)) // Using 0 as placeholder for deprecated id
        .collect();

    Ok(converted_results)
}

/// Sync user photo by ID (simplified)
async fn sync_user_photo_by_id(
    client: &reqwest::Client,
    access_token: &str,
    ms_user_id: &str,
    user_uuid: &str,
) -> Result<PhotoSyncUrls, String> {
    // Try to download 120x120 photo first
    let photo_url = format!("https://graph.microsoft.com/v1.0/users/{ms_user_id}/photos/120x120/$value");
    
    let response = client
        .get(&photo_url)
        .header("Authorization", format!("Bearer {access_token}"))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch profile photo: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Profile photo not found: {}", response.status()));
    }

    let photo_bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read profile photo data: {e}"))?;

    if photo_bytes.is_empty() {
        return Err("Empty profile photo".to_string());
    }

    // Save photo to disk and return PhotoSyncUrls
    let avatar_url = save_profile_photo_to_disk(&photo_bytes, user_uuid, "120x120").await?;
    
    Ok(PhotoSyncUrls {
        avatar_url,
        avatar_thumb: None, // Thumbnail support can be added later
    })
}