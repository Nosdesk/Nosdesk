/// Configuration utilities for the application
use std::env;
use uuid::Uuid;

/// Get JWT secret from environment
#[allow(dead_code)]
pub fn get_jwt_secret() -> String {
    env::var("JWT_SECRET").expect("JWT_SECRET environment variable must be set")
}

/// Get database URL from environment
#[allow(dead_code)]
pub fn get_database_url() -> String {
    env::var("DATABASE_URL").expect("DATABASE_URL environment variable must be set")
}

/// Get server configuration
#[allow(dead_code)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl ServerConfig {
    #[allow(dead_code)]
    pub fn from_env() -> Self {
        Self {
            host: env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
            port: env::var("PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()
                .expect("PORT must be a valid number"),
        }
    }
}

/// Generate a new UUID
#[allow(dead_code)]
pub fn generate_uuid() -> Uuid {
    Uuid::now_v7()
}

/// Validate environment variables are set
#[allow(dead_code)]
pub fn validate_env_vars() {
    let required_vars = ["JWT_SECRET", "DATABASE_URL"];
    
    for var in required_vars.iter() {
        if env::var(var).is_err() {
            panic!("Required environment variable {} is not set", var);
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Missing(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Missing(key) => write!(f, "Missing environment variable: {}", key),
        }
    }
}

// This allows ConfigError to be used with `?` in functions returning Result<_, Box<dyn std::error::Error>>
impl std::error::Error for ConfigError {}

// Helper to get an environment variable or return a ConfigError
fn get_env_var(name: &str) -> Result<String, ConfigError> {
    env::var(name).map_err(|_| ConfigError::Missing(name.to_string()))
}

pub fn get_microsoft_client_id() -> Result<String, ConfigError> {
    get_env_var("MICROSOFT_CLIENT_ID")
}

pub fn get_microsoft_tenant_id() -> Result<String, ConfigError> {
    get_env_var("MICROSOFT_TENANT_ID")
}

pub fn get_microsoft_client_secret() -> Result<String, ConfigError> {
    get_env_var("MICROSOFT_CLIENT_SECRET")
}

pub fn get_microsoft_redirect_uri() -> Result<String, ConfigError> {
    get_env_var("MICROSOFT_REDIRECT_URI")
} 