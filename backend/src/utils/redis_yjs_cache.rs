/// Redis cache for Yjs document state
///
/// This module provides a caching layer for Yjs documents to survive backend restarts
/// and prevent state vector mismatches. Documents are stored with a TTL and fall back
/// to PostgreSQL if Redis is unavailable.
use redis::{AsyncCommands, RedisError};
use std::sync::Arc;

/// TTL for cached documents (1 hour = 3600 seconds)
const DOCUMENT_TTL: usize = 3600;

/// Redis key prefix for Yjs documents
const KEY_PREFIX: &str = "yjs:doc";

/// Redis cache for Yjs document state
pub struct RedisYjsCache {
    client: redis::Client,
}

impl RedisYjsCache {
    /// Create a new Redis cache instance
    pub fn new(redis_url: &str) -> Result<Self, RedisError> {
        let client = redis::Client::open(redis_url)?;
        Ok(Self { client })
    }

    /// Generate Redis key for a document
    fn document_key(doc_id: &str) -> String {
        format!("{}:{}", KEY_PREFIX, doc_id)
    }

    /// Get document state from Redis
    /// Returns None if document not found or Redis unavailable
    pub async fn get_document(&self, doc_id: &str) -> Option<Vec<u8>> {
        let key = Self::document_key(doc_id);

        match self.client.get_multiplexed_async_connection().await {
            Ok(mut conn) => {
                match conn.get::<_, Vec<u8>>(&key).await {
                    Ok(data) => {
                        println!("✅ Redis cache HIT for document: {} ({} bytes)", doc_id, data.len());
                        Some(data)
                    }
                    Err(e) => {
                        println!("❌ Redis cache MISS for document {}: {:?}", doc_id, e);
                        None
                    }
                }
            }
            Err(e) => {
                println!("⚠️ Redis connection failed for document {}: {:?}", doc_id, e);
                None
            }
        }
    }

    /// Set document state in Redis with TTL
    pub async fn set_document(&self, doc_id: &str, data: &[u8]) {
        self.set_document_with_ttl(doc_id, data, DOCUMENT_TTL).await
    }

    /// Set document state in Redis with custom TTL
    pub async fn set_document_with_ttl(&self, doc_id: &str, data: &[u8], ttl: usize) {
        let key = Self::document_key(doc_id);

        match self.client.get_multiplexed_async_connection().await {
            Ok(mut conn) => {
                match conn.set_ex::<_, _, ()>(&key, data, ttl as u64).await {
                    Ok(_) => {
                        println!("💾 Redis cached document: {} ({} bytes, TTL: {}s)", doc_id, data.len(), ttl);
                    }
                    Err(e) => {
                        println!("⚠️ Failed to cache document {} in Redis: {:?}", doc_id, e);
                    }
                }
            }
            Err(e) => {
                println!("⚠️ Redis connection failed when caching document {}: {:?}", doc_id, e);
            }
        }
    }

    /// Delete document from Redis
    pub async fn delete_document(&self, doc_id: &str) {
        let key = Self::document_key(doc_id);

        match self.client.get_multiplexed_async_connection().await {
            Ok(mut conn) => {
                match conn.del::<_, ()>(&key).await {
                    Ok(_) => {
                        println!("🗑️ Deleted document from Redis cache: {}", doc_id);
                    }
                    Err(e) => {
                        println!("⚠️ Failed to delete document {} from Redis: {:?}", doc_id, e);
                    }
                }
            }
            Err(e) => {
                println!("⚠️ Redis connection failed when deleting document {}: {:?}", doc_id, e);
            }
        }
    }

    /// Update TTL for a document (refresh expiration)
    pub async fn refresh_ttl(&self, doc_id: &str) {
        self.refresh_ttl_with_duration(doc_id, DOCUMENT_TTL).await
    }

    /// Update TTL for a document with custom duration
    pub async fn refresh_ttl_with_duration(&self, doc_id: &str, ttl: usize) {
        let key = Self::document_key(doc_id);

        match self.client.get_multiplexed_async_connection().await {
            Ok(mut conn) => {
                match conn.expire::<_, ()>(&key, ttl as i64).await {
                    Ok(_) => {
                        println!("⏰ Refreshed TTL for document: {} ({}s)", doc_id, ttl);
                    }
                    Err(e) => {
                        println!("⚠️ Failed to refresh TTL for document {}: {:?}", doc_id, e);
                    }
                }
            }
            Err(e) => {
                println!("⚠️ Redis connection failed when refreshing TTL for document {}: {:?}", doc_id, e);
            }
        }
    }
}

/// Convenience function to create an Arc-wrapped cache instance
pub fn create_redis_cache(redis_url: &str) -> Result<Arc<RedisYjsCache>, RedisError> {
    RedisYjsCache::new(redis_url).map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_redis_key_format() {
        assert_eq!(RedisYjsCache::document_key("ticket-123"), "yjs:doc:ticket-123");
    }
}
