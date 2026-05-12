use uuid::Uuid;

/// Get Redis URL from environment with sensible default
pub fn get_redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string())
}

/// Generic rate limiting utility using Redis
/// This provides a reusable rate limiting implementation following DRY principles
pub struct RateLimiter;

#[derive(Debug)]
pub enum RateLimitError {
    RedisError(String),
    ConnectionFailed,
}

impl std::fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RedisError(msg) => write!(f, "Redis error: {msg}"),
            Self::ConnectionFailed => write!(f, "Failed to connect to Redis"),
        }
    }
}

impl std::error::Error for RateLimitError {}

impl RateLimiter {
    /// Check if a rate limit has been exceeded
    ///
    /// # Arguments
    /// * `redis_url` - Redis connection URL (e.g., "redis://localhost:6379")
    /// * `key` - Unique key for this rate limit (e.g., "mfa_attempts:user_uuid")
    /// * `max_attempts` - Maximum number of attempts allowed
    /// * `window_seconds` - Time window in seconds
    ///
    /// # Returns
    /// * `Ok(true)` - Request is allowed (under limit)
    /// * `Ok(false)` - Rate limit exceeded
    /// * `Err(_)` - Redis connection or other error
    pub async fn check_rate_limit(
        redis_url: &str,
        key: &str,
        max_attempts: u32,
        window_seconds: u64,
    ) -> Result<bool, RateLimitError> {
        // Get the current count
        let current_count = Self::get_attempt_count(redis_url, key).await?;

        // Check if under limit
        if current_count < max_attempts {
            // Increment the counter with TTL
            Self::increment_attempt(redis_url, key, window_seconds).await?;
            Ok(true)
        } else {
            // Rate limit exceeded
            tracing::warn!("Rate limit exceeded for key: {} ({}/{})", key, current_count, max_attempts);
            Ok(false)
        }
    }

    /// Get the current attempt count for a key
    ///
    /// # Arguments
    /// * `redis_url` - Redis connection URL
    /// * `key` - Unique key for this rate limit
    ///
    /// # Returns
    /// Current count (0 if key doesn't exist)
    pub async fn get_attempt_count(redis_url: &str, key: &str) -> Result<u32, RateLimitError> {
        use redis::AsyncCommands;

        let client = redis::Client::open(redis_url)
            .map_err(|e| RateLimitError::RedisError(e.to_string()))?;

        let mut con = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|_| RateLimitError::ConnectionFailed)?;

        let count: Option<u32> = con
            .get(key)
            .await
            .map_err(|e| RateLimitError::RedisError(e.to_string()))?;

        Ok(count.unwrap_or(0))
    }

    /// Increment the attempt counter for a key with automatic expiry
    ///
    /// # Arguments
    /// * `redis_url` - Redis connection URL
    /// * `key` - Unique key for this rate limit
    /// * `ttl_seconds` - Time to live in seconds (auto-expire)
    pub async fn increment_attempt(
        redis_url: &str,
        key: &str,
        ttl_seconds: u64,
    ) -> Result<(), RateLimitError> {
        

        let client = redis::Client::open(redis_url)
            .map_err(|e| RateLimitError::RedisError(e.to_string()))?;

        let mut con = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|_| RateLimitError::ConnectionFailed)?;

        // Use a Lua script for atomic increment + expire
        // This ensures the TTL is set atomically with the increment
        let script = r#"
            local current = redis.call('INCR', KEYS[1])
            if current == 1 then
                redis.call('EXPIRE', KEYS[1], ARGV[1])
            end
            return current
        "#;

        redis::Script::new(script)
            .key(key)
            .arg(ttl_seconds)
            .invoke_async::<_, ()>(&mut con)
            .await
            .map_err(|e| RateLimitError::RedisError(e.to_string()))?;

        Ok(())
    }


    /// Generate a standardized rate limit key for MFA attempts
    ///
    /// # Arguments
    /// * `user_uuid` - User's UUID
    ///
    /// # Returns
    /// Formatted Redis key for MFA rate limiting
    pub fn mfa_attempt_key(user_uuid: &Uuid) -> String {
        format!("mfa_attempts:{user_uuid}")
    }

    /// Generate a lockout key for login attempts, keyed on
    /// email-and-client-IP.
    ///
    /// AUD-013: keying on email alone let any attacker DoS a known
    /// user by deliberately failing logins until lockout fired.
    /// Including the client IP means the bad-actor IP locks itself
    /// out without affecting the legitimate user's IP. Email stays
    /// in the key so an attacker can't trivially rotate IPs across
    /// every targeted account.
    ///
    /// `client_ip` should be the trusted-proxy-resolved value from
    /// `utils::client_ip`; raw `peer_addr` is unsafe when a reverse
    /// proxy is in front. Callers that genuinely don't have a
    /// client IP (rare) pass `None`, which slots into a single
    /// `unknown` bucket per email so lockout still applies.
    pub fn login_attempt_key(email: &str, client_ip: Option<std::net::IpAddr>) -> String {
        let ip = client_ip
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        format!("login_attempts:{}:{ip}", email.to_lowercase())
    }

    /// Clear all attempts for a key (used on successful login)
    pub async fn clear_attempts(redis_url: &str, key: &str) -> Result<(), RateLimitError> {
        use redis::AsyncCommands;

        let client = redis::Client::open(redis_url)
            .map_err(|e| RateLimitError::RedisError(e.to_string()))?;

        let mut con = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|_| RateLimitError::ConnectionFailed)?;

        con.del::<_, ()>(key)
            .await
            .map_err(|e| RateLimitError::RedisError(e.to_string()))?;

        Ok(())
    }

    /// Check if an account is locked (exceeded max attempts)
    /// Returns remaining lockout time in seconds if locked, None if not locked
    pub async fn check_lockout(
        redis_url: &str,
        key: &str,
        max_attempts: u32,
    ) -> Result<Option<u64>, RateLimitError> {
        use redis::AsyncCommands;

        let client = redis::Client::open(redis_url)
            .map_err(|e| RateLimitError::RedisError(e.to_string()))?;

        let mut con = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|_| RateLimitError::ConnectionFailed)?;

        let count: Option<u32> = con
            .get(key)
            .await
            .map_err(|e| RateLimitError::RedisError(e.to_string()))?;

        if count.unwrap_or(0) >= max_attempts {
            // Get TTL to show remaining lockout time
            let ttl: i64 = con
                .ttl(key)
                .await
                .map_err(|e| RateLimitError::RedisError(e.to_string()))?;

            Ok(Some(if ttl > 0 { ttl as u64 } else { 0 }))
        } else {
            Ok(None)
        }
    }

    /// Record a failed login attempt
    pub async fn record_failed_attempt(
        redis_url: &str,
        key: &str,
        lockout_seconds: u64,
    ) -> Result<u32, RateLimitError> {
        

        let client = redis::Client::open(redis_url)
            .map_err(|e| RateLimitError::RedisError(e.to_string()))?;

        let mut con = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|_| RateLimitError::ConnectionFailed)?;

        // Atomic increment + set expiry
        let script = r#"
            local current = redis.call('INCR', KEYS[1])
            if current == 1 then
                redis.call('EXPIRE', KEYS[1], ARGV[1])
            end
            return current
        "#;

        let count: u32 = redis::Script::new(script)
            .key(key)
            .arg(lockout_seconds)
            .invoke_async(&mut con)
            .await
            .map_err(|e| RateLimitError::RedisError(e.to_string()))?;

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mfa_attempt_key_format() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let key = RateLimiter::mfa_attempt_key(&uuid);
        assert_eq!(key, "mfa_attempts:550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn login_attempt_key_includes_ip() {
        use std::net::IpAddr;
        let ip: IpAddr = "203.0.113.42".parse().unwrap();
        let key = RateLimiter::login_attempt_key("Alice@example.com", Some(ip));
        assert_eq!(key, "login_attempts:alice@example.com:203.0.113.42");
    }

    #[test]
    fn login_attempt_key_lowercases_email() {
        let key = RateLimiter::login_attempt_key("Alice@Example.COM", None);
        assert!(key.starts_with("login_attempts:alice@example.com:"));
    }

    #[test]
    fn login_attempt_key_unknown_ip_buckets_per_email() {
        // Two requests for the same email with no resolvable IP land
        // in the same bucket, so lockout still applies even when the
        // proxy gate is unconfigured.
        let a = RateLimiter::login_attempt_key("bob@example.com", None);
        let b = RateLimiter::login_attempt_key("bob@example.com", None);
        assert_eq!(a, b);
        assert!(a.ends_with(":unknown"));
    }

    #[test]
    fn login_attempt_key_different_ips_get_different_keys() {
        // The whole point of AUD-013: two attackers (or one attacker
        // rotating IPs) accumulate lockout state independently.
        // A user's legitimate IP isn't locked out by another IP's
        // failed attempts on the same email.
        use std::net::IpAddr;
        let attacker: IpAddr = "198.51.100.7".parse().unwrap();
        let legit_user: IpAddr = "203.0.113.1".parse().unwrap();
        let a = RateLimiter::login_attempt_key("alice@example.com", Some(attacker));
        let b = RateLimiter::login_attempt_key("alice@example.com", Some(legit_user));
        assert_ne!(a, b);
    }

    // Note: Integration tests requiring Redis would go in tests/ directory
}
