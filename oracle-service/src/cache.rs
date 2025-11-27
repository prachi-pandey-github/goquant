use anyhow::Result;
use redis::{Client, AsyncCommands};
use serde::{Serialize, Deserialize};
use std::time::Duration;
use tracing::{debug, error};

use crate::types::PriceData;

/// Redis-based price caching for ultra-fast price queries
pub struct PriceCache {
    client: Client,
    connection_pool: redis::aio::ConnectionManager,
    cache_ttl: u64, // Time-to-live in seconds
}

impl PriceCache {
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = Client::open(redis_url)?;
        let connection_pool = client.get_connection_manager().await?;
        
        Ok(Self {
            client,
            connection_pool,
            cache_ttl: 300, // 5 minutes default TTL
        })
    }
    
    /// Set price in cache with automatic expiration
    pub async fn set_price(&self, symbol: &str, price_data: &PriceData) -> Result<()> {
        let mut conn = self.connection_pool.clone();
        let key = format!("price:{}", symbol);
        let value = serde_json::to_string(price_data)?;
        
        // Set with TTL
        conn.set_ex::<_, _, ()>(&key, &value, self.cache_ttl).await?;
        
        // Also set in a sorted set for price history (optional)
        let history_key = format!("history:{}", symbol);
        let score = price_data.timestamp as f64;
        conn.zadd::<_, _, _, ()>(&history_key, &value, score).await?;
        
        // Keep only last 1000 entries in history
        conn.zremrangebyrank::<_, ()>(&history_key, 0, -1001).await?;
        
        debug!("Cached price for {} at ${}", symbol, self.format_price(price_data));
        Ok(())
    }
    
    /// Get price from cache
    pub async fn get_price(&self, symbol: &str) -> Result<Option<PriceData>> {
        let mut conn = self.connection_pool.clone();
        let key = format!("price:{}", symbol);
        
        let value: Option<String> = conn.get(&key).await?;
        
        match value {
            Some(json_str) => {
                let price_data: PriceData = serde_json::from_str(&json_str)?;
                debug!("Retrieved cached price for {}: ${}", symbol, self.format_price(&price_data));
                Ok(Some(price_data))
            },
            None => {
                debug!("No cached price found for {}", symbol);
                Ok(None)
            }
        }
    }
    
    /// Get price history for a symbol
    pub async fn get_price_history(&self, symbol: &str, limit: usize) -> Result<Vec<PriceData>> {
        let mut conn = self.connection_pool.clone();
        let history_key = format!("history:{}", symbol);
        
        // Get most recent entries
        let values: Vec<String> = conn.zrevrange(&history_key, 0, limit as isize - 1).await?;
        
        let mut history = Vec::new();
        for value in values {
            if let Ok(price_data) = serde_json::from_str::<PriceData>(&value) {
                history.push(price_data);
            }
        }
        
        Ok(history)
    }
    
    /// Set multiple prices in a batch operation
    pub async fn set_multiple_prices(&self, prices: &[(String, PriceData)]) -> Result<()> {
        let mut conn = self.connection_pool.clone();
        
        // Use pipeline for batch operations
        let mut pipe = redis::pipe();
        
        for (symbol, price_data) in prices {
            let key = format!("price:{}", symbol);
            let value = serde_json::to_string(price_data)?;
            pipe.set_ex(&key, &value, self.cache_ttl);
        }
        
        pipe.query_async::<_, ()>(&mut conn).await?;
        
        debug!("Batch cached {} prices", prices.len());
        Ok(())
    }
    
    /// Get multiple prices in a batch operation
    pub async fn get_multiple_prices(&self, symbols: &[String]) -> Result<Vec<Option<PriceData>>> {
        let mut conn = self.connection_pool.clone();
        
        let keys: Vec<String> = symbols.iter()
            .map(|symbol| format!("price:{}", symbol))
            .collect();
        
        let values: Vec<Option<String>> = conn.get(&keys).await?;
        
        let mut results = Vec::new();
        for value in values {
            match value {
                Some(json_str) => {
                    match serde_json::from_str::<PriceData>(&json_str) {
                        Ok(price_data) => results.push(Some(price_data)),
                        Err(_) => results.push(None),
                    }
                },
                None => results.push(None),
            }
        }
        
        Ok(results)
    }
    
    /// Publish price update to subscribers
    pub async fn publish_price_update(&self, symbol: &str, price_data: &PriceData) -> Result<()> {
        let mut conn = self.connection_pool.clone();
        let channel = format!("price_updates:{}", symbol);
        let message = serde_json::to_string(price_data)?;
        
        let subscriber_count: i32 = conn.publish(&channel, &message).await?;
        
        if subscriber_count > 0 {
            debug!("Published price update for {} to {} subscribers", symbol, subscriber_count);
        }
        
        Ok(())
    }
    
    /// Subscribe to price updates for a symbol
    pub async fn subscribe_to_price_updates(&self, symbols: Vec<String>) -> Result<redis::aio::PubSub> {
        let conn = self.client.get_async_connection().await?;
        let mut pubsub = conn.into_pubsub();
        
        for symbol in symbols {
            let channel = format!("price_updates:{}", symbol);
            pubsub.subscribe(&channel).await?;
        }
        
        Ok(pubsub)
    }
    
    /// Get cache statistics
    pub async fn get_stats(&self) -> Result<CacheStats> {
        let mut conn = self.connection_pool.clone();
        
        // Get basic Redis stats  
        let info: String = redis::cmd("INFO").arg("memory").query_async(&mut conn).await?;
        let keyspace: String = redis::cmd("INFO").arg("keyspace").query_async(&mut conn).await?;
        
        // Count price keys
        let price_keys: Vec<String> = conn.keys("price:*").await?;
        let history_keys: Vec<String> = conn.keys("history:*").await?;
        
        Ok(CacheStats {
            total_price_keys: price_keys.len(),
            total_history_keys: history_keys.len(),
            memory_usage: Self::parse_memory_usage(&info),
            redis_info: info,
            keyspace_info: keyspace,
        })
    }
    
    /// Clear cache for a specific symbol
    pub async fn clear_symbol(&self, symbol: &str) -> Result<()> {
        let mut conn = self.connection_pool.clone();
        
        let price_key = format!("price:{}", symbol);
        let history_key = format!("history:{}", symbol);
        
        conn.del::<_, ()>(&[price_key, history_key]).await?;
        
        debug!("Cleared cache for symbol: {}", symbol);
        Ok(())
    }
    
    /// Clear all cached data
    pub async fn clear_all(&self) -> Result<()> {
        let mut conn = self.connection_pool.clone();
        redis::cmd("FLUSHDB").query_async::<_, ()>(&mut conn).await?;
        
        debug!("Cleared all cached data");
        Ok(())
    }
    
    /// Health check for Redis connection
    pub async fn health_check(&self) -> bool {
        let mut conn = match self.connection_pool.clone() {
            conn => conn,
        };
        
        match redis::cmd("PING").query_async::<_, String>(&mut conn).await {
            Ok(_) => {
                debug!("Redis health check passed");
                true
            },
            Err(e) => {
                error!("Redis health check failed: {}", e);
                false
            }
        }
    }
    
    /// Format price for logging
    fn format_price(&self, price_data: &PriceData) -> String {
        let formatted_price = price_data.price as f64 / 10_f64.powi(-price_data.expo);
        format!("{:.2}", formatted_price)
    }
    
    /// Parse memory usage from Redis INFO command
    fn parse_memory_usage(info: &str) -> Option<u64> {
        for line in info.lines() {
            if line.starts_with("used_memory:") {
                if let Some(value) = line.split(':').nth(1) {
                    return value.parse().ok();
                }
            }
        }
        None
    }
}

/// Cache statistics structure
#[derive(Debug, Serialize, Deserialize)]
pub struct CacheStats {
    pub total_price_keys: usize,
    pub total_history_keys: usize,
    pub memory_usage: Option<u64>,
    pub redis_info: String,
    pub keyspace_info: String,
}

impl PriceData {
    /// Check if price data is fresh (not stale)
    pub fn is_fresh(&self, max_age: Duration) -> bool {
        let current_timestamp = chrono::Utc::now().timestamp();
        let age = current_timestamp - self.timestamp;
        age <= max_age.as_secs() as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PriceSource;
    
    async fn setup_test_cache() -> PriceCache {
        // Use Redis test instance or mock
        PriceCache::new("redis://127.0.0.1:6379/1").await
            .expect("Failed to connect to test Redis")
    }
    
    fn create_test_price_data() -> PriceData {
        PriceData {
            price: 50000_00000000,
            confidence: 500_00000,
            expo: -8,
            timestamp: chrono::Utc::now().timestamp(),
            source: PriceSource::Pyth,
            symbol: "BTC/USD".to_string(),
        }
    }
    
    #[tokio::test]
    async fn test_set_and_get_price() {
        let cache = setup_test_cache().await;
        let price_data = create_test_price_data();
        
        let result = cache.set_price("BTC/USD", &price_data).await;
        assert!(result.is_ok());
        
        let retrieved = cache.get_price("BTC/USD").await.unwrap();
        assert!(retrieved.is_some());
        
        let retrieved_data = retrieved.unwrap();
        assert_eq!(retrieved_data.price, price_data.price);
        assert_eq!(retrieved_data.symbol, price_data.symbol);
    }
    
    #[tokio::test]
    async fn test_price_freshness() {
        let fresh_price = PriceData {
            timestamp: chrono::Utc::now().timestamp(),
            ..create_test_price_data()
        };
        
        let stale_price = PriceData {
            timestamp: chrono::Utc::now().timestamp() - 3600, // 1 hour ago
            ..create_test_price_data()
        };
        
        assert!(fresh_price.is_fresh(Duration::from_secs(60)));
        assert!(!stale_price.is_fresh(Duration::from_secs(60)));
    }
}