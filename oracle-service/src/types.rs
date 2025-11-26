use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Price data structure used throughout the system
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PriceData {
    pub price: i64,           // Price in fixed-point notation
    pub confidence: u64,      // Confidence interval
    pub expo: i32,            // Exponent for decimal places
    pub timestamp: i64,       // Unix timestamp
    pub source: PriceSource,  // Source of the price data
    pub symbol: String,       // Trading symbol (e.g., "BTC/USD")
}

/// Price source enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PriceSource {
    Pyth,
    Switchboard,
    Aggregated,
    Internal,
}

/// Symbol configuration for oracle feeds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,                    // Symbol name (e.g., "BTC/USD")
    pub pyth_feed_id: String,           // Pyth price feed address
    pub switchboard_aggregator: String, // Switchboard aggregator address
    pub max_staleness: i64,             // Maximum age in seconds
    pub max_confidence: u64,            // Maximum confidence in basis points
    pub max_deviation: u64,             // Maximum deviation in basis points
}

/// Oracle health status tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleHealth {
    pub is_healthy: bool,
    pub last_update: i64,
    pub consecutive_failures: u32,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub average_latency: f64, // in milliseconds
    pub last_error: Option<String>,
}

impl Default for OracleHealth {
    fn default() -> Self {
        Self {
            is_healthy: true,
            last_update: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            consecutive_failures: 0,
            total_requests: 0,
            successful_requests: 0,
            average_latency: 0.0,
            last_error: None,
        }
    }
}

impl OracleHealth {
    pub fn update(&mut self, success: bool) {
        self.last_update = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
            
        self.total_requests += 1;
        
        if success {
            self.successful_requests += 1;
            self.consecutive_failures = 0;
            self.is_healthy = true;
            self.last_error = None;
        } else {
            self.consecutive_failures += 1;
            // Mark unhealthy after 3 consecutive failures
            if self.consecutive_failures >= 3 {
                self.is_healthy = false;
            }
        }
    }
    
    pub fn success_rate(&self) -> f64 {
        if self.total_requests == 0 {
            return 1.0;
        }
        self.successful_requests as f64 / self.total_requests as f64
    }
    
    pub fn update_latency(&mut self, latency_ms: f64) {
        // Simple exponential moving average
        if self.average_latency == 0.0 {
            self.average_latency = latency_ms;
        } else {
            self.average_latency = self.average_latency * 0.9 + latency_ms * 0.1;
        }
    }
    
    pub fn set_error(&mut self, error: String) {
        self.last_error = Some(error);
    }
}

/// API response structures
#[derive(Debug, Serialize, Deserialize)]
pub struct PriceResponse {
    pub symbol: String,
    pub price: f64,
    pub confidence: f64,
    pub timestamp: i64,
    pub source: PriceSource,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub overall_status: String,
    pub oracles: std::collections::HashMap<String, OracleHealthStatus>,
    pub cache_status: CacheHealthStatus,
    pub uptime: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OracleHealthStatus {
    pub is_healthy: bool,
    pub success_rate: f64,
    pub average_latency: f64,
    pub last_update: i64,
    pub consecutive_failures: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CacheHealthStatus {
    pub is_connected: bool,
    pub total_keys: usize,
    pub memory_usage: Option<u64>,
}

/// WebSocket message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    PriceUpdate {
        symbol: String,
        price: f64,
        confidence: f64,
        timestamp: i64,
        source: PriceSource,
    },
    HealthAlert {
        oracle: String,
        status: String,
        message: String,
        timestamp: i64,
    },
    Subscribe {
        symbols: Vec<String>,
    },
    Unsubscribe {
        symbols: Vec<String>,
    },
    Error {
        message: String,
    },
}

/// Configuration structure
#[derive(Debug, Deserialize)]
pub struct Config {
    pub solana: SolanaConfig,
    pub redis: RedisConfig,
    pub database: DatabaseConfig,
    pub server: ServerConfig,
    pub oracles: Vec<Symbol>,
}

#[derive(Debug, Deserialize)]
pub struct SolanaConfig {
    pub rpc_url: String,
    pub commitment: String,
}

#[derive(Debug, Deserialize)]
pub struct RedisConfig {
    pub url: String,
    pub pool_size: u32,
}

#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub cors_origins: Vec<String>,
}

/// Error types
#[derive(Debug, thiserror::Error)]
pub enum OracleError {
    #[error("Price not available: {0}")]
    PriceUnavailable(String),
    
    #[error("Stale price data: {0}")]
    StalePrice(String),
    
    #[error("Insufficient confidence: {0}")]
    InsufficientConfidence(String),
    
    #[error("Oracle connection failed: {0}")]
    ConnectionFailed(String),
    
    #[error("Cache error: {0}")]
    CacheError(String),
    
    #[error("Database error: {0}")]
    DatabaseError(String),
    
    #[error("Configuration error: {0}")]
    ConfigError(String),
    
    #[error("Validation error: {0}")]
    ValidationError(String),
}

/// Utility functions
impl PriceData {
    /// Convert price to decimal format
    pub fn to_decimal(&self) -> f64 {
        self.price as f64 / 10_f64.powi(-self.expo)
    }
    
    /// Convert confidence to decimal format
    pub fn confidence_to_decimal(&self) -> f64 {
        self.confidence as f64 / 10_f64.powi(-self.expo)
    }
    
    /// Calculate confidence as percentage of price
    pub fn confidence_percentage(&self) -> f64 {
        if self.price == 0 {
            return 100.0;
        }
        (self.confidence as f64 / self.price as f64) * 100.0
    }
    
    /// Check if price is within acceptable deviation from reference
    pub fn is_within_deviation(&self, reference_price: f64, max_deviation_bp: u64) -> bool {
        let current_price = self.to_decimal();
        let deviation = (current_price - reference_price).abs() / reference_price;
        let max_deviation = max_deviation_bp as f64 / 10000.0; // Convert basis points to decimal
        
        deviation <= max_deviation
    }
}

impl PriceResponse {
    pub fn from_price_data(price_data: &PriceData) -> Self {
        Self {
            symbol: price_data.symbol.clone(),
            price: price_data.to_decimal(),
            confidence: price_data.confidence_to_decimal(),
            timestamp: price_data.timestamp,
            source: price_data.source.clone(),
        }
    }
}

impl From<&OracleHealth> for OracleHealthStatus {
    fn from(health: &OracleHealth) -> Self {
        Self {
            is_healthy: health.is_healthy,
            success_rate: health.success_rate(),
            average_latency: health.average_latency,
            last_update: health.last_update,
            consecutive_failures: health.consecutive_failures,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_price_data_conversion() {
        let price_data = PriceData {
            price: 50000_00000000, // $50,000 with 8 decimals
            confidence: 500_00000, // $5 confidence
            expo: -8,
            timestamp: 1000000000,
            source: PriceSource::Pyth,
            symbol: "BTC/USD".to_string(),
        };
        
        assert_eq!(price_data.to_decimal(), 50000.0);
        assert_eq!(price_data.confidence_to_decimal(), 5.0);
        assert_eq!(price_data.confidence_percentage(), 0.01); // 0.01%
    }
    
    #[test]
    fn test_oracle_health_update() {
        let mut health = OracleHealth::default();
        
        // Test successful updates
        health.update(true);
        health.update(true);
        assert!(health.is_healthy);
        assert_eq!(health.success_rate(), 1.0);
        
        // Test failure updates
        health.update(false);
        health.update(false);
        health.update(false);
        assert!(!health.is_healthy);
        assert_eq!(health.consecutive_failures, 3);
    }
    
    #[test]
    fn test_deviation_check() {
        let price_data = PriceData {
            price: 50000_00000000,
            confidence: 500_00000,
            expo: -8,
            timestamp: 1000000000,
            source: PriceSource::Pyth,
            symbol: "BTC/USD".to_string(),
        };
        
        // Test within 1% deviation (100 basis points)
        assert!(price_data.is_within_deviation(50500.0, 100)); // 1% = 100 bp
        assert!(!price_data.is_within_deviation(51000.0, 100)); // 2% > 100 bp
    }
}