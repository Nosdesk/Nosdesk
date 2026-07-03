use actix_web::{web, HttpMessage, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use urlencoding;
use uuid::Uuid;
// Removed unused imports: std::path::Path, tokio::fs, tokio::io::AsyncWriteExt, image::{ImageFormat, DynamicImage}, tracing::{span, Level}
use tracing::{debug, error, info, instrument, trace, warn};

use crate::db::{DbConnection, Pool};
use crate::handlers::errors;
use crate::handlers::helpers;
// Auth providers are now configured via environment variables
use crate::config_utils;
use crate::models::{
    AuthProvider, NewSyncHistory, NewUserAuthIdentity, SyncHistoryUpdate, User, UserAuthIdentity,
};
use crate::repository::assets as asset_repo;
use crate::repository::groups as groups_repo;
use crate::repository::sync_history as sync_history_repo;
use crate::repository::user_auth_identities as identity_repo;
use crate::repository::user_emails as user_emails_repo;
use crate::repository::users as user_repo;
use crate::utils;

// Helper function for environment-based auth providers
fn get_default_microsoft_provider() -> Result<AuthProvider, diesel::result::Error> {
    // Using environment variables, return a fixed provider for Microsoft
    if config_utils::get_microsoft_client_id().is_ok()
        && config_utils::get_microsoft_client_secret().is_ok()
        && config_utils::get_microsoft_tenant_id().is_ok()
    {
        Ok(AuthProvider::new(
            2,
            "Microsoft".to_string(),
            "microsoft".to_string(),
            true,
            false,
        ))
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
    let client_id = config_utils::get_microsoft_client_id()
        .map_err(|_| "MICROSOFT_CLIENT_ID not configured")?;
    let client_secret = config_utils::get_microsoft_client_secret()
        .map_err(|_| "MICROSOFT_CLIENT_SECRET not configured")?;
    let tenant_id = config_utils::get_microsoft_tenant_id()
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
        .post(format!(
            "https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/token"
        ))
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
            let error_desc = token_data
                .get("error_description")
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
    pub completed_items: usize, // cumulative items completed in prior entities
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
    #[serde(rename = "companyName")]
    pub company_name: Option<String>,
    #[serde(rename = "streetAddress")]
    pub street_address: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    #[serde(rename = "postalCode")]
    pub postal_code: Option<String>,
    pub country: Option<String>,
    #[serde(rename = "proxyAddresses")]
    pub proxy_addresses: Option<Vec<String>>,
    #[serde(rename = "otherMails")]
    pub other_mails: Option<Vec<String>>,
    #[serde(rename = "accountEnabled")]
    pub account_enabled: Option<bool>,
}

// Entra ID Asset structure from API response (/devices endpoint)
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

// Microsoft Graph Asset structure from API response (Intune managedDevice)
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

// Group sync statistics. `errors` is the legacy string-typed channel
// the admin UI's existing JSON consumer reads; `failures` is the new
// typed channel — same data, classifier-routed, used by the
// scheduler's structured tracing and by the typed
// SyncOutcome -> SyncResult projection. New code should push through
// `record_failure(&mut self, ...)` so both stay in lockstep.
#[derive(Serialize, Debug, Default)]
pub struct GroupSyncStats {
    pub groups_created: usize,
    pub groups_updated: usize,
    pub user_membership_changes: usize,
    pub device_membership_changes: usize,
    pub errors: Vec<String>,
    #[serde(skip)]
    pub failures: Vec<crate::services::msgraph::ItemFailure>,
}

// User sync statistics
#[derive(Serialize, Debug)]
pub struct UserSyncStats {
    pub new_users_created: usize,
    pub existing_users_updated: usize,
    pub identities_linked: usize,
    pub errors: Vec<String>,
    #[serde(skip)]
    pub failures: Vec<crate::services::msgraph::ItemFailure>,
}

// Asset sync statistics
#[derive(Serialize, Debug)]
pub struct DeviceSyncStats {
    pub new_devices_created: usize,
    pub existing_devices_updated: usize,
    pub devices_assigned: usize,
    pub errors: Vec<String>,
    #[serde(skip)]
    pub failures: Vec<crate::services::msgraph::ItemFailure>,
}

/// Shared "push a failure into a sync-stats struct" surface. The
/// three stats structs are nominally distinct (different counter
/// fields per entity) but they all carry the same dual error
/// channel — legacy `errors: Vec<String>` for the admin UI's JSON
/// consumer, typed `failures: Vec<ItemFailure>` for the scheduler /
/// structured tracing / SyncOutcome projection. The trait keeps
/// the dual write in lockstep so a future PR can't silently break
/// one of the two.
trait SyncStatsExt {
    fn errors_mut(&mut self) -> &mut Vec<String>;
    fn failures_mut(&mut self) -> &mut Vec<crate::services::msgraph::ItemFailure>;

    /// Push a typed failure. Emits the structured warn (entity,
    /// external_id, error_kind, classification, attempt) via the
    /// pipeline helper, mirrors a classifier-only line into the
    /// legacy string channel for the admin UI, and stores the
    /// typed record for the outcome aggregate.
    fn record_failure(
        &mut self,
        entity: crate::services::msgraph::EntityKind,
        external_id: &str,
        error: crate::services::msgraph::MsGraphSyncError,
        attempt: u32,
    ) {
        // Classifier-only string: error's Display contract excludes
        // user-typed content (name, email, ticket title), so this
        // legacy line is safe to surface in the admin UI without
        // re-leaking PII the typed channel already filters out.
        let legacy_line = format!("{} {}: {}", entity.as_str(), external_id, error);
        let failure = crate::services::msgraph::record_failure(entity, external_id, error, attempt);
        self.errors_mut().push(legacy_line);
        self.failures_mut().push(failure);
    }
}

impl SyncStatsExt for UserSyncStats {
    fn errors_mut(&mut self) -> &mut Vec<String> {
        &mut self.errors
    }
    fn failures_mut(&mut self) -> &mut Vec<crate::services::msgraph::ItemFailure> {
        &mut self.failures
    }
}

impl SyncStatsExt for DeviceSyncStats {
    fn errors_mut(&mut self) -> &mut Vec<String> {
        &mut self.errors
    }
    fn failures_mut(&mut self) -> &mut Vec<crate::services::msgraph::ItemFailure> {
        &mut self.failures
    }
}

impl SyncStatsExt for GroupSyncStats {
    fn errors_mut(&mut self) -> &mut Vec<String> {
        &mut self.errors
    }
    fn failures_mut(&mut self) -> &mut Vec<crate::services::msgraph::ItemFailure> {
        &mut self.failures
    }
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
    completed_items: Option<usize>,
}

impl<'a> SyncProgressUpdate<'a> {
    /// Create a new progress update with the given parameters
    fn new(
        session_id: &'a str,
        entity: &'a str,
        current: usize,
        total: usize,
        status: &'a str,
        message: &'a str,
    ) -> Self {
        Self {
            session_id,
            entity,
            current,
            total,
            status,
            message,
            sync_type: entity, // Default sync_type to entity
            is_delta: None,
            completed_items: None,
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

    /// Set the completed_items offset (items completed in prior entities)
    fn with_completed_items(mut self, completed_items: usize) -> Self {
        self.completed_items = Some(completed_items);
        self
    }

    /// Apply the update to the in-memory progress map
    fn apply(self) {
        let now = Utc::now();

        if let Ok(mut progress_map) = SYNC_PROGRESS.lock() {
            // Preserve existing values if not explicitly provided
            let existing = progress_map.get(self.session_id);
            let started_at = existing.map(|p| p.started_at).unwrap_or(now);
            let preserved_is_delta = self
                .is_delta
                .unwrap_or_else(|| existing.map(|p| p.is_delta).unwrap_or(false));
            let preserved_completed_items = self
                .completed_items
                .unwrap_or_else(|| existing.map(|p| p.completed_items).unwrap_or(0));

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
                completed_items: preserved_completed_items,
            };
            progress_map.insert(self.session_id.to_string(), progress);
        }
    }
}

// Helper functions for progress tracking (convenience wrappers)
fn update_sync_progress(
    session_id: &str,
    entity: &str,
    current: usize,
    total: usize,
    status: &str,
    message: &str,
) {
    SyncProgressUpdate::new(session_id, entity, current, total, status, message).apply();
}

fn update_sync_progress_with_type(
    session_id: &str,
    entity: &str,
    current: usize,
    total: usize,
    status: &str,
    message: &str,
    sync_type: &str,
    is_delta: Option<bool>,
) {
    let mut update = SyncProgressUpdate::new(session_id, entity, current, total, status, message)
        .with_sync_type(sync_type);
    if let Some(delta) = is_delta {
        update = update.with_is_delta(delta);
    }
    update.apply();
}

fn update_sync_progress_with_offset(
    session_id: &str,
    entity: &str,
    current: usize,
    total: usize,
    status: &str,
    message: &str,
    completed_items: usize,
) {
    SyncProgressUpdate::new(session_id, entity, current, total, status, message)
        .with_completed_items(completed_items)
        .apply();
}

fn update_sync_progress_with_type_and_offset(
    session_id: &str,
    entity: &str,
    current: usize,
    total: usize,
    status: &str,
    message: &str,
    sync_type: &str,
    is_delta: Option<bool>,
    completed_items: usize,
) {
    let mut update = SyncProgressUpdate::new(session_id, entity, current, total, status, message)
        .with_sync_type(sync_type)
        .with_completed_items(completed_items);
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
    let _conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    // Extract claims from cookie auth middleware
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    let session_id = path.into_inner();

    match get_sync_progress(&session_id) {
        Some(progress) => HttpResponse::Ok().json(progress),
        None => errors::not_found_msg("Sync session not found"),
    }
}

/// Get all active sync sessions
pub async fn get_active_syncs(
    req: actix_web::HttpRequest,
    db_pool: web::Data<Pool>,
) -> impl Responder {
    let _conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    // Extract claims from cookie auth middleware
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    if let Ok(progress_map) = SYNC_PROGRESS.lock() {
        let active_syncs: Vec<SyncProgressState> = progress_map
            .values()
            .filter(|progress| {
                // Only return syncs that are truly active (running or starting)
                progress.status == "running"
                    || progress.status == "starting"
                    || progress.status == "cancelling"
            })
            .cloned()
            .collect();

        HttpResponse::Ok().json(json!({
            "active_syncs": active_syncs,
            "count": active_syncs.len()
        }))
    } else {
        errors::internal("Failed to access sync progress")
    }
}

/// Get the most recent completed sync session
pub async fn get_last_sync(
    req: actix_web::HttpRequest,
    db_pool: web::Data<Pool>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    // Extract claims from cookie auth middleware
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
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
                message: sync_history
                    .error_message
                    .unwrap_or_else(|| "Sync completed".to_string()),
                started_at: DateTime::from_naive_utc_and_offset(sync_history.started_at, Utc),
                updated_at: DateTime::from_naive_utc_and_offset(
                    sync_history.completed_at.unwrap_or(sync_history.started_at),
                    Utc,
                ),
                sync_type: sync_history.sync_type,
                is_delta: sync_history.is_delta,
                completed_items: 0,
            };
            HttpResponse::Ok().json(response)
        }
        Err(_) => {
            // Fallback to in-memory storage if database query fails
            if let Ok(progress_map) = SYNC_PROGRESS.lock() {
                let last_sync = progress_map
                    .values()
                    .filter(|progress| {
                        progress.status == "completed"
                            || progress.status == "error"
                            || progress.status == "cancelled"
                    })
                    .max_by_key(|progress| progress.updated_at);

                match last_sync {
                    Some(sync) => HttpResponse::Ok().json(sync),
                    None => HttpResponse::Ok().json(json!(null)),
                }
            } else {
                errors::internal("Failed to access sync progress")
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
    let _conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    // Extract claims from cookie auth middleware
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    let session_id = path.into_inner();

    // Check if the session exists and is cancellable
    if let Some(progress) = get_sync_progress(&session_id) {
        if progress.status == "running" || progress.status == "starting" {
            cancel_sync(&session_id);
            update_sync_progress_with_type(
                &session_id,
                &progress.entity,
                progress.current,
                progress.total,
                "cancelling",
                "Cancellation requested",
                &progress.sync_type,
                None,
            );

            HttpResponse::Ok().json(json!({
                "success": true,
                "message": "Sync cancellation requested"
            }))
        } else {
            errors::bad_request("Sync is not running")
        }
    } else {
        errors::not_found_msg("Sync session not found")
    }
}

/// Validate Microsoft Graph configuration
pub async fn get_config_validation(req: actix_web::HttpRequest) -> impl Responder {
    // Extract claims from cookie auth middleware
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    let mut missing_fields = Vec::new();

    // Check and retrieve each required environment variable via config_utils
    let client_id = config_utils::get_microsoft_client_id().ok();
    let client_secret = config_utils::get_microsoft_client_secret().ok();
    let tenant_id = config_utils::get_microsoft_tenant_id().ok();
    let redirect_uri = config_utils::get_microsoft_redirect_uri().ok();

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
    let _conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    // Extract claims from cookie auth middleware
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Check if Microsoft is configured via environment variables
    let microsoft_configured = config_utils::get_microsoft_client_id().is_ok()
        && config_utils::get_microsoft_client_secret().is_ok()
        && config_utils::get_microsoft_tenant_id().is_ok();

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

    // Look up the most recent sync_history row to populate the
    // last_sync field. The row is recorded per sync run by
    // sync_history_repo::create_sync_history (see the scheduled
    // delta-sync path); here we just surface its completed_at so
    // admins can see when sync last ran without having to grep
    // logs. Treat lookup failure as "no sync yet" rather than an
    // error — a brand-new install has nothing to report.
    // sync_history is RLS-enabled (Phase 3c.2). The connection-
    // status endpoint reads the most-recent sync row across all
    // workspaces (admin view), so background_run with bypass is
    // correct here.
    let last_sync =
        crate::sync::session::background_run(&db_pool, "background:msgraph_status", |conn| {
            crate::repository::sync_history::get_last_completed_sync(conn).map(|h| {
                h.completed_at.map(|naive| {
                    chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc)
                })
            })
        })
        .ok()
        .flatten();

    HttpResponse::Ok().json(ConnectionStatus {
        status: "connected".to_string(),
        message: "Microsoft Graph connection is configured and ready".to_string(),
        last_sync,
        available_entities: vec![
            "users".to_string(),
            "devices".to_string(),
            "groups".to_string(),
        ],
    })
}

/// Test Microsoft Graph connection
pub async fn test_connection(req: actix_web::HttpRequest) -> impl Responder {
    // Extract claims from cookie auth middleware
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    tracing::info!("🔬 Testing Microsoft Graph connection");

    // Get Microsoft provider
    let provider = match get_default_microsoft_provider() {
        Ok(provider) => provider,
        Err(_) => return errors::bad_request("Microsoft auth provider not found"),
    };

    // Test the connection by making a simple Graph API call
    match test_graph_connection(provider.id).await {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(error) => HttpResponse::Ok().json(json!({
            "success": false,
            "status": "error",
            "message": format!("Connection test failed: {}", error)
        })),
    }
}

/// Sync data from Microsoft Graph
#[instrument(level = "info", skip(req, db_pool, request, ws), fields(count = request.entities.len()))]
pub async fn sync_data(
    req: actix_web::HttpRequest,
    db_pool: web::Data<Pool>,
    ws: crate::extractors::WorkspaceContext,
    request: web::Json<SyncDataRequest>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    // Triggering a full Entra/Intune directory sync (mass record
    // create/update via the integration's credentials) is workspace-
    // admin only. See security-audit-2026-06.
    let _claims =
        match crate::utils::rbac::require_workspace_role(&req, crate::models::WorkspaceRole::Admin)
        {
            Ok(c) => c,
            Err(resp) => return resp,
        };

    // Get Microsoft provider
    let provider = match get_default_microsoft_provider() {
        Ok(provider) => provider,
        Err(_) => return errors::bad_request("Microsoft auth provider not found"),
    };

    // Validate configuration before spawning background task
    if let Err(e) = check_microsoft_config() {
        return errors::bad_request(format!("Microsoft Graph configuration invalid: {e}"));
    }

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
    } else if entities.iter().any(|e| e == "devices") {
        "devices".to_string()
    } else if entities.iter().any(|e| e == "users") {
        "users".to_string()
    } else if entities.iter().any(|e| e == "groups") {
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
            return errors::internal("Failed to create sync history record");
        }
    };

    let session_id = sync_history.id.to_string();
    info!("Created sync history record with ID: {}", session_id);

    // Initialize session and progress tracking
    initialize_sync_session(&session_id);

    update_sync_progress_with_type(
        &session_id,
        "initializing",
        0,
        0,
        "starting",
        "Initializing sync process",
        &sync_type,
        Some(use_delta),
    );

    // Start the sync process in the background
    let provider_id = provider.id;
    let session_id_clone = session_id.clone();
    let sync_workspace_id = ws.workspace_id;

    // perform_sync holds the conn across many async Microsoft Graph
    // fetches and writes to sync_history (RLS-enabled) plus users
    // / groups / devices (mix of RLS and non-RLS). Same async-mixed-
    // with-DB shape as the channels poll loop, so we use the same
    // session-level elevation pattern: SET ROLE nosdesk_admin +
    // pin app.workspace_id for the spawn's lifetime, RESET ROLE
    // and clear the GUCs before the conn drops back into the pool.
    tokio::spawn(async move {
        let mut conn = match db_pool.get() {
            Ok(conn) => conn,
            Err(_) => {
                update_sync_progress(
                    &session_id_clone,
                    "error",
                    0,
                    0,
                    "error",
                    "Database connection failed",
                );
                return;
            }
        };

        let actor = crate::sync::actor::ActorContext::system("background:msgraph_sync")
            .with_workspace(sync_workspace_id);
        if let Err(e) = crate::sync::session::elevate_session_role(&mut conn, &actor) {
            error!("Failed to elevate session for msgraph sync: {}", e);
            update_sync_progress(
                &session_id_clone,
                "error",
                0,
                0,
                "error",
                "Session elevation failed",
            );
            return;
        }

        match perform_sync(
            &mut conn,
            provider_id,
            &entities,
            &session_id_clone,
            use_delta,
        )
        .await
        {
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
                            Ok(_) => {
                                info!("Successfully updated sync history for session {}", sync_id)
                            }
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
                        None,
                    );

                    // Check if background photo sync should start after user sync completes
                    if entities.iter().any(|e| e == "users") && sync_result.total_processed > 0 {
                        let background_photo_sync = std::env::var("MSGRAPH_BACKGROUND_PHOTOS")
                            .ok()
                            .and_then(|v| v.parse::<bool>().ok())
                            .unwrap_or(true);

                        if background_photo_sync {
                            info!(
                                "Starting background photo sync for {} processed users",
                                sync_result.total_processed
                            );
                            let db_pool_bg = db_pool.clone();
                            let session_id_bg = session_id_clone.clone();

                            tokio::spawn(async move {
                                // Give the main sync a moment to finish database operations
                                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

                                // Get access token for photo sync
                                match fetch_microsoft_graph_users_optimized(provider_id).await {
                                    Ok((_, access_token)) => {
                                        if let Err(e) = background_photo_sync_task(
                                            db_pool_bg,
                                            provider_id,
                                            session_id_bg,
                                            access_token,
                                        )
                                        .await
                                        {
                                            error!("Background photo sync failed: {}", e);
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to get access token for background photo sync: {}", e);
                                    }
                                }
                            });
                        }
                    }
                }
            }
            Err(error) => {
                let error_message = format!("Sync failed: {error}");
                error!("Sync failed for session {}: {}", session_id_clone, error);

                update_sync_progress_with_type(
                    &session_id_clone,
                    &sync_type,
                    0,
                    0,
                    "error",
                    &error_message,
                    &sync_type,
                    None,
                );

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

        // Always RESET ROLE + clear actor GUCs before the conn
        // drops back into the pool so the elevated state from
        // elevate_session_role above doesn't leak to the next
        // checkout.
        crate::sync::session::reset_session_role(&mut conn);
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

/// Scheduler-callable entry point. Runs a delta sync of users + devices
/// + groups against the default Microsoft provider, intended for
/// periodic invocation via [`crate::services::scheduler`].
///
/// This is a thin wrapper over the HTTP handler's background path
/// (`perform_sync`) — it skips the claims/session-id machinery the
/// interactive sync view needs and reports outcomes through the
/// scheduler's own status registry instead of writing `sync_history`
/// rows. When the user-initiated handler and the scheduled job
/// eventually want to share a sync_history audit trail we can hoist
/// that logic — for now the two paths coexist without interfering.
///
/// Returns early with `Ok(())` when Microsoft credentials are not
/// configured so the job doesn't spam errors on installs that aren't
/// using Intune integration.
pub async fn run_scheduled_delta_sync(pool: &crate::db::Pool) -> anyhow::Result<()> {
    if check_microsoft_config().is_err() {
        // Not configured — treat as a clean no-op so the scheduler
        // status registry shows "ok" instead of a noisy error.
        debug!("MS Graph scheduled sync skipped — provider not configured");
        return Ok(());
    }

    let provider = get_default_microsoft_provider()
        .map_err(|e| anyhow::anyhow!("MS Graph provider lookup failed: {e}"))?;

    let mut conn = pool.get().map_err(|e| anyhow::anyhow!("db pool: {e}"))?;

    // perform_sync writes sync_history (RLS) plus users/groups/devices across
    // many async Graph fetches, same shape as the interactive path. MS Graph
    // is configured by instance env vars, so it targets the bootstrap
    // workspace. Elevate + pin for the run's lifetime and reset before the
    // conn returns to the pool. Without this the RLS writes silently fail.
    let actor = crate::sync::actor::ActorContext::system("scheduler:msgraph_delta_sync")
        .with_workspace(crate::sync::actor::BOOTSTRAP_WORKSPACE_ID);
    crate::sync::session::elevate_session_role(&mut conn, &actor)
        .map_err(|e| anyhow::anyhow!("session elevation failed: {e}"))?;

    let entities = vec![
        "users".to_string(),
        "devices".to_string(),
        "groups".to_string(),
    ];
    // Synthetic session id — `update_sync_progress` writes to an
    // in-memory map keyed on the id. Scheduled runs don't surface
    // progress through the admin UI, so the id is used exclusively as
    // a cleanup key. The Drop guard below removes both map entries
    // on any exit path (success, error, or panic) — without it, each
    // 30-min tick would leak a handful of bytes into the statics.
    let session_id = uuid::Uuid::new_v4().to_string();
    initialize_sync_session(&session_id);
    let _guard = SyncSessionGuard {
        session_id: session_id.clone(),
    };

    // Job-level Result answers "did the sync run?" not "were all
    // items perfect?". Partial item failures are a normal operating
    // state for a delta sync (a single user with a malformed email,
    // a transient HTTP blip, etc.) — they get logged at the failure
    // site with structured fields and don't bubble up here, because
    // the alternative is `anyhow::anyhow!(...)` which captures a
    // backtrace and dumps a 60-frame stack trace into the scheduler
    // log for what's effectively normal noise.
    //
    // Only "couldn't even start" failures (token fetch error, DB
    // unreachable, internal sync-machinery bug) return Err. Those
    // are real operator-attention events the scheduler's status
    // registry should reflect as a failed run.
    let outcome = perform_sync(&mut conn, provider.id, &entities, &session_id, true).await;
    // Reset the session elevation before the conn drops back into the pool,
    // so the bypass + workspace pin can't leak across checkouts.
    crate::sync::session::reset_session_role(&mut conn);
    match outcome {
        Ok(result) => {
            if result.total_errors > 0 {
                tracing::warn!(
                    provider_id = provider.id,
                    processed = result.total_processed,
                    failed = result.total_errors,
                    "msgraph scheduled sync completed with partial item failures"
                );
            }
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!("msgraph sync machinery failed: {e}")),
    }
}

/// RAII guard that drops the per-run entries from both `SYNC_PROGRESS`
/// and `SYNC_CANCELLATION` when the scheduled sync finishes. Scoped
/// to this module because only the scheduled-sync path needs it —
/// interactive syncs keep their entries so the admin UI can render
/// the final state after the sync returns.
struct SyncSessionGuard {
    session_id: String,
}

impl Drop for SyncSessionGuard {
    fn drop(&mut self) {
        if let Ok(mut map) = SYNC_PROGRESS.lock() {
            map.remove(&self.session_id);
        }
        if let Ok(mut map) = SYNC_CANCELLATION.lock() {
            map.remove(&self.session_id);
        }
    }
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
                5. Add: Organization.Read.All, User.Read.All, Asset.Read.All, Group.Read.All\n\
                6. Click 'Grant admin consent for [Your Tenant]' (requires Global Admin)\n\
                7. Wait 5-10 minutes for permissions to propagate\n\n\
                Common required permissions for full functionality:\n\
                • Organization.Read.All - Read tenant information\n\
                • User.Read.All - Read user profiles\n\
                • Asset.Read.All - Read device information\n\
                • Group.Read.All - Read group information\n\
                • DeviceManagementManagedDevices.Read.All - Read Intune devices\n\n\
                Error Code: {error_code}"
            ));
        }

        Err(format!(
            "Microsoft Graph API error ({} {}): {} (Error Code: {})",
            status.as_u16(),
            status.canonical_reason().unwrap_or("Unknown"),
            error_msg,
            error_code
        ))
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
    let primary_sync_type = if entities.iter().any(|e| e == "devices") {
        "devices"
    } else if entities.iter().any(|e| e == "users") {
        "users"
    } else if entities.iter().any(|e| e == "groups") {
        "groups"
    } else {
        "sync"
    };

    // Track cumulative items completed across entities for overall progress
    let mut completed_items: usize = 0;

    // Don't overwrite entity-specific progress - just process each entity
    for entity in entities.iter() {
        let sync_progress = match entity.as_str() {
            "users" => sync_users(conn, provider_id, session_id, use_delta, completed_items).await,
            "devices" => {
                sync_devices(conn, provider_id, session_id, use_delta, completed_items).await
            }
            "groups" => {
                sync_groups(conn, provider_id, session_id, use_delta, completed_items).await
            }
            _ => {
                total_errors += 1;
                // Update progress with error for unsupported entity
                update_sync_progress_with_type(
                    session_id,
                    entity,
                    0,
                    0,
                    "error",
                    &format!("Unsupported entity type: {entity}"),
                    primary_sync_type,
                    None,
                );
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

        // Accumulate completed items for the next entity's offset
        completed_items += sync_progress.total;

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
    completed_items: usize,
) -> SyncProgress {
    let mut stats = UserSyncStats {
        new_users_created: 0,
        existing_users_updated: 0,
        identities_linked: 0,
        errors: Vec::new(),
        failures: Vec::new(),
    };

    let sync_mode_msg = if use_delta { "delta" } else { "full" };
    update_sync_progress_with_offset(
        session_id,
        "users",
        0,
        0,
        "running",
        &format!("Fetching users from Microsoft Graph ({sync_mode_msg})"),
        completed_items,
    );

    // Step 1: Fetch users from Microsoft Graph using delta query
    let delta_result = match fetch_microsoft_graph_users_delta(conn, use_delta).await {
        Ok(result) => result,
        Err(error) => {
            update_sync_progress_with_offset(
                session_id,
                "users",
                0,
                0,
                "error",
                &format!("Failed to fetch users: {error}"),
                completed_items,
            );
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

    // Handle removed users first (if delta sync returned any).
    //
    // Design choice: we revoke active sessions and record a security
    // event, but we DO NOT delete or soft-delete the users row.
    //   - Revoke severs access immediately: an Entra ID admin
    //     removing a fired employee from the source IDP propagates
    //     to "they can no longer log in to Nosdesk" on the next
    //     delta-sync run.
    //   - Keeping the row preserves historical attribution
    //     (tickets they created, comments they wrote, audit log
    //     entries). A compromised admin in the source IDP can't
    //     wipe Nosdesk data by mass-removing users.
    //   - Operators who want hard cleanup can do it via the admin
    //     UI / nosdesk-cli once they've decided.
    if !removed_user_ids.is_empty() {
        info!(
            count = removed_user_ids.len(),
            "Processing removed users from delta response"
        );
        for removed_id in &removed_user_ids {
            let Ok(identity) = find_identity_by_provider_user_id(conn, provider_id, removed_id)
            else {
                continue;
            };

            let revoked = crate::repository::active_sessions::revoke_other_sessions(
                conn,
                &identity.user_uuid,
                None,
            )
            .unwrap_or_else(|e| {
                warn!(
                    error = ?e,
                    user_uuid = %identity.user_uuid,
                    ms_id = %removed_id,
                    "Failed to revoke sessions for user removed from Entra ID",
                );
                0
            });

            if let Err(e) = crate::utils::security_events::record_security_event(
                conn,
                crate::utils::security_events::SecurityEventInput {
                    user_uuid: Some(identity.user_uuid),
                    event_type: "user_removed_via_msgraph",
                    severity: "warning",
                    details: Some(serde_json::json!({
                        "ms_id": removed_id,
                        "provider_id": provider_id,
                        "sessions_revoked": revoked,
                        "action": "access_revoked_user_row_kept",
                    })),
                    request: None,
                    session_id: None,
                },
            ) {
                warn!(
                    error = ?e,
                    user_uuid = %identity.user_uuid,
                    ms_id = %removed_id,
                    "Failed to record security event for Entra ID user removal",
                );
            }

            warn!(
                user_uuid = %identity.user_uuid,
                ms_id = %removed_id,
                sessions_revoked = revoked,
                "Entra ID reported user removed: revoked sessions, kept user row for attribution",
            );
        }
    }

    let total_users = microsoft_users.len();

    // Check if filtering disabled accounts
    let skip_disabled_accounts = std::env::var("MSGRAPH_SKIP_DISABLED_ACCOUNTS")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);

    // Performance configuration
    let background_photo_sync = std::env::var("MSGRAPH_BACKGROUND_PHOTOS")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);

    if skip_disabled_accounts {
        info!(
            "Fetched {} enabled users from Microsoft Graph (disabled accounts filtered out)",
            total_users
        );
    } else {
        info!(
            "Fetched {} users from Microsoft Graph (including disabled accounts)",
            total_users
        );
    }

    if background_photo_sync {
        info!("Background photo sync enabled - users will be created immediately, photos synced separately");
    } else {
        info!("Inline photo sync enabled - users created with photos during sync (slower)");
    }

    if total_users == 0 {
        debug!("No users found to sync from Microsoft Graph");
        update_sync_progress_with_offset(
            session_id,
            "users",
            0,
            0,
            "completed",
            "No users found to sync",
            completed_items,
        );
        return SyncProgress {
            entity: "users".to_string(),
            processed: 0,
            total: 0,
            status: "completed".to_string(),
            errors: Vec::new(),
        };
    }

    info!(
        "Starting user sync: processing {} users concurrently",
        total_users
    );
    update_sync_progress_with_offset(
        session_id,
        "users",
        0,
        total_users,
        "running",
        &format!("Processing {total_users} users concurrently"),
        completed_items,
    );

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
            update_sync_progress_with_offset(
                session_id,
                "users",
                0,
                total_users,
                "error",
                &error_msg,
                completed_items,
            );
            SyncProgress {
                entity: "users".to_string(),
                processed: 0,
                total: total_users,
                status: "error".to_string(),
                errors: vec![error_msg],
            }
        })
        .unwrap();

    // Step 2: Process users in optimized batches
    let mut processed_count = 0;
    let sync_was_cancelled = false;

    // Process users in batches with optimized database operations
    for batch in microsoft_users.chunks(user_batch_size) {
        let batch_start = processed_count;
        let batch_size = batch.len();

        update_sync_progress_with_offset(
            session_id,
            "users",
            batch_start,
            total_users,
            "running",
            &format!(
                "Processing batch {}-{} of {}",
                batch_start + 1,
                batch_start + batch_size,
                total_users
            ),
            completed_items,
        );

        // Resolve every user's Microsoft identity for this batch in one query,
        // rather than a per-user lookup inside the processors. A prefetch
        // failure is an infra-class DB error: record it and stop the pass. An
        // empty map would be unsafe here (existing users would be misread as
        // new and duplicated), so we don't fall back to one.
        let batch_external_ids: Vec<&str> = batch.iter().map(|u| u.id.as_str()).collect();
        let identity_map = match identity_repo::find_identities_by_provider_user_ids(
            conn,
            "microsoft",
            &batch_external_ids,
        ) {
            Ok(map) => map,
            Err(e) => {
                error!(error = %e, batch = batch.len(), "msgraph user sync: identity prefetch failed; stopping user pass");
                if let Some(first) = batch.first() {
                    stats.record_failure(
                        crate::services::msgraph::EntityKind::Users,
                        &first.id,
                        e.into(),
                        1,
                    );
                }
                break;
            }
        };

        // Process each user in the batch with optimized profile photo handling
        for ms_user in batch {
            // Check for cancellation before processing each user
            if is_sync_cancelled(session_id) {
                let processed = stats.new_users_created
                    + stats.existing_users_updated
                    + stats.identities_linked;
                let cancel_message = format!("Sync was cancelled by user request. Processed {} of {} users ({} created, {} updated, {} linked)", 
                    processed_count, total_users, stats.new_users_created, stats.existing_users_updated, stats.identities_linked);

                // Update progress with cancellation status
                update_sync_progress_with_type_and_offset(
                    session_id,
                    "users",
                    processed_count,
                    total_users,
                    "cancelled",
                    &cancel_message,
                    "users",
                    None,
                    completed_items,
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

            update_sync_progress_with_type_and_offset(
                session_id,
                "users",
                processed_count - 1,
                total_users,
                "running",
                &format!("Processing user: {}", ms_user.user_principal_name),
                "users",
                None,
                completed_items,
            );

            if background_photo_sync {
                // Fast sync without photos
                match process_microsoft_user_no_photos(
                    conn,
                    provider_id,
                    ms_user,
                    identity_map.get(&ms_user.id).cloned(),
                    &mut stats,
                )
                .await
                {
                    Ok(_) => {
                        trace!(
                            "Successfully processed user (without photos): {}",
                            ms_user.user_principal_name
                        );
                    }
                    Err(error) => {
                        stats.record_failure(
                            crate::services::msgraph::EntityKind::Users,
                            &ms_user.id,
                            error.into(),
                            1,
                        );
                    }
                }
            } else {
                // Traditional sync with photos inline
                match process_microsoft_user_optimized_v2(
                    conn,
                    provider_id,
                    ms_user,
                    identity_map.get(&ms_user.id).cloned(),
                    &mut stats,
                    &access_token,
                    &client,
                )
                .await
                {
                    Ok(_) => {
                        trace!(
                            "Successfully processed user: {}",
                            ms_user.user_principal_name
                        );
                    }
                    Err(error) => {
                        stats.record_failure(
                            crate::services::msgraph::EntityKind::Users,
                            &ms_user.id,
                            error.into(),
                            1,
                        );
                    }
                }
            }

            // Update progress more frequently
            if processed_count % 5 == 0 || processed_count == total_users {
                let _processed = stats.new_users_created
                    + stats.existing_users_updated
                    + stats.identities_linked;
                update_sync_progress_with_offset(
                    session_id,
                    "users",
                    processed_count,
                    total_users,
                    "running",
                    &format!(
                        "Processed {}/{} users ({} created, {} updated, {} linked, {} errors)",
                        processed_count,
                        total_users,
                        stats.new_users_created,
                        stats.existing_users_updated,
                        stats.identities_linked,
                        stats.errors.len()
                    ),
                    completed_items,
                );
            }
        }

        // Small delay between batches to be respectful to the API and database
        if processed_count < total_users {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    let processed =
        stats.new_users_created + stats.existing_users_updated + stats.identities_linked;

    // Only mark as completed if the sync wasn't cancelled
    if !sync_was_cancelled {
        update_sync_progress_with_offset(
            session_id,
            "users",
            total_users,
            total_users,
            "completed",
            &format!(
                "Completed: {} created, {} updated, {} linked, {} errors",
                stats.new_users_created,
                stats.existing_users_updated,
                stats.identities_linked,
                stats.errors.len()
            ),
            completed_items,
        );

        // Background photo sync will be handled at the main sync level if configured

        SyncProgress {
            entity: "users".to_string(),
            processed,
            total: total_users,
            status: if stats.errors.is_empty() {
                "completed".to_string()
            } else {
                "completed_with_errors".to_string()
            },
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
async fn fetch_microsoft_graph_users_optimized(
    _provider_id: i32,
) -> Result<(Vec<MicrosoftGraphUser>, String), String> {
    let (client, access_token) = get_msgraph_client_and_token().await?;

    // Build the Microsoft Graph API request for users
    // Select fields for MicrosoftGraphUser struct
    // Important: Include proxyAddresses and otherMails for email aliases, and accountEnabled for filtering
    let select_fields = "id,displayName,givenName,surname,mail,userPrincipalName,jobTitle,department,officeLocation,mobilePhone,businessPhones,companyName,streetAddress,city,state,postalCode,country,proxyAddresses,otherMails,accountEnabled";

    // Skip disabled accounts by default for performance
    let skip_disabled_accounts = std::env::var("MSGRAPH_SKIP_DISABLED_ACCOUNTS")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
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
            .map_err(|e| {
                format!("Failed to send Microsoft Graph request (page {page_count}): {e}")
            })?;

        let status = graph_response.status();
        let response_data: serde_json::Value = graph_response.json().await.map_err(|e| {
            format!("Failed to parse Microsoft Graph response (page {page_count}): {e}")
        })?;

        if !status.is_success() {
            let error_msg = response_data
                .get("error")
                .and_then(|err| err.get("message"))
                .and_then(|msg| msg.as_str())
                .unwrap_or("Unknown Microsoft Graph error");
            return Err(format!(
                "Microsoft Graph API error (page {page_count}, {status}): {error_msg}"
            ));
        }

        // Parse the users from this page
        let users_array = response_data
            .get("value")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                format!("Microsoft Graph response missing 'value' array (page {page_count})")
            })?;

        let mut page_users = Vec::new();
        for user_value in users_array {
            match serde_json::from_value::<MicrosoftGraphUser>(user_value.clone()) {
                Ok(user) => {
                    page_users.push(user);
                }
                Err(e) => {
                    warn!(page = page_count, error = %e, data = %user_value, "Failed to parse user from Microsoft Graph");
                    // Continue processing other users even if one fails to parse
                }
            }
        }

        debug!(
            "Page {}: Parsed {} users from Microsoft Graph",
            page_count,
            page_users.len()
        );
        all_users.extend(page_users);

        // Check if there's a next page
        if let Some(next_link) = response_data
            .get("@odata.nextLink")
            .and_then(|link| link.as_str())
        {
            url = next_link.to_string();
            trace!(
                "Found next page link, continuing to page {}...",
                page_count + 1
            );
        } else {
            debug!("No more pages found, finished pagination");
            break;
        }

        // Add a small delay between requests to be respectful to the API
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    info!(
        "Successfully fetched {} users from Microsoft Graph across {} pages",
        all_users.len(),
        page_count
    );

    // Log sample users at debug level
    if !all_users.is_empty() && log::log_enabled!(log::Level::Debug) {
        debug!("Sample users fetched: {} total", all_users.len().min(5));
        for (i, user) in all_users.iter().take(5).enumerate() {
            debug!(
                "  {}: {} ({})",
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
    let select_fields = "id,displayName,givenName,surname,mail,userPrincipalName,jobTitle,department,officeLocation,mobilePhone,businessPhones,companyName,streetAddress,city,state,postalCode,country,proxyAddresses,otherMails,accountEnabled";

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
        if let Err(e) =
            crate::repository::sync_history::delete_delta_token(conn, "microsoft", "users")
        {
            if !matches!(e, diesel::result::Error::NotFound) {
                warn!(error = %e, "Failed to clear delta token");
            }
        }
        None
    };

    // Build initial URL
    let skip_disabled_accounts = std::env::var("MSGRAPH_SKIP_DISABLED_ACCOUNTS")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
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
            .map_err(|e| {
                format!("Failed to send Microsoft Graph delta request (page {page_count}): {e}")
            })?;

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

        let response_data: serde_json::Value = graph_response.json().await.map_err(|e| {
            format!("Failed to parse Microsoft Graph delta response (page {page_count}): {e}")
        })?;

        if !status.is_success() {
            let error_msg = response_data
                .get("error")
                .and_then(|err| err.get("message"))
                .and_then(|msg| msg.as_str())
                .unwrap_or("Unknown Microsoft Graph error");
            return Err(format!(
                "Microsoft Graph API error (page {page_count}, {status}): {error_msg}"
            ));
        }

        // Parse users from this page
        let users_array = response_data
            .get("value")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                format!("Microsoft Graph delta response missing 'value' array (page {page_count})")
            })?;

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

        debug!(
            "Delta page {}: {} users, {} removed",
            page_count,
            all_users.len(),
            removed_user_ids.len()
        );

        // Check for deltaLink (end of changes) or nextLink (more pages)
        if let Some(delta_link) = response_data
            .get("@odata.deltaLink")
            .and_then(|v| v.as_str())
        {
            new_delta_link = Some(delta_link.to_string());
            debug!("Received deltaLink, finished fetching changes");
            break;
        } else if let Some(next_link) = response_data
            .get("@odata.nextLink")
            .and_then(|v| v.as_str())
        {
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
        match crate::repository::sync_history::upsert_delta_token(
            conn,
            "microsoft",
            "users",
            delta_link,
        ) {
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

/// Process a single Microsoft Graph user with optimized HTTP client (v2)
async fn process_microsoft_user_optimized_v2(
    conn: &mut DbConnection,
    provider_id: i32,
    ms_user: &MicrosoftGraphUser,
    existing_identity: Option<UserAuthIdentity>,
    stats: &mut UserSyncStats,
    access_token: &str,
    client: &reqwest::Client,
) -> Result<(), crate::services::msgraph::MsGraphSyncError> {
    // Step 1: Microsoft identity already resolved for this page's batch.
    if let Some(existing_identity) = existing_identity {
        // User already has Microsoft identity - update existing user and identity
        return update_existing_microsoft_user_optimized(
            conn,
            ms_user,
            existing_identity,
            stats,
            access_token,
            client,
        )
        .await;
    }

    // Step 2: Extract all email addresses from Microsoft Graph user
    let emails = extract_user_emails(ms_user);
    let email_addresses: Vec<String> = emails.iter().map(|(email, _, _)| email.clone()).collect();

    // Step 3: Check if any user exists with any of these email addresses
    if let Ok(Some(existing_user)) =
        user_emails_repo::find_user_by_any_of_emails(conn, &email_addresses)
    {
        // Local user exists but no Microsoft identity - link them
        return link_existing_user_to_microsoft_optimized(
            conn,
            provider_id,
            ms_user,
            existing_user,
            stats,
            access_token,
            client,
        )
        .await;
    }

    // Step 4: No existing user found - create new user with Microsoft identity
    create_new_user_from_microsoft_optimized(
        conn,
        provider_id,
        ms_user,
        stats,
        access_token,
        client,
    )
    .await
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

/// Update existing user who already has Microsoft identity (optimized version)
/// Map a Graph user onto the directory-contact shape the contact repo applies
/// (job title / company / department / office + work/mobile phones + the work
/// address). Trims and drops empties.
fn build_directory_contact(ms: &MicrosoftGraphUser) -> crate::models::DirectoryContact {
    use crate::models::{DirectoryAddress, DirectoryContact};
    let clean = |s: &Option<String>| {
        s.as_ref()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };

    let mut phones: Vec<(String, String)> = Vec::new();
    if let Some(bp) = &ms.business_phones {
        for p in bp {
            let p = p.trim();
            if !p.is_empty() {
                phones.push((p.to_string(), "work".to_string()));
            }
        }
    }
    if let Some(m) = clean(&ms.mobile_phone) {
        phones.push((m, "mobile".to_string()));
    }

    let street = clean(&ms.street_address);
    let city = clean(&ms.city);
    let region = clean(&ms.state);
    let postal_code = clean(&ms.postal_code);
    let country = clean(&ms.country);
    let address = (street.is_some()
        || city.is_some()
        || region.is_some()
        || postal_code.is_some()
        || country.is_some())
    .then_some(DirectoryAddress {
        street,
        city,
        region,
        postal_code,
        country,
    });

    DirectoryContact {
        job_title: clean(&ms.job_title),
        organization: clean(&ms.company_name),
        department: clean(&ms.department),
        office_location: clean(&ms.office_location),
        phones,
        address,
    }
}

/// Apply the directory-imported contact fields onto a synced user. Shared by
/// the create + update sync paths so the four call sites can't drift.
fn surface_contact(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    ms_user: &MicrosoftGraphUser,
) -> diesel::QueryResult<()> {
    crate::repository::user_contact::apply_directory_contact(
        conn,
        user_uuid,
        "microsoft",
        &build_directory_contact(ms_user),
        None,
    )
}

#[instrument(level = "debug", skip(conn, ms_user, stats, access_token, client), fields(user_uuid = %existing_identity.user_uuid))]
async fn update_existing_microsoft_user_optimized(
    conn: &mut DbConnection,
    ms_user: &MicrosoftGraphUser,
    existing_identity: UserAuthIdentity,
    stats: &mut UserSyncStats,
    access_token: &str,
    client: &reqwest::Client,
) -> Result<(), crate::services::msgraph::MsGraphSyncError> {
    use crate::services::msgraph::MsGraphSyncError;

    // User info is in the span context

    // Get the associated user. diesel::Error has a typed From impl
    // that routes NotFound / unique-violation / FK-violation through
    // DbConflict and infra errors through DbInfra; `?` does the
    // right classification without an intermediate format!.
    let user = user_repo::get_user_by_uuid(&existing_identity.user_uuid, conn)?;

    // Extract all email addresses from Microsoft Graph
    let emails = extract_user_emails(ms_user);
    let _primary_email = emails
        .first()
        .map(|(email, _, _)| email.clone())
        .unwrap_or_else(|| ms_user.user_principal_name.clone());

    // Update user information with latest from Microsoft Graph
    let updated_name = ms_user.display_name.as_ref().unwrap_or(&user.name);

    // Only update core fields, preserve role/pronouns/avatars, but update timestamp and Microsoft UUID
    let user_update = crate::models::UserUpdate {
        name: if updated_name != &user.name {
            Some(updated_name.clone())
        } else {
            None
        },

        pronouns: None,     // Preserve pronouns
        avatar_url: None,   // Preserve avatar
        banner_url: None,   // Preserve banner
        avatar_thumb: None, // Preserve avatar thumb
        microsoft_uuid: Some(utils::parse_uuid(&ms_user.id).map_err(|_| {
            MsGraphSyncError::Mapping {
                hint: "invalid Microsoft UUID format",
                source: None,
            }
        })?),
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    // Update user if there are changes
    if user_update.name.is_some() || user_update.microsoft_uuid.is_some() {
        user_repo::update_user(&user.uuid, user_update, conn, None)?;

        // Surface directory contact fields (read-only on the manual side).
        surface_contact(conn, user.uuid, ms_user)?;
        debug!(user_name = %user.name, "Updated user information");
    }

    // Store all email addresses
    let email_data: Vec<(String, String, bool, String)> = emails
        .into_iter()
        .map(|(email, email_type, verified)| (email, email_type, verified, "microsoft".to_string()))
        .collect();

    if !email_data.is_empty() {
        debug!(
            "Storing {} email addresses for user: {}",
            email_data.len(),
            user.name
        );

        match user_emails_repo::add_multiple_emails(conn, &user.uuid, email_data.clone()) {
            Ok(stored_emails) => {
                let added_count = stored_emails.len();
                debug!(
                    "Successfully stored {} email addresses for user: {}",
                    added_count, user.name
                );

                // Clean up any Microsoft emails that are no longer present
                let current_emails: Vec<String> = email_data
                    .iter()
                    .map(|(email, _, _, _)| email.clone())
                    .collect();
                match user_emails_repo::cleanup_obsolete_emails(
                    conn,
                    &user.uuid,
                    &current_emails,
                    "microsoft",
                ) {
                    Ok(cleaned_count) => {
                        if cleaned_count > 0 {
                            debug!(
                                "Cleaned up {} obsolete Microsoft email addresses for user: {}",
                                cleaned_count, user.name
                            );
                        }
                    }
                    Err(e) => {
                        warn!(
                            "Failed to cleanup obsolete emails for user {}: {}",
                            user.name, e
                        );
                    }
                }
            }
            Err(e) => {
                stats.record_failure(
                    crate::services::msgraph::EntityKind::Users,
                    &ms_user.id,
                    e.into(),
                    1,
                );
            }
        }
    } else {
        trace!("No email addresses to store for user: {}", user.name);
    }

    // Update identity data with latest from Microsoft Graph. The
    // From impls on MsGraphSyncError route serde_json::Error to
    // Parse (Permanent) and diesel::Error to DbConflict / DbInfra
    // so `?` carries the classification through without an
    // intermediate `format!`.
    let identity_data = serde_json::to_value(ms_user)?;
    update_identity_data(conn, existing_identity.id, Some(identity_data))?;

    // Sync profile photo using optimized client
    if let Ok(photo_urls) = sync_user_profile_photo(
        client,
        access_token,
        ms_user,
        &utils::uuid_to_string(&user.uuid),
    )
    .await
    {
        if let Err(e) = update_user_avatar_by_id(
            conn,
            &user.uuid,
            photo_urls.avatar_url,
            photo_urls.avatar_thumb,
        )
        .await
        {
            warn!(user_name = %user.name, error = %e, "Failed to update avatar for user");
        }
    }

    stats.existing_users_updated += 1;
    Ok(())
}

/// Link existing local user to Microsoft identity (optimized version)
#[instrument(level = "debug", skip(conn, ms_user, stats, access_token, client, existing_user), fields(user_uuid = %existing_user.uuid, provider_id = provider_id))]
async fn link_existing_user_to_microsoft_optimized(
    conn: &mut DbConnection,
    provider_id: i32,
    ms_user: &MicrosoftGraphUser,
    existing_user: User,
    stats: &mut UserSyncStats,
    access_token: &str,
    client: &reqwest::Client,
) -> Result<(), crate::services::msgraph::MsGraphSyncError> {
    use crate::services::msgraph::MsGraphSyncError;

    // Create Microsoft identity for existing user. The typed From
    // impls on serde_json::Error and diesel::result::Error mean
    // these `?` calls classify themselves at the source — no
    // intermediate `format!` stringification.
    let identity_data = serde_json::to_value(ms_user)?;

    let new_identity = NewUserAuthIdentity {
        user_uuid: existing_user.uuid,
        provider_type: "microsoft".to_string(),
        external_id: ms_user.id.clone(),
        email: ms_user.mail.clone(),
        metadata: Some(identity_data),
        password_hash: None,
        workspace_id: None,
    };

    identity_repo::create_identity(new_identity, conn)?;

    // Extract all email addresses from Microsoft Graph
    let emails = extract_user_emails(ms_user);
    let _primary_email = emails
        .first()
        .map(|(email, _, _)| email.clone())
        .unwrap_or_else(|| ms_user.user_principal_name.clone());

    // Update user information with Microsoft data and store Microsoft UUID
    let updated_name = ms_user.display_name.as_ref().unwrap_or(&existing_user.name);
    let user_update = crate::models::UserUpdate {
        name: if updated_name != &existing_user.name {
            Some(updated_name.clone())
        } else {
            None
        },

        pronouns: None,
        avatar_url: None,
        banner_url: None,
        avatar_thumb: None,
        microsoft_uuid: Some(utils::parse_uuid(&ms_user.id).map_err(|_| {
            MsGraphSyncError::Mapping {
                hint: "invalid Microsoft UUID format",
                source: None,
            }
        })?),
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    // Always update to store the Microsoft UUID
    user_repo::update_user(&existing_user.uuid, user_update, conn, None)?;

    // Store all email addresses
    let email_data: Vec<(String, String, bool, String)> = emails
        .into_iter()
        .map(|(email, email_type, verified)| (email, email_type, verified, "microsoft".to_string()))
        .collect();

    if !email_data.is_empty() {
        debug!(
            "Storing {} email addresses for linked user: {}",
            email_data.len(),
            existing_user.name
        );

        match user_emails_repo::add_multiple_emails(conn, &existing_user.uuid, email_data.clone()) {
            Ok(stored_emails) => {
                let added_count = stored_emails.len();
                debug!(
                    "Successfully stored {} email addresses for linked user: {}",
                    added_count, existing_user.name
                );
            }
            Err(e) => {
                stats.record_failure(
                    crate::services::msgraph::EntityKind::Users,
                    &ms_user.id,
                    e.into(),
                    1,
                );
            }
        }
    } else {
        trace!(
            "No email addresses to store for linked user: {}",
            existing_user.name
        );
    }

    // Sync profile photo using optimized client
    if let Ok(photo_urls) = sync_user_profile_photo(
        client,
        access_token,
        ms_user,
        &utils::uuid_to_string(&existing_user.uuid),
    )
    .await
    {
        if let Err(e) = update_user_avatar_by_id(
            conn,
            &existing_user.uuid,
            photo_urls.avatar_url,
            photo_urls.avatar_thumb,
        )
        .await
        {
            warn!(
                "Failed to update avatar for user {}: {}",
                existing_user.name, e
            );
        }
    }

    stats.identities_linked += 1;
    Ok(())
}

/// Create new user from Microsoft Graph data (optimized version)
#[instrument(level = "debug", skip(conn, ms_user, stats, access_token, client), fields(provider_id = provider_id))]
async fn create_new_user_from_microsoft_optimized(
    conn: &mut DbConnection,
    provider_id: i32,
    ms_user: &MicrosoftGraphUser,
    stats: &mut UserSyncStats,
    access_token: &str,
    client: &reqwest::Client,
) -> Result<(), crate::services::msgraph::MsGraphSyncError> {
    use crate::services::msgraph::MsGraphSyncError;

    // Generate UUID for new user (this is our local UUID, different from Microsoft's)
    let _user_uuid = Uuid::now_v7().to_string();

    // Extract all email addresses from Microsoft Graph
    let emails = extract_user_emails(ms_user);
    let primary_email = emails
        .first()
        .map(|(email, _, _)| email.clone())
        .unwrap_or_else(|| ms_user.user_principal_name.clone());

    // Determine name (prefer displayName, fallback to givenName + surname, fallback to userPrincipalName)
    let name = ms_user
        .display_name
        .clone()
        .or_else(|| match (&ms_user.given_name, &ms_user.surname) {
            (Some(first), Some(last)) => Some(format!("{first} {last}")),
            (Some(first), None) => Some(first.clone()),
            (None, Some(last)) => Some(last.clone()),
            _ => None,
        })
        .unwrap_or_else(|| ms_user.user_principal_name.clone());

    // Create new user with default role 'user' and store Microsoft UUID
    let user_uuid = Uuid::now_v7();
    let microsoft_uuid =
        Some(
            utils::parse_uuid(&ms_user.id).map_err(|_| MsGraphSyncError::Mapping {
                hint: "invalid Microsoft UUID format",
                source: None,
            })?,
        );
    let new_user = utils::NewUserBuilder::microsoft_user(
        name.clone(),
        primary_email.clone(),
        crate::models::PlatformRole::User,
        microsoft_uuid,
    )
    .with_uuid(user_uuid)
    .build();

    // diesel::result::Error has a typed From impl on
    // MsGraphSyncError that routes unique-violation / FK to
    // DbConflict and infrastructural failures to DbInfra; `?`
    // classifies at the source.
    let created_user = user_repo::create_user(new_user, conn)?;

    // Surface directory contact fields onto the new user.
    surface_contact(conn, created_user.uuid, ms_user)?;

    // Store all email addresses
    let email_data: Vec<(String, String, bool, String)> = emails
        .into_iter()
        .map(|(email, email_type, verified)| (email, email_type, verified, "microsoft".to_string()))
        .collect();

    if !email_data.is_empty() {
        debug!(
            "Storing {} email addresses for new user: {}",
            email_data.len(),
            name
        );

        if let Err(e) =
            user_emails_repo::add_multiple_emails(conn, &created_user.uuid, email_data.clone())
        {
            stats.record_failure(
                crate::services::msgraph::EntityKind::Users,
                &ms_user.id,
                e.into(),
                1,
            );
        }
    } else {
        trace!("No email addresses to store for new user: {}", name);
    }

    // Create Microsoft identity for the new user
    let identity_data = serde_json::to_value(ms_user)?;

    let new_identity = NewUserAuthIdentity {
        user_uuid: created_user.uuid,
        provider_type: "microsoft".to_string(),
        external_id: ms_user.id.clone(),
        email: ms_user.mail.clone(),
        metadata: Some(identity_data),
        password_hash: None,
        workspace_id: None,
    };

    identity_repo::create_identity(new_identity, conn)?;

    // Sync profile photo using optimized client
    if let Ok(photo_urls) = sync_user_profile_photo(
        client,
        access_token,
        ms_user,
        &utils::uuid_to_string(&user_uuid),
    )
    .await
    {
        if let Err(e) = update_user_avatar_by_id(
            conn,
            &created_user.uuid,
            photo_urls.avatar_url,
            photo_urls.avatar_thumb,
        )
        .await
        {
            warn!("Failed to update avatar for user {}: {}", name, e);
        }
    }

    info!(
        "Created new user: {} with {} email addresses",
        name,
        email_data.len()
    );
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
    completed_items: usize,
) -> SyncProgress {
    let mut stats = DeviceSyncStats {
        new_devices_created: 0,
        existing_devices_updated: 0,
        devices_assigned: 0,
        errors: Vec::new(),
        failures: Vec::new(),
    };

    let sync_mode_msg = if use_delta { "delta" } else { "full" };
    update_sync_progress_with_type_and_offset(
        session_id,
        "devices",
        0,
        0,
        "running",
        &format!("Fetching devices from Microsoft Graph ({sync_mode_msg})"),
        "devices",
        None,
        completed_items,
    );

    // Step 1: Fetch devices from Microsoft Graph using delta query
    let delta_result = match fetch_microsoft_graph_devices_delta(conn, use_delta).await {
        Ok(result) => result,
        Err(error) => {
            update_sync_progress_with_type_and_offset(
                session_id,
                "devices",
                0,
                0,
                "error",
                &format!("Failed to fetch devices: {error}"),
                "devices",
                None,
                completed_items,
            );
            return SyncProgress {
                entity: "devices".to_string(),
                processed: 0,
                total: 0,
                status: "error".to_string(),
                errors: vec![format!(
                    "Failed to fetch Microsoft Graph devices: {}",
                    error
                )],
            };
        }
    };

    let entra_devices = delta_result.devices;
    let removed_device_ids = delta_result.removed_device_ids;
    let _access_token = delta_result.access_token;

    // Handle removed devices first (if delta sync returned any).
    //
    // Design choice: warn-log the removal but keep the device row.
    // Devices have no auth implication (unlike users) — the only
    // hazard of a stale device row is inventory drift, not access.
    // Operators reviewing the warning can choose to delete the
    // device manually via the admin UI. This matches the
    // "preserve history, surface the signal" stance used for user
    // removals above.
    if !removed_device_ids.is_empty() {
        info!(
            count = removed_device_ids.len(),
            "Processing removed devices from delta response"
        );
        for removed_id in &removed_device_ids {
            if let Ok(device) = asset_repo::get_device_by_entra_id(conn, removed_id) {
                warn!(
                    device_id = %device.id,
                    device_name = %device.name,
                    entra_id = %removed_id,
                    "Entra ID reported device removed: kept device row (no auth impact); delete manually via admin if desired",
                );
            }
        }
    }

    let total_devices = entra_devices.len();
    info!(
        device_count = total_devices,
        was_delta = !delta_result.was_full_sync,
        "Fetched devices from Entra ID"
    );

    if total_devices == 0 {
        update_sync_progress_with_type_and_offset(
            session_id,
            "devices",
            0,
            0,
            "completed",
            "No devices found to sync",
            "devices",
            None,
            completed_items,
        );
        return SyncProgress {
            entity: "devices".to_string(),
            processed: 0,
            total: 0,
            status: "completed".to_string(),
            errors: Vec::new(),
        };
    }

    // Note: No need to resolve Entra Object IDs - the /devices endpoint already returns them as the `id` field
    update_sync_progress_with_type_and_offset(
        session_id,
        "devices",
        0,
        total_devices,
        "running",
        &format!("Processing {total_devices} Entra devices"),
        "devices",
        None,
        completed_items,
    );

    // Step 2: Process Entra devices. Resolve every device's existing local row
    // up front in two batched queries (by Entra Object ID, then by Azure AD
    // device ID for the fallback), instead of one or two lookups per device. A
    // lookup failure degrades to "not found" -> create, matching the previous
    // per-device `.ok()` behaviour.
    let (existing_by_entra, existing_by_ms) = {
        let entra_ids: Vec<&str> = entra_devices.iter().map(|d| d.id.as_str()).collect();
        let ms_ids: Vec<&str> = entra_devices
            .iter()
            .filter_map(|d| d.device_id.as_deref())
            .collect();
        (
            asset_repo::get_devices_by_entra_ids_full(conn, &entra_ids).unwrap_or_default(),
            asset_repo::get_devices_by_microsoft_ids_full(conn, &ms_ids).unwrap_or_default(),
        )
    };

    let mut processed_count = 0;

    for entra_device in entra_devices {
        // Check for cancellation before processing each device
        if is_sync_cancelled(session_id) {
            let processed = stats.new_devices_created + stats.existing_devices_updated;
            let cancel_message = format!("Sync was cancelled by user request. Processed {} of {} devices ({} created, {} updated)",
                processed_count, total_devices, stats.new_devices_created, stats.existing_devices_updated);

            update_sync_progress_with_type_and_offset(
                session_id,
                "devices",
                processed_count,
                total_devices,
                "cancelled",
                &cancel_message,
                "devices",
                None,
                completed_items,
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

        let device_name = entra_device
            .display_name
            .as_deref()
            .unwrap_or(&entra_device.id);

        update_sync_progress_with_type_and_offset(
            session_id,
            "devices",
            processed_count - 1,
            total_devices,
            "running",
            &format!("Processing device: {device_name}"),
            "devices",
            None,
            completed_items,
        );

        let existing_device = existing_by_entra
            .get(&entra_device.id)
            .cloned()
            .or_else(|| {
                entra_device
                    .device_id
                    .as_ref()
                    .and_then(|mid| existing_by_ms.get(mid).cloned())
            });

        match process_entra_device(
            conn,
            provider_id,
            &entra_device,
            existing_device,
            &mut stats,
        )
        .await
        {
            Ok(_) => {
                debug!(device_name = %device_name, "Successfully processed Entra device");
            }
            Err(error) => {
                stats.record_failure(
                    crate::services::msgraph::EntityKind::Devices,
                    &entra_device.id,
                    error.into(),
                    1,
                );
            }
        }

        // Update progress more frequently
        if processed_count % 5 == 0 || processed_count == total_devices {
            let _processed = stats.new_devices_created + stats.existing_devices_updated;
            update_sync_progress_with_type_and_offset(
                session_id,
                "devices",
                processed_count,
                total_devices,
                "running",
                &format!(
                    "Processed {}/{} devices ({} created, {} updated, {} assigned, {} errors)",
                    processed_count,
                    total_devices,
                    stats.new_devices_created,
                    stats.existing_devices_updated,
                    stats.devices_assigned,
                    stats.errors.len()
                ),
                "devices",
                None,
                completed_items,
            );
        }

        // Small delay between devices to be respectful to the database
        if processed_count < total_devices {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    let processed = stats.new_devices_created + stats.existing_devices_updated;

    update_sync_progress_with_type_and_offset(
        session_id,
        "devices",
        total_devices,
        total_devices,
        "completed",
        &format!(
            "Completed: {} created, {} updated, {} assigned, {} errors",
            stats.new_devices_created,
            stats.existing_devices_updated,
            stats.devices_assigned,
            stats.errors.len()
        ),
        "devices",
        None,
        completed_items,
    );

    SyncProgress {
        entity: "devices".to_string(),
        processed,
        total: total_devices,
        status: if stats.errors.is_empty() {
            "completed".to_string()
        } else {
            "completed_with_errors".to_string()
        },
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
    completed_items: usize,
) -> SyncProgress {
    let mut stats = GroupSyncStats::default();

    let sync_mode_msg = if use_delta { "delta" } else { "full" };
    update_sync_progress_with_type_and_offset(
        session_id,
        "groups",
        0,
        0,
        "running",
        &format!("Fetching groups from Microsoft Graph ({sync_mode_msg})"),
        "groups",
        None,
        completed_items,
    );

    // Load sync configuration
    let config = GroupSyncConfig::from_env();

    // Step 1: Fetch groups from Microsoft Graph using delta query
    let delta_result = match fetch_microsoft_graph_groups_delta(conn, use_delta).await {
        Ok(result) => result,
        Err(error) => {
            update_sync_progress_with_type_and_offset(
                session_id,
                "groups",
                0,
                0,
                "error",
                &format!("Failed to fetch groups: {error}"),
                "groups",
                None,
                completed_items,
            );
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
    let groups_to_sync: Vec<_> = delta_result
        .groups
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
        update_sync_progress_with_type_and_offset(
            session_id,
            "groups",
            0,
            0,
            "completed",
            "No groups found to sync (check filter settings)",
            "groups",
            None,
            completed_items,
        );
        return SyncProgress {
            entity: "groups".to_string(),
            processed: 0,
            total: 0,
            status: "completed".to_string(),
            errors: Vec::new(),
        };
    }

    update_sync_progress_with_type_and_offset(
        session_id,
        "groups",
        0,
        total_groups,
        "running",
        &format!("Processing {total_groups} groups"),
        "groups",
        None,
        completed_items,
    );

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
            update_sync_progress_with_type_and_offset(
                session_id,
                "groups",
                processed_count,
                total_groups,
                "cancelled",
                &cancel_message,
                "groups",
                None,
                completed_items,
            );
            return SyncProgress {
                entity: "groups".to_string(),
                processed: processed_count,
                total: total_groups,
                status: "cancelled".to_string(),
                errors: stats.errors,
            };
        }

        processed_count += 1;

        // Handle removed groups.
        //
        // Design choice: warn-log and skip. Groups in Nosdesk are
        // membership pivots (visibility scopes, assignment pools);
        // removing the row would cascade-orphan memberships and
        // could break ticket visibility for users who weren't
        // themselves removed from Entra ID. Keeping the row
        // preserves those memberships until an operator decides
        // to clean up manually via the admin UI. Same "preserve
        // history, surface the signal" stance as users + devices.
        if group_item.is_removed {
            warn!(
                group_id = %group_item.group_id,
                "Entra ID reported group removed: kept group row to preserve memberships; delete manually via admin if desired",
            );
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

        update_sync_progress_with_type_and_offset(
            session_id,
            "groups",
            processed_count - 1,
            total_groups,
            "running",
            &format!("Processing group: {group_name}"),
            "groups",
            None,
            completed_items,
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
                if !was_full_sync
                    && (!group_item.members_added.is_empty()
                        || !group_item.members_removed.is_empty())
                {
                    // Delta sync: apply incremental membership changes
                    match apply_delta_group_membership(
                        conn,
                        group.id,
                        &group_item.members_added,
                        &group_item.members_removed,
                    )
                    .await
                    {
                        Ok(changes) => {
                            stats.user_membership_changes += changes;
                            if changes > 0 {
                                debug!(group_name = %group_name, changes = changes, "Applied delta membership changes");
                            }
                        }
                        Err(e) => {
                            stats.record_failure(
                                crate::services::msgraph::EntityKind::Groups,
                                &ms_group.id,
                                e.into(),
                                1,
                            );
                        }
                    }
                } else {
                    // Full sync or no delta changes: fetch all members
                    // Sync user group membership
                    match sync_group_membership(
                        conn,
                        &client,
                        &access_token,
                        &ms_group.id,
                        group.id,
                    )
                    .await
                    {
                        Ok(changes) => {
                            stats.user_membership_changes += changes;
                            if changes > 0 {
                                debug!(group_name = %group_name, changes = changes, "Synced user membership changes");
                            }
                        }
                        Err(e) => {
                            stats.record_failure(
                                crate::services::msgraph::EntityKind::Groups,
                                &ms_group.id,
                                e.into(),
                                1,
                            );
                        }
                    }

                    // Sync device group membership
                    match sync_device_group_membership(
                        conn,
                        &client,
                        &access_token,
                        &ms_group.id,
                        group.id,
                    )
                    .await
                    {
                        Ok(changes) => {
                            stats.device_membership_changes += changes;
                            if changes > 0 {
                                debug!(group_name = %group_name, changes = changes, "Synced device membership changes");
                            }
                        }
                        Err(e) => {
                            stats.record_failure(
                                crate::services::msgraph::EntityKind::Groups,
                                &ms_group.id,
                                e.into(),
                                1,
                            );
                        }
                    }
                }
            }
            Err(e) => {
                stats.record_failure(
                    crate::services::msgraph::EntityKind::Groups,
                    &ms_group.id,
                    e.into(),
                    1,
                );
            }
        }

        // Progress update every 5 groups
        if processed_count % 5 == 0 || processed_count == total_groups {
            update_sync_progress_with_type_and_offset(
                session_id,
                "groups",
                processed_count,
                total_groups,
                "running",
                &format!(
                    "Processed {}/{} groups ({} created, {} updated, {} errors)",
                    processed_count,
                    total_groups,
                    stats.groups_created,
                    stats.groups_updated,
                    stats.errors.len()
                ),
                "groups",
                None,
                completed_items,
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
        stats.groups_created,
        stats.groups_updated,
        stats.user_membership_changes,
        stats.device_membership_changes,
        stats.errors.len()
    );

    update_sync_progress_with_type_and_offset(
        session_id,
        "groups",
        total_groups,
        total_groups,
        "completed",
        &final_message,
        "groups",
        None,
        completed_items,
    );

    SyncProgress {
        entity: "groups".to_string(),
        processed,
        total: total_groups,
        status: if stats.errors.is_empty() {
            "completed".to_string()
        } else {
            "completed_with_errors".to_string()
        },
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
    let select_fields =
        "id,displayName,description,mailEnabled,securityEnabled,groupTypes,mail,members";

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
        if let Err(e) =
            crate::repository::sync_history::delete_delta_token(conn, "microsoft", "groups")
        {
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
        debug!(
            "Fetching group delta page {} from Microsoft Graph",
            page_count
        );

        let graph_response = client
            .get(&url)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| {
                format!(
                    "Failed to send Microsoft Graph group delta request (page {page_count}): {e}"
                )
            })?;

        let status = graph_response.status();

        // Handle 410 Gone - delta token expired, need to do full sync
        if status == reqwest::StatusCode::GONE {
            warn!("Group delta token expired (410 Gone), falling back to full sync");

            // Clear the expired token
            let _ =
                crate::repository::sync_history::delete_delta_token(conn, "microsoft", "groups");

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

        let response_data: serde_json::Value = graph_response.json().await.map_err(|e| {
            format!("Failed to parse Microsoft Graph group delta response (page {page_count}): {e}")
        })?;

        if !status.is_success() {
            let error_msg = response_data
                .get("error")
                .and_then(|err| err.get("message"))
                .and_then(|msg| msg.as_str())
                .unwrap_or("Unknown Microsoft Graph error");
            return Err(format!(
                "Microsoft Graph API error (page {page_count}, {status}): {error_msg}"
            ));
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

            if let Some(members_delta) = group_value.get("members@delta").and_then(|v| v.as_array())
            {
                for member_value in members_delta {
                    // Check if this member was removed
                    if member_value.get("@removed").is_some() {
                        if let Some(member_id) = member_value.get("id").and_then(|v| v.as_str()) {
                            members_removed.push(member_id.to_string());
                        }
                    } else {
                        // Parse the member
                        if let Ok(member) = serde_json::from_value::<MicrosoftGraphGroupMember>(
                            member_value.clone(),
                        ) {
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
        if let Some(delta_link) = response_data
            .get("@odata.deltaLink")
            .and_then(|v| v.as_str())
        {
            new_delta_link = Some(delta_link.to_string());
            debug!("Received group deltaLink, finished fetching changes");
            break;
        } else if let Some(next_link) = response_data
            .get("@odata.nextLink")
            .and_then(|v| v.as_str())
        {
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
        match crate::repository::sync_history::upsert_delta_token(
            conn,
            "microsoft",
            "groups",
            delta_link,
        ) {
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
) -> Result<Vec<MicrosoftGraphGroupMember>, crate::services::msgraph::MsGraphSyncError> {
    use crate::services::msgraph::MsGraphSyncError;
    let mut all_members: Vec<MicrosoftGraphGroupMember> = Vec::new();
    let mut next_link: Option<String> = Some(format!(
        "https://graph.microsoft.com/v1.0/groups/{group_id}/members?$select=id,displayName,userPrincipalName&$top=999"
    ));

    while let Some(url) = next_link {
        // reqwest::Error -> MsGraphSyncError classifies the network
        // failure kind (timeout / connect / tls / body) via the From
        // impl; if the status reached the response, it routes to
        // HttpTransient (429 / 5xx) or HttpPermanent (everything
        // else) via the same impl.
        let response = client.get(&url).bearer_auth(access_token).send().await?;

        if !response.status().is_success() {
            // Decompose into status + body. `from_status` does the
            // transient/permanent split (429/5xx -> retryable,
            // 4xx -> skip) so the retry executor can act on it.
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(MsGraphSyncError::from_status(
                status,
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("graph members fetch body: {body}"),
                ),
            ));
        }

        let data: serde_json::Value = response.json().await?;

        if let Some(members) = data["value"].as_array() {
            for member_value in members {
                if let Ok(member) =
                    serde_json::from_value::<MicrosoftGraphGroupMember>(member_value.clone())
                {
                    // Include user and device members
                    if member.is_user() || member.is_device() {
                        all_members.push(member);
                    }
                }
            }
        }

        next_link = data["@odata.nextLink"].as_str().map(String::from);
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
) -> Result<usize, crate::services::msgraph::MsGraphSyncError> {
    use crate::services::msgraph::{with_retry, RetryConfig};
    use std::collections::HashSet;
    use tokio_util::sync::CancellationToken;

    // Fetch current members from Microsoft Graph, with retry on
    // transient HTTP failures. fetch_group_members's status
    // decomposition already classifies 429 / 5xx as HttpTransient
    // and 4xx as HttpPermanent, so the executor's classify-driven
    // retry policy does the right thing: pagination retries the
    // failing page (idempotent since each page request is independent
    // of the others' completion state), permanent failures bail out
    // immediately.
    //
    // Cancellation: a CancellationToken is required by the executor
    // signature. The scheduler's own shutdown token isn't currently
    // threaded into the sync flow (SYNC_CANCELLATION static + the
    // scheduler's shutdown live in separate registries today); a
    // follow-up will wire them together via a child token spawned
    // at the entry of `perform_sync`. Until then we pass a fresh
    // token that never fires — the inner sleeps are bounded by
    // RetryConfig::max_backoff so the retry chain can't outlive the
    // scheduler's 30-min tick.
    let cancel = CancellationToken::new();
    let retry_config = RetryConfig::default();
    let graph_members = with_retry("groups.members", retry_config, &cancel, || async {
        fetch_group_members(client, access_token, graph_group_id).await
    })
    .await
    .map_err(|f| f.error)?;

    // Filter to only user members (not devices or nested groups)
    let user_members: Vec<_> = graph_members.iter().filter(|m| m.is_user()).collect();

    // Get current local membership. diesel error routes to DbConflict
    // or DbInfra via the typed From impl.
    let current_local_members = groups_repo::get_member_uuids_for_group(conn, local_group_id)?;
    let current_local_set: HashSet<_> = current_local_members.into_iter().collect();

    // Map Graph user IDs to local user UUIDs
    let graph_member_ids: Vec<&str> = user_members.iter().map(|m| m.id.as_str()).collect();
    let user_mappings =
        identity_repo::get_user_uuids_by_external_ids(&graph_member_ids, "microsoft", conn)?;

    let new_member_uuids: HashSet<_> = user_mappings.into_iter().map(|(_, uuid)| uuid).collect();

    // Calculate additions and removals
    let to_add: Vec<_> = new_member_uuids
        .difference(&current_local_set)
        .cloned()
        .collect();
    let to_remove: Vec<_> = current_local_set
        .difference(&new_member_uuids)
        .cloned()
        .collect();

    let mut changes = 0;

    // Add new members
    for user_uuid in &to_add {
        if let Err(e) = groups_repo::add_user_to_group(conn, *user_uuid, local_group_id, None) {
            warn!(user_uuid = %user_uuid, group_id = local_group_id, error = %e, "Failed to add user to group");
        } else {
            changes += 1;
        }
    }

    // Remove old members (only for users synced from Microsoft - preserve
    // manually added users). Resolve which of `to_remove` carry a Microsoft
    // identity in one query rather than per user. A read failure yields an
    // empty set, so everyone is preserved (matches the old per-user fallback).
    let microsoft_synced =
        identity_repo::users_with_provider(conn, "microsoft", &to_remove).unwrap_or_default();
    for user_uuid in &to_remove {
        if microsoft_synced.contains(user_uuid) {
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
) -> Result<usize, crate::services::msgraph::MsGraphSyncError> {
    let mut changes = 0;

    // Process added members (users only for now)
    let user_members_added: Vec<_> = members_added.iter().filter(|m| m.is_user()).collect();
    if !user_members_added.is_empty() {
        let external_ids: Vec<&str> = user_members_added.iter().map(|m| m.id.as_str()).collect();
        let user_mappings =
            identity_repo::get_user_uuids_by_external_ids(&external_ids, "microsoft", conn)?;

        // Read current membership once; `add_user_to_group` is idempotent, so
        // this only avoids a redundant insert-txn per already-present member.
        let current_members: std::collections::HashSet<uuid::Uuid> =
            groups_repo::get_member_uuids_for_group(conn, local_group_id)
                .unwrap_or_default()
                .into_iter()
                .collect();

        for (external_id, user_uuid) in user_mappings {
            if current_members.contains(&user_uuid) {
                continue;
            }
            if let Err(e) = groups_repo::add_user_to_group(conn, user_uuid, local_group_id, None) {
                warn!(user_uuid = %user_uuid, group_id = local_group_id, external_id = %external_id, error = %e, "Failed to add user to group from delta");
            } else {
                debug!(user_uuid = %user_uuid, group_id = local_group_id, "Added user to group from delta");
                changes += 1;
            }
        }
    }

    // Process removed members
    if !members_removed.is_empty() {
        let external_ids_refs: Vec<&str> = members_removed.iter().map(|s| s.as_str()).collect();
        let user_mappings =
            identity_repo::get_user_uuids_by_external_ids(&external_ids_refs, "microsoft", conn)?;

        for (external_id, user_uuid) in user_mappings {
            if let Err(e) = groups_repo::remove_user_from_group(conn, &user_uuid, local_group_id) {
                warn!(user_uuid = %user_uuid, group_id = local_group_id, external_id = %external_id, error = %e, "Failed to remove user from group from delta");
            } else {
                debug!(user_uuid = %user_uuid, group_id = local_group_id, "Removed user from group from delta");
                changes += 1;
            }
        }
    }

    // Process added device members. Resolve every added device's Entra ID in
    // one query and read current membership once, instead of both per device.
    let device_members_added: Vec<_> = members_added.iter().filter(|m| m.is_device()).collect();
    if !device_members_added.is_empty() {
        let entra_ids: Vec<&str> = device_members_added.iter().map(|m| m.id.as_str()).collect();
        let device_mappings = asset_repo::get_devices_by_entra_ids(conn, &entra_ids)?;
        let current_devices: std::collections::HashSet<i32> =
            groups_repo::get_device_ids_for_group(conn, local_group_id)
                .unwrap_or_default()
                .into_iter()
                .collect();

        for (_entra_id, device_id) in device_mappings {
            if current_devices.contains(&device_id) {
                continue;
            }
            if let Err(e) = groups_repo::add_device_to_group(
                conn,
                device_id,
                local_group_id,
                None,
                Some("microsoft"),
            ) {
                warn!(device_id, group_id = local_group_id, error = %e, "Failed to add device to group from delta");
            } else {
                debug!(
                    device_id,
                    group_id = local_group_id,
                    "Added device to group from delta"
                );
                changes += 1;
            }
        }
    }

    // Process removed device members: resolve the removed Entra IDs to local
    // devices in one query (non-device IDs simply don't resolve).
    if !members_removed.is_empty() {
        let entra_ids: Vec<&str> = members_removed.iter().map(|s| s.as_str()).collect();
        let device_mappings = asset_repo::get_devices_by_entra_ids(conn, &entra_ids)?;
        for (_entra_id, device_id) in device_mappings {
            if let Err(e) = groups_repo::remove_device_from_group(conn, device_id, local_group_id) {
                warn!(device_id, group_id = local_group_id, error = %e, "Failed to remove device from group from delta");
            } else {
                debug!(
                    device_id,
                    group_id = local_group_id,
                    "Removed device from group from delta"
                );
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
) -> Result<usize, crate::services::msgraph::MsGraphSyncError> {
    use crate::services::msgraph::{with_retry, RetryConfig};
    use std::collections::HashSet;
    use tokio_util::sync::CancellationToken;

    // Retry on transient HTTP failures — see the analogous comment
    // in sync_group_membership for the cancellation gap.
    let cancel = CancellationToken::new();
    let graph_members = with_retry(
        "groups.device_members",
        RetryConfig::default(),
        &cancel,
        || async { fetch_group_members(client, access_token, graph_group_id).await },
    )
    .await
    .map_err(|f| f.error)?;

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
    let current_synced_devices =
        groups_repo::get_synced_device_ids_for_group(conn, local_group_id, "microsoft")?;
    let current_local_set: HashSet<_> = current_synced_devices.into_iter().collect();

    // Map Graph device IDs (Entra Object IDs) to local device IDs
    let graph_device_ids: Vec<&str> = device_members.iter().map(|m| m.id.as_str()).collect();

    debug!(
        group_id = graph_group_id,
        graph_device_ids = ?graph_device_ids,
        "Looking up local devices by Entra IDs"
    );

    let device_mappings = asset_repo::get_devices_by_entra_ids(conn, &graph_device_ids)?;

    debug!(
        group_id = graph_group_id,
        mapped_count = device_mappings.len(),
        "Mapped Graph device IDs to local device IDs"
    );

    let new_device_ids: HashSet<_> = device_mappings.into_iter().map(|(_, id)| id).collect();

    // Calculate additions and removals
    let to_add: Vec<_> = new_device_ids
        .difference(&current_local_set)
        .cloned()
        .collect();
    let to_remove: Vec<_> = current_local_set
        .difference(&new_device_ids)
        .cloned()
        .collect();

    let mut changes = 0;

    // Add new device members
    for device_id in &to_add {
        if let Err(e) = groups_repo::add_device_to_group(
            conn,
            *device_id,
            local_group_id,
            None,
            Some("microsoft"),
        ) {
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
    pub avatar_url: Option<String>,   // 120x120 or fallback
    pub avatar_thumb: Option<String>, // 48x48 thumbnail
}

/// Fetch and save user profile photo from Microsoft Graph (updated to download 120x120 and generate thumbnail)
async fn sync_user_profile_photo(
    client: &reqwest::Client,
    access_token: &str,
    user: &MicrosoftGraphUser,
    local_user_uuid: &str,
) -> Result<PhotoSyncUrls, String> {
    debug!(
        "Fetching profile photo for user: {}",
        user.user_principal_name
    );

    let mut avatar_url = None;
    let mut avatar_thumb = None;

    // Download 120x120 for profile views (main avatar) and generate thumbnail from it
    match download_profile_photo_size(client, access_token, user, local_user_uuid, "120x120").await
    {
        Ok(Some(url)) => {
            debug!(
                "Successfully downloaded 120x120 photo for user: {}",
                user.user_principal_name
            );
            avatar_url = Some(url.clone());

            // Generate 48x48 WebP thumbnail from the 120x120 image
            match crate::utils::generate_user_avatar_thumbnail(&url, local_user_uuid).await {
                Ok(Some(thumb_url)) => {
                    debug!(
                        "Successfully generated thumbnail for user: {}",
                        user.user_principal_name
                    );
                    avatar_thumb = Some(thumb_url);
                }
                Ok(None) => debug!(
                    "Failed to generate thumbnail for user: {}",
                    user.user_principal_name
                ),
                Err(e) => warn!(
                    "Error generating thumbnail for user {}: {}",
                    user.user_principal_name, e
                ),
            }
        }
        Ok(None) => trace!(
            "No 120x120 photo available for user: {}",
            user.user_principal_name
        ),
        Err(e) => debug!(
            "Failed to download 120x120 photo for user {}: {}",
            user.user_principal_name, e
        ),
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
                    }
                    Ok(None) => {
                        debug!(user_principal_name = %user.user_principal_name, "Failed to generate thumbnail from default photo")
                    }
                    Err(e) => {
                        warn!(user_principal_name = %user.user_principal_name, error = %e, "Error generating thumbnail from default photo")
                    }
                }
            }
            Ok(None) => {
                debug!(user_principal_name = %user.user_principal_name, "No default photo available")
            }
            Err(e) => {
                warn!(user_principal_name = %user.user_principal_name, error = %e, "Failed to download default photo")
            }
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

    let photo_url = format!(
        "https://graph.microsoft.com/v1.0/users/{}/photos/{}/$value",
        user.id, size
    );

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
            return Err(format!(
                "Failed to fetch {size} profile photo, status: {status}"
            ));
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
    trace!(
        user_uuid = local_user_uuid,
        size = size_label,
        "Processing Microsoft Graph profile photo"
    );

    // Use the shared image processing function to convert to WebP with size constraints
    let max_size = match size_label {
        "120x120" => 120,
        "default" => 200, // Default gets processed to 200px max
        _ => 200,         // Fallback to 200px
    };

    match crate::utils::image::process_avatar_image(photo_bytes, local_user_uuid, max_size).await {
        Ok(Some(avatar_url)) => {
            debug!(user_uuid = local_user_uuid, avatar_url = %avatar_url, "Successfully processed Microsoft Graph photo");
            Ok(Some(avatar_url))
        }
        Ok(None) => {
            debug!(
                user_uuid = local_user_uuid,
                "Failed to process Microsoft Graph photo"
            );
            Ok(None)
        }
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

            pronouns: None,
            avatar_url: avatar_url.clone(),
            banner_url: None,
            avatar_thumb: avatar_thumb.clone(),
            microsoft_uuid: None, // Don't update Microsoft UUID when updating avatar
            updated_at: Some(chrono::Utc::now().naive_utc()),
        };

        match user_repo::update_user(user_uuid, user_update, conn, None) {
            Ok(updated_user) => {
                debug!(user_uuid = %user_uuid, avatar_url = ?updated_user.avatar_url, avatar_thumb = ?updated_user.avatar_thumb, "Successfully updated avatar URLs");
            }
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
    let photo_url = format!(
        "https://graph.microsoft.com/v1.0/users/{}/photo/$value",
        user.id
    );

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
        if let Err(e) =
            crate::repository::sync_history::delete_delta_token(conn, "microsoft", "devices")
        {
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
        debug!(
            "Fetching device delta page {} from Microsoft Graph",
            page_count
        );

        let graph_response = client
            .get(&url)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| {
                format!(
                    "Failed to send Microsoft Graph device delta request (page {page_count}): {e}"
                )
            })?;

        let status = graph_response.status();

        // Handle 410 Gone - delta token expired, need to do full sync
        if status == reqwest::StatusCode::GONE {
            warn!("Device delta token expired (410 Gone), falling back to full sync");

            // Clear the expired token
            let _ =
                crate::repository::sync_history::delete_delta_token(conn, "microsoft", "devices");

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

        let response_data: serde_json::Value = graph_response.json().await.map_err(|e| {
            format!(
                "Failed to parse Microsoft Graph device delta response (page {page_count}): {e}"
            )
        })?;

        if !status.is_success() {
            let error_msg = response_data
                .get("error")
                .and_then(|err| err.get("message"))
                .and_then(|msg| msg.as_str())
                .unwrap_or("Unknown Microsoft Graph error");
            return Err(format!(
                "Microsoft Graph API error (page {page_count}, {status}): {error_msg}"
            ));
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

        debug!(
            "Entra device delta page {}: {} devices, {} removed",
            page_count,
            all_devices.len(),
            removed_device_ids.len()
        );

        // Check for deltaLink (end of changes) or nextLink (more pages)
        if let Some(delta_link) = response_data
            .get("@odata.deltaLink")
            .and_then(|v| v.as_str())
        {
            new_delta_link = Some(delta_link.to_string());
            debug!("Received device deltaLink, finished fetching changes");
            break;
        } else if let Some(next_link) = response_data
            .get("@odata.nextLink")
            .and_then(|v| v.as_str())
        {
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
        match crate::repository::sync_history::upsert_delta_token(
            conn,
            "microsoft",
            "devices",
            delta_link,
        ) {
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
    existing_device: Option<crate::models::Asset>,
    stats: &mut DeviceSyncStats,
) -> Result<(), crate::services::msgraph::MsGraphSyncError> {
    let device_name = entra_device
        .display_name
        .as_deref()
        .unwrap_or(&entra_device.id);
    debug!(device_name = %device_name, entra_id = %entra_device.id, "Processing Entra device");

    // `existing_device` was resolved by the caller from the batch's Entra-ID
    // and Microsoft-ID maps (previously two per-device lookups here).

    // Step 3: Prepare device data
    let device_display_name = entra_device
        .display_name
        .as_ref()
        .cloned()
        .unwrap_or_else(|| format!("Device-{}", entra_device.id));

    let hostname = device_display_name.clone();

    let model = entra_device
        .model
        .as_ref()
        .cloned()
        .unwrap_or_else(|| "Unknown Model".to_string());

    let manufacturer = entra_device
        .manufacturer
        .as_ref()
        .cloned()
        .unwrap_or_else(|| "Unknown Manufacturer".to_string());

    // Map compliance state from isCompliant boolean
    let compliance_state = entra_device
        .is_compliant
        .map(|c| if c { "compliant" } else { "noncompliant" }.to_string());

    // Parse registration date time
    let registration_date = parse_microsoft_datetime(&entra_device.registration_date_time);

    // Parse last sign in time for last_sync_time
    let last_sign_in = parse_microsoft_datetime(&entra_device.approximate_last_sign_in_date_time);

    // Build the IT attribute blob Entra owns. Pass B moved every
    // one of these from a top-level column into the per-row
    // attributes JSONB; the sync writes them through that path so
    // the rendering form (DynamicAttributeForm) reads back from
    // the same place.
    let mut entra_attrs = serde_json::Map::new();
    entra_attrs.insert(
        "hostname".to_string(),
        serde_json::Value::String(hostname.clone()),
    );
    entra_attrs.insert(
        "entra_device_id".to_string(),
        serde_json::Value::String(entra_device.id.clone()),
    );
    if let Some(ref ms_id) = entra_device.device_id {
        entra_attrs.insert(
            "microsoft_device_id".to_string(),
            serde_json::Value::String(ms_id.clone()),
        );
    }
    if let Some(ref c) = compliance_state {
        entra_attrs.insert(
            "compliance_state".to_string(),
            serde_json::Value::String(c.clone()),
        );
    }
    if let Some(t) = last_sign_in {
        entra_attrs.insert(
            "last_sync_time".to_string(),
            serde_json::Value::String(t.and_utc().to_rfc3339()),
        );
    }
    if let Some(ref os) = entra_device.operating_system {
        entra_attrs.insert(
            "operating_system".to_string(),
            serde_json::Value::String(os.clone()),
        );
    }
    if let Some(ref ov) = entra_device.operating_system_version {
        entra_attrs.insert(
            "os_version".to_string(),
            serde_json::Value::String(ov.clone()),
        );
    }
    if let Some(m) = entra_device.is_managed {
        entra_attrs.insert("is_managed".to_string(), serde_json::Value::Bool(m));
    }
    if let Some(t) = registration_date {
        entra_attrs.insert(
            "enrollment_date".to_string(),
            serde_json::Value::String(t.and_utc().to_rfc3339()),
        );
    }

    if let Some(existing) = existing_device {
        // Merge sync-owned keys on top of whatever the admin
        // may have hand-edited so we don't blow away non-Entra
        // attribute values that another integration could set.
        let mut merged = existing.attributes.as_object().cloned().unwrap_or_default();
        for (k, v) in entra_attrs {
            merged.insert(k, v);
        }
        let device_update = crate::models::AssetUpdate {
            name: Some(device_display_name.clone()),
            model: Some(model),
            manufacturer: Some(manufacturer),
            attributes: Some(serde_json::Value::Object(merged)),
            external_sync_source: Some(Some("entra".to_string())),
            updated_at: Some(chrono::Utc::now().naive_utc()),
            ..Default::default()
        };

        // diesel error classified at source by MsGraphSyncError::from.
        asset_repo::update_device(conn, existing.id, device_update)?;

        debug!(device_name = %device_display_name, "Updated existing Entra device");
        stats.existing_devices_updated += 1;
    } else {
        // Seed warranty_status='Unknown' on the new row so the
        // legacy IT-desk warranty buckets still classify it.
        entra_attrs.insert(
            "warranty_status".to_string(),
            serde_json::Value::String("Unknown".to_string()),
        );
        let new_device = crate::models::NewAsset {
            name: device_display_name.clone(),
            serial_number: None,
            model: Some(model),
            manufacturer: Some(manufacturer),
            primary_user_uuid: None,
            location: None,
            notes: None,
            purchase_date: None,
            asset_tag: None,
            kind: "device".to_string(),
            attributes: serde_json::Value::Object(entra_attrs),
            quantity: None,
            unit: None,
            external_sync_source: Some("entra".to_string()),
            low_stock_threshold: None,
        };

        asset_repo::create_device(conn, new_device)?;

        info!(device_name = %device_display_name, "Created new Entra device");
        stats.new_devices_created += 1;
    }

    Ok(())
}

/// Get Entra Object ID from Azure AD Asset ID
pub async fn get_entra_object_id(
    req: actix_web::HttpRequest,
    db_pool: web::Data<Pool>,
    path: web::Path<String>,
) -> impl Responder {
    let _conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    // Extract claims from cookie auth middleware
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Get Microsoft provider
    let provider = match get_default_microsoft_provider() {
        Ok(provider) => provider,
        Err(_) => return errors::bad_request("Microsoft auth provider not found"),
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
        Err(error) => errors::bad_request(format!("Failed to fetch Object ID: {}", error))
    }
}

/// Fetch Entra Object ID from Microsoft Graph using Azure AD Asset ID
async fn fetch_entra_object_id_from_graph(
    _provider_id: i32,
    azure_ad_device_id: &str,
) -> Result<String, String> {
    let (client, access_token) = get_msgraph_client_and_token().await?;

    // Query Microsoft Graph for the device using the Azure AD Asset ID
    // Filter by deviceId (Azure AD Asset ID) to get the Object ID (id field)
    let url = format!(
        "https://graph.microsoft.com/v1.0/devices?$filter=deviceId eq '{azure_ad_device_id}'&$select=id,deviceId"
    );

    debug!(azure_ad_device_id = %azure_ad_device_id, "Fetching Entra Object ID for Azure AD Asset ID");

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
        return Err(format!(
            "No device found with Azure AD Asset ID: {azure_ad_device_id}"
        ));
    }

    // Get the first (and should be only) device
    let device = &devices_array[0];
    let object_id = device
        .get("id")
        .and_then(|id| id.as_str())
        .ok_or_else(|| "Device Object ID not found in response".to_string())?;

    debug!(object_id = %object_id, azure_ad_device_id = %azure_ad_device_id, "Successfully found Object ID for Azure AD Asset ID");

    Ok(object_id.to_string())
}

/// Extract all email addresses from Microsoft Graph user data
fn extract_user_emails(ms_user: &MicrosoftGraphUser) -> Vec<(String, String, bool)> {
    let mut emails = Vec::new();

    debug!(
        "Extracting emails for user: {} (ID: {})",
        ms_user.user_principal_name, ms_user.id
    );

    // Primary email (mail field)
    if let Some(mail) = &ms_user.mail {
        if !mail.is_empty() && mail.contains('@') {
            emails.push((mail.clone(), "primary".to_string(), true));
            trace!("Added primary email: {}", mail);
        }
    }

    // User Principal Name (if different from mail)
    if !ms_user.user_principal_name.is_empty()
        && ms_user.user_principal_name.contains('@')
        && !emails
            .iter()
            .any(|(e, _, _)| e == &ms_user.user_principal_name)
    {
        let email_type = if emails.is_empty() {
            "primary".to_string()
        } else {
            "upn".to_string()
        };
        emails.push((
            ms_user.user_principal_name.clone(),
            email_type.clone(),
            true,
        ));
        trace!(
            "Added UPN email: {} (type: {})",
            ms_user.user_principal_name,
            email_type
        );
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
            if !email.is_empty()
                && email.contains('@')
                && !emails.iter().any(|(e, _, _)| e == email)
            {
                emails.push((email.clone(), "other".to_string(), true));
                trace!("Added other email: {}", email);
            } else {
                trace!("Skipped invalid or duplicate other email: {}", email);
            }
        }
    }

    // If no emails found, use the userPrincipalName as a fallback
    if emails.is_empty() && !ms_user.user_principal_name.is_empty() {
        emails.push((
            ms_user.user_principal_name.clone(),
            "primary".to_string(),
            true,
        ));
        debug!("Added fallback UPN email: {}", ms_user.user_principal_name);
    }

    debug!(
        "Extracted {} emails for user {}",
        emails.len(),
        ms_user.user_principal_name
    );

    emails
}

/// Extract email address from Exchange proxy address format
fn extract_smtp_address(proxy_address: &str) -> Option<String> {
    if let Some(rest) = proxy_address
        .strip_prefix("SMTP:")
        .or_else(|| proxy_address.strip_prefix("smtp:"))
    {
        Some(rest.to_string())
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
                        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok()
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
    existing_identity: Option<UserAuthIdentity>,
    stats: &mut UserSyncStats,
) -> Result<(), crate::services::msgraph::MsGraphSyncError> {
    // Microsoft identity already resolved for this page's batch. A failure to
    // resolve it is handled once, at batch-prefetch time (an infra error there
    // aborts the pass rather than being misread as "no identity").
    if let Some(existing_identity) = existing_identity {
        return update_existing_microsoft_user_no_photos(conn, ms_user, existing_identity, stats)
            .await;
    }

    // No identity: link to an existing user by email, else create.
    let emails = extract_user_emails(ms_user);
    if let Some(existing_user) = find_existing_user_by_emails(conn, &emails) {
        link_existing_user_to_microsoft_no_photos(conn, provider_id, ms_user, existing_user, stats)
            .await
    } else {
        create_new_user_from_microsoft_no_photos(conn, provider_id, ms_user, stats).await
    }
}

/// Background photo sync task (simplified)
async fn background_photo_sync_task(
    db_pool: web::Data<Pool>,
    provider_id: i32,
    session_id: String,
    access_token: String,
) -> Result<(), String> {
    info!(
        "Starting background photo sync for provider {}",
        provider_id
    );

    update_sync_progress_with_type(
        &session_id,
        "photos",
        0,
        0,
        "starting",
        "Finding users without profile photos...",
        "photos",
        None,
    );

    // Get database connection
    let mut conn = db_pool
        .get()
        .map_err(|e| format!("Database connection failed: {e}"))?;

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
            None,
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
                if let Err(e) = update_user_avatar_by_id(
                    &mut conn,
                    &user_uuid,
                    photo_urls.avatar_url,
                    photo_urls.avatar_thumb,
                )
                .await
                {
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
                None,
            );
        }
    }

    let final_message =
        format!("Background photo sync completed: {success_count}/{total_users} success");
    info!("{}", final_message);

    update_sync_progress_with_type(
        &session_id,
        "photos",
        total_users,
        total_users,
        "completed",
        &final_message,
        "photos",
        None,
    );

    Ok(())
}

/// Update existing Microsoft user without photos (simplified)
async fn update_existing_microsoft_user_no_photos(
    conn: &mut DbConnection,
    ms_user: &MicrosoftGraphUser,
    existing_identity: UserAuthIdentity,
    stats: &mut UserSyncStats,
) -> Result<(), crate::services::msgraph::MsGraphSyncError> {
    use crate::services::msgraph::MsGraphSyncError;

    let user = user_repo::get_user_by_uuid(&existing_identity.user_uuid, conn)?;

    let emails = extract_user_emails(ms_user);
    let _primary_email = emails
        .first()
        .map(|(email, _, _)| email.clone())
        .unwrap_or_else(|| ms_user.user_principal_name.clone());

    let updated_name = ms_user.display_name.as_ref().unwrap_or(&user.name);

    let user_update = crate::models::UserUpdate {
        name: if updated_name != &user.name {
            Some(updated_name.clone())
        } else {
            None
        },

        pronouns: None,
        avatar_url: None,
        banner_url: None,
        avatar_thumb: None,
        microsoft_uuid: Some(utils::parse_uuid(&ms_user.id).map_err(|_| {
            MsGraphSyncError::Mapping {
                hint: "invalid Microsoft UUID format",
                source: None,
            }
        })?),
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    if user_update.name.is_some() || user_update.microsoft_uuid.is_some() {
        user_repo::update_user(&user.uuid, user_update, conn, None)?;

        // Surface directory contact fields (read-only on the manual side).
        surface_contact(conn, user.uuid, ms_user)?;
    }

    if !emails.is_empty() {
        let email_data: Vec<(String, String, bool, String)> = emails
            .into_iter()
            .map(|(email, email_type, verified)| {
                (email, email_type, verified, "microsoft".to_string())
            })
            .collect();

        let _ = user_emails_repo::add_multiple_emails(conn, &user.uuid, email_data);
    }

    let identity_data = serde_json::to_value(ms_user)?;
    update_identity_data(conn, existing_identity.id, Some(identity_data))?;

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
) -> Result<(), crate::services::msgraph::MsGraphSyncError> {
    use crate::services::msgraph::MsGraphSyncError;

    let identity_data = serde_json::to_value(ms_user)?;

    let new_identity = NewUserAuthIdentity {
        user_uuid: existing_user.uuid,
        provider_type: "microsoft".to_string(),
        external_id: ms_user.id.clone(),
        email: ms_user.mail.clone(),
        metadata: Some(identity_data),
        password_hash: None,
        workspace_id: None,
    };

    identity_repo::create_identity(new_identity, conn)?;

    let user_update = crate::models::UserUpdate {
        name: None,

        pronouns: None,
        avatar_url: None,
        banner_url: None,
        avatar_thumb: None,
        microsoft_uuid: Some(utils::parse_uuid(&ms_user.id).map_err(|_| {
            MsGraphSyncError::Mapping {
                hint: "invalid Microsoft UUID format",
                source: None,
            }
        })?),
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    user_repo::update_user(&existing_user.uuid, user_update, conn, None)?;

    stats.identities_linked += 1;
    Ok(())
}

/// Create new user from Microsoft without photos (simplified)
async fn create_new_user_from_microsoft_no_photos(
    conn: &mut DbConnection,
    _provider_id: i32,
    ms_user: &MicrosoftGraphUser,
    stats: &mut UserSyncStats,
) -> Result<(), crate::services::msgraph::MsGraphSyncError> {
    use crate::services::msgraph::MsGraphSyncError;

    let user_uuid = Uuid::now_v7();

    let name = ms_user
        .display_name
        .clone()
        .or_else(|| match (&ms_user.given_name, &ms_user.surname) {
            (Some(first), Some(last)) => Some(format!("{first} {last}")),
            (Some(first), None) => Some(first.clone()),
            (None, Some(last)) => Some(last.clone()),
            _ => None,
        })
        .unwrap_or_else(|| ms_user.user_principal_name.clone());

    let primary_email = ms_user
        .mail
        .as_ref()
        .unwrap_or(&ms_user.user_principal_name)
        .clone();

    let microsoft_uuid =
        Some(
            utils::parse_uuid(&ms_user.id).map_err(|_| MsGraphSyncError::Mapping {
                hint: "invalid Microsoft UUID format",
                source: None,
            })?,
        );
    let new_user = utils::NewUserBuilder::microsoft_user(
        name.clone(),
        primary_email,
        crate::models::PlatformRole::User,
        microsoft_uuid,
    )
    .with_uuid(user_uuid)
    .build();

    let created_user = user_repo::create_user(new_user, conn)?;

    // Surface directory contact fields onto the new user.
    surface_contact(conn, created_user.uuid, ms_user)?;

    let identity_data = serde_json::to_value(ms_user)?;

    let new_identity = NewUserAuthIdentity {
        user_uuid: created_user.uuid,
        provider_type: "microsoft".to_string(),
        external_id: ms_user.id.clone(),
        email: ms_user.mail.clone(),
        metadata: Some(identity_data),
        password_hash: None,
        workspace_id: None,
    };

    identity_repo::create_identity(new_identity, conn)?;

    let emails = extract_user_emails(ms_user);
    if !emails.is_empty() {
        let email_data: Vec<(String, String, bool, String)> = emails
            .into_iter()
            .map(|(email, email_type, verified)| {
                (email, email_type, verified, "microsoft".to_string())
            })
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
    emails: &[(String, String, bool)],
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
    use crate::schema::{user_auth_identities, users};
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
    let photo_url =
        format!("https://graph.microsoft.com/v1.0/users/{ms_user_id}/photos/120x120/$value");

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
