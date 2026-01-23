use actix_web::{web, HttpResponse, HttpRequest, HttpMessage, Responder};
use serde_json::json;
use serde::Deserialize;
use tracing::error;
use urlencoding;

use crate::db::Pool;
// Auth providers are now configured via environment variables
use crate::config_utils;
use crate::models::AuthProvider;

// Helper functions for environment-based auth providers
fn get_default_microsoft_provider_id() -> Result<i32, diesel::result::Error> {
    // Using environment variables - return fixed ID for Microsoft
    if std::env::var("MICROSOFT_CLIENT_ID").is_ok() 
        && std::env::var("MICROSOFT_CLIENT_SECRET").is_ok() 
        && std::env::var("MICROSOFT_TENANT_ID").is_ok() {
        Ok(2) // Microsoft provider ID
    } else {
        Err(diesel::result::Error::NotFound)
    }
}

fn get_provider_by_id(provider_id: i32) -> Result<AuthProvider, diesel::result::Error> {
    match provider_id {
        1 => Ok(AuthProvider::new(1, "Local".to_string(), "local".to_string(), true, true)),
        2 => {
            if std::env::var("MICROSOFT_CLIENT_ID").is_ok() 
                && std::env::var("MICROSOFT_CLIENT_SECRET").is_ok() 
                && std::env::var("MICROSOFT_TENANT_ID").is_ok() {
                Ok(AuthProvider::new(2, "Microsoft".to_string(), "microsoft".to_string(), true, false))
            } else {
                Err(diesel::result::Error::NotFound)
            }
        },
        _ => Err(diesel::result::Error::NotFound)
    }
}

// Structure for Microsoft Graph API requests
#[derive(Deserialize, Debug)]
pub struct MicrosoftGraphRequest {
    pub provider_id: Option<i32>,
    pub endpoint: String,
    pub method: Option<String>,
    pub body: Option<serde_json::Value>,
    pub query_params: Option<serde_json::Value>,
    pub headers: Option<serde_json::Value>,
}

/// Provides helpful permission guidance based on the endpoint being accessed
fn get_permission_help_message(endpoint: &str) -> serde_json::Value {
    // Determine the resource type from the endpoint
    let (resource_type, permissions) = if endpoint.contains("/users") || endpoint.starts_with("/users") {
        ("Users", json!({
            "required_permissions": ["User.Read.All"],
            "recommended_permissions": ["User.ReadWrite.All"],
            "description": "Read user profiles and basic directory information"
        }))
    } else if endpoint.contains("/devices") || endpoint.starts_with("/devices") {
        ("Devices", json!({
            "required_permissions": ["Device.Read.All"],
            "recommended_permissions": ["Device.ReadWrite.All"],
            "description": "Read device information from Intune and Azure AD"
        }))
    } else if endpoint.contains("/groups") || endpoint.starts_with("/groups") {
        ("Groups", json!({
            "required_permissions": ["Group.Read.All"],
            "recommended_permissions": ["Group.ReadWrite.All"],
            "description": "Read group information and memberships"
        }))
    } else if endpoint.contains("/directoryObjects") || endpoint.starts_with("/directoryObjects") {
        ("Directory Objects", json!({
            "required_permissions": ["Directory.Read.All"],
            "recommended_permissions": ["Directory.ReadWrite.All"],
            "description": "Read directory objects including users, groups, and devices"
        }))
    } else if endpoint.contains("/organization") || endpoint.starts_with("/organization") {
        ("Organization", json!({
            "required_permissions": ["Organization.Read.All"],
            "recommended_permissions": ["Organization.Read.All"],
            "description": "Read organization and tenant information"
        }))
    } else if endpoint.contains("/deviceManagement") || endpoint.starts_with("/deviceManagement") {
        ("Device Management (Intune)", json!({
            "required_permissions": ["DeviceManagementManagedDevices.Read.All"],
            "recommended_permissions": [
                "DeviceManagementManagedDevices.ReadWrite.All",
                "DeviceManagementConfiguration.Read.All"
            ],
            "description": "Read and manage devices enrolled in Microsoft Intune"
        }))
    } else {
        ("Microsoft Graph", json!({
            "required_permissions": ["Directory.Read.All"],
            "recommended_permissions": ["Directory.ReadWrite.All"],
            "description": "General directory read access"
        }))
    };

    json!({
        "message": format!("Your Azure AD application needs API permissions to access {} data", resource_type),
        "permissions": permissions,
        "setup_instructions": {
            "steps": [
                "1. Go to Azure Portal (portal.azure.com)",
                "2. Navigate to 'Azure Active Directory' → 'App registrations'",
                "3. Select your application",
                "4. Click 'API permissions' in the left menu",
                "5. Click 'Add a permission' → 'Microsoft Graph' → 'Application permissions'",
                "6. Search for and add the required permissions listed above",
                "7. Click 'Grant admin consent' (requires admin privileges)",
                "8. Wait a few minutes for permissions to propagate"
            ],
            "important_notes": [
                "Application permissions require admin consent",
                "Changes may take 5-10 minutes to take effect",
                "Ensure you're adding 'Application permissions', not 'Delegated permissions'"
            ]
        }
    })
}

// Microsoft Graph API request handler
pub async fn process_graph_request(
    db_pool: web::Data<Pool>,
    req: HttpRequest,
    request_data: web::Json<MicrosoftGraphRequest>,
) -> impl Responder {
    // Get database connection
    let conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
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

    // Get the provider_id from the request or use the default Microsoft provider
    let provider_id_val = match request_data.provider_id {
        Some(id) => id,
        None => {
            // Find the default Microsoft provider
            match get_default_microsoft_provider_id() {
                Ok(provider_id) => provider_id,
                Err(e) => {
                    if let diesel::result::Error::NotFound = e {
                        return HttpResponse::NotFound().json(json!({
                            "status": "error",
                            "message": "No Microsoft authentication provider configured"
                        }));
                    } else {
                        error!(error = ?e, "Error getting default Microsoft provider");
                        return HttpResponse::InternalServerError().json(json!({
                            "status": "error",
                            "message": "Failed to retrieve Microsoft provider"
                        }));
                    }
                }
            }
        }
    };

    // Get the provider
    let provider = match get_provider_by_id(provider_id_val) {
        Ok(p) => {
            if p.provider_type != "microsoft" {
                return HttpResponse::BadRequest().json(json!({
                    "status": "error",
                    "message": "This endpoint only supports Microsoft Graph API requests"
                }));
            }
            p
        },
        Err(e) => {
            if let diesel::result::Error::NotFound = e {
                return HttpResponse::NotFound().json(json!({
                    "status": "error",
                    "message": "Authentication provider not found"
                }));
            } else {
                error!(provider_id = provider_id_val, error = ?e, "Error getting auth provider");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Failed to retrieve authentication provider"
                }));
            }
        }
    };

    // Check if the provider is enabled
    if !provider.enabled {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "The Microsoft authentication provider is not enabled"
        }));
    }

    // Get required configuration values from environment variables
    let client_id = match config_utils::get_microsoft_client_id() {
        Ok(val) => val,
        Err(e) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": format!("Microsoft configuration error: {}", e)
        })),
    };

    let tenant_id = match config_utils::get_microsoft_tenant_id() {
        Ok(val) => val,
        Err(e) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": format!("Microsoft configuration error: {}", e)
        })),
    };

    let client_secret = match config_utils::get_microsoft_client_secret() {
        Ok(val) => val,
        Err(e) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": format!("Microsoft configuration error: {}", e)
        })),
    };

    // Get an access token
    let params = [
        ("client_id", client_id.as_str()),
        ("client_secret", client_secret.as_str()),
        ("grant_type", "client_credentials"),
        ("scope", "https://graph.microsoft.com/.default"),
    ];

    // Make the token request
    let client = reqwest::Client::new();
    let token_response = match client
        .post(format!("https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/token"))
        .form(&params)
        .send()
        .await
    {
        Ok(response) => {
            match response.json::<serde_json::Value>().await {
                Ok(token_data) => {
                    if token_data.get("access_token").is_none() {
                        return HttpResponse::BadRequest().json(json!({
                            "status": "error",
                            "message": "Failed to obtain access token",
                            "details": token_data.get("error_description").and_then(|v| v.as_str()).unwrap_or("Unknown error")
                        }));
                    }
                    token_data
                },
                Err(e) => {
                    error!(error = ?e, "Error parsing token response");
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Failed to parse Microsoft authentication response"
                    }));
                }
            }
        },
        Err(e) => {
            error!(error = ?e, "Error getting Microsoft access token");
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to get Microsoft access token"
            }));
        }
    };

    let access_token = token_response["access_token"].as_str().unwrap();

    // Build the Microsoft Graph API request
    let endpoint = &request_data.endpoint;
    let method = request_data.method.as_deref().unwrap_or("GET");
    
    // Build the URL with query parameters if provided
    let mut url = format!("https://graph.microsoft.com/v1.0{endpoint}");
    
    if let Some(query) = &request_data.query_params {
        if let Some(obj) = query.as_object() {
            let mut query_string = String::new();
            
            for (key, value) in obj {
                if !query_string.is_empty() {
                    query_string.push('&');
                } else {
                    query_string.push('?');
                }
                
                if let Some(str_val) = value.as_str() {
                    query_string.push_str(&format!("{}={}", urlencoding::encode(key), urlencoding::encode(str_val)));
                } else {
                    query_string.push_str(&format!("{}={}", urlencoding::encode(key), urlencoding::encode(&value.to_string())));
                }
            }
            
            url.push_str(&query_string);
        }
    }

    // Create the request based on the method
    let mut request_builder = match method {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        "PATCH" => client.patch(&url),
        _ => {
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": format!("Unsupported HTTP method: {}", method)
            }));
        }
    };

    // Add the authorization header
    request_builder = request_builder.header("Authorization", format!("Bearer {access_token}"));
    
    // Add custom headers if provided
    if let Some(headers) = &request_data.headers {
        if let Some(obj) = headers.as_object() {
            for (key, value) in obj {
                if let Some(str_val) = value.as_str() {
                    request_builder = request_builder.header(key, str_val);
                }
            }
        }
    }

    // Add the request body if provided for non-GET/DELETE methods
    if method != "GET" && method != "DELETE" {
        if let Some(body) = &request_data.body {
            request_builder = request_builder.json(body);
        }
    }

    // Execute the request
    match request_builder.send().await {
        Ok(response) => {
            let status = response.status();

            // Handle permission errors with helpful messages
            if status == 403 {
                match response.json::<serde_json::Value>().await {
                    Ok(error_data) => {
                        let error_msg = error_data
                            .get("error")
                            .and_then(|err| err.get("message"))
                            .and_then(|msg| msg.as_str())
                            .unwrap_or("Insufficient permissions");

                        let error_code = error_data
                            .get("error")
                            .and_then(|err| err.get("code"))
                            .and_then(|code| code.as_str())
                            .unwrap_or("Authorization_RequestDenied");

                        // Provide helpful permission guidance based on the endpoint
                        let permission_help = get_permission_help_message(endpoint);

                        return HttpResponse::Forbidden().json(json!({
                            "status": "error",
                            "message": error_msg,
                            "error_code": error_code,
                            "permission_help": permission_help,
                            "documentation": "https://learn.microsoft.com/en-us/graph/permissions-reference"
                        }));
                    },
                    Err(_) => {
                        return HttpResponse::Forbidden().json(json!({
                            "status": "error",
                            "message": "Insufficient permissions to access Microsoft Graph API",
                            "permission_help": get_permission_help_message(endpoint),
                            "documentation": "https://learn.microsoft.com/en-us/graph/permissions-reference"
                        }));
                    }
                }
            }

            // Parse response for other status codes
            match response.json::<serde_json::Value>().await {
                Ok(data) => {
                    if status.is_success() {
                        HttpResponse::build(status).json(json!({
                            "status": "success",
                            "data": data
                        }))
                    } else {
                        // Include error details for non-success responses
                        let error_msg = data
                            .get("error")
                            .and_then(|err| err.get("message"))
                            .and_then(|msg| msg.as_str())
                            .unwrap_or("Microsoft Graph API error");

                        HttpResponse::build(status).json(json!({
                            "status": "error",
                            "message": error_msg,
                            "data": data
                        }))
                    }
                },
                Err(e) => {
                    error!(error = ?e, "Error parsing Microsoft Graph response");
                    HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Failed to parse Microsoft Graph response"
                    }))
                }
            }
        },
        Err(e) => {
            error!(error = ?e, "Error sending Microsoft Graph request");
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to send Microsoft Graph request"
            }))
        }
    }
}

// Helper endpoints for common Graph API operations
pub async fn get_graph_users(
    db_pool: web::Data<Pool>,
    req: HttpRequest,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    // Convert the query parameters to what Microsoft Graph needs, only including non-empty values
    let mut query_params = serde_json::Map::new();
    
    if let Some(filter) = query.get("filter") {
        if !filter.is_empty() {
            query_params.insert("$filter".to_string(), serde_json::Value::String(filter.clone()));
        }
    }
    
    if let Some(select) = query.get("select") {
        if !select.is_empty() {
            query_params.insert("$select".to_string(), serde_json::Value::String(select.clone()));
        }
    }
    
    if let Some(top) = query.get("top") {
        if !top.is_empty() {
            query_params.insert("$top".to_string(), serde_json::Value::String(top.clone()));
        }
    }
    
    // Handle providerId separately as it's not a Graph API parameter
    let provider_id = query.get("providerId").and_then(|id| id.parse::<i32>().ok());

    // Create a Graph request
    let graph_request = MicrosoftGraphRequest {
        provider_id,
        endpoint: "/users".to_string(),
        method: Some("GET".to_string()),
        body: None,
        query_params: if query_params.is_empty() { None } else { Some(serde_json::Value::Object(query_params)) },
        headers: None,
    };

    // Process the request
    process_graph_request(db_pool, req, web::Json(graph_request)).await
}

pub async fn get_graph_devices(
    db_pool: web::Data<Pool>,
    req: HttpRequest,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    // Convert the query parameters to what Microsoft Graph needs, only including non-empty values
    let mut query_params = serde_json::Map::new();
    
    if let Some(filter) = query.get("filter") {
        if !filter.is_empty() {
            query_params.insert("$filter".to_string(), serde_json::Value::String(filter.clone()));
        }
    }
    
    if let Some(select) = query.get("select") {
        if !select.is_empty() {
            query_params.insert("$select".to_string(), serde_json::Value::String(select.clone()));
        }
    }
    
    if let Some(top) = query.get("top") {
        if !top.is_empty() {
            query_params.insert("$top".to_string(), serde_json::Value::String(top.clone()));
        }
    }
    
    // Handle providerId separately as it's not a Graph API parameter
    let provider_id = query.get("providerId").and_then(|id| id.parse::<i32>().ok());

    // Create a Graph request
    let graph_request = MicrosoftGraphRequest {
        provider_id,
        endpoint: "/devices".to_string(),
        method: Some("GET".to_string()),
        body: None,
        query_params: if query_params.is_empty() { None } else { Some(serde_json::Value::Object(query_params)) },
        headers: None,
    };

    // Process the request
    process_graph_request(db_pool, req, web::Json(graph_request)).await
}

pub async fn get_graph_groups(
    db_pool: web::Data<Pool>,
    req: HttpRequest,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    // Convert the query parameters to what Microsoft Graph needs, only including non-empty values
    let mut query_params = serde_json::Map::new();
    
    if let Some(filter) = query.get("filter") {
        if !filter.is_empty() {
            query_params.insert("$filter".to_string(), serde_json::Value::String(filter.clone()));
        }
    }
    
    if let Some(select) = query.get("select") {
        if !select.is_empty() {
            query_params.insert("$select".to_string(), serde_json::Value::String(select.clone()));
        }
    }
    
    if let Some(top) = query.get("top") {
        if !top.is_empty() {
            query_params.insert("$top".to_string(), serde_json::Value::String(top.clone()));
        }
    }
    
    // Handle providerId separately as it's not a Graph API parameter
    let provider_id = query.get("providerId").and_then(|id| id.parse::<i32>().ok());

    // Create a Graph request
    let graph_request = MicrosoftGraphRequest {
        provider_id,
        endpoint: "/groups".to_string(),
        method: Some("GET".to_string()),
        body: None,
        query_params: if query_params.is_empty() { None } else { Some(serde_json::Value::Object(query_params)) },
        headers: None,
    };

    // Process the request
    process_graph_request(db_pool, req, web::Json(graph_request)).await
}

pub async fn get_graph_directory_objects(
    db_pool: web::Data<Pool>,
    req: HttpRequest,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    // Convert the query parameters to what Microsoft Graph needs, only including non-empty values
    let mut query_params = serde_json::Map::new();
    
    if let Some(filter) = query.get("filter") {
        if !filter.is_empty() {
            query_params.insert("$filter".to_string(), serde_json::Value::String(filter.clone()));
        }
    }
    
    if let Some(select) = query.get("select") {
        if !select.is_empty() {
            query_params.insert("$select".to_string(), serde_json::Value::String(select.clone()));
        }
    }
    
    if let Some(top) = query.get("top") {
        if !top.is_empty() {
            query_params.insert("$top".to_string(), serde_json::Value::String(top.clone()));
        }
    }
    
    // Handle providerId separately as it's not a Graph API parameter
    let provider_id = query.get("providerId").and_then(|id| id.parse::<i32>().ok());

    // Create a Graph request for directory objects
    let graph_request = MicrosoftGraphRequest {
        provider_id,
        endpoint: "/directoryObjects".to_string(),
        method: Some("GET".to_string()),
        body: None,
        query_params: if query_params.is_empty() { None } else { Some(serde_json::Value::Object(query_params)) },
        headers: None,
    };

    // Process the request
    process_graph_request(db_pool, req, web::Json(graph_request)).await
} 