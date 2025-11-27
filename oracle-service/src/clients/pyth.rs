use anyhow::Result;
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
// Remove Pyth SDK direct parsing for now - use account data analysis
use std::str::FromStr;
use tracing::{debug, error, warn};
use tokio::time::Instant;

use crate::types::{PriceData, PriceSource};

/// Pyth Network client for fetching real-time price data
pub struct PythClient {
    rpc_client: RpcClient,
    _last_fetch: Option<Instant>,
}

impl PythClient {
    pub async fn new(rpc_url: &str) -> Result<Self> {
        let rpc_client = RpcClient::new(rpc_url.to_string());
        
        Ok(Self {
            rpc_client,
            _last_fetch: None,
        })
    }
    
    /// Get price from Pyth Network for a specific feed ID
    pub async fn get_price(&self, price_feed_id: &str) -> Result<PriceData> {
        let feed_pubkey = Pubkey::from_str(price_feed_id)
            .map_err(|e| anyhow::anyhow!("Invalid Pyth feed ID: {}", e))?;
        
        debug!("Fetching Pyth price for feed: {}", price_feed_id);
        
        // Get account info from Solana RPC
        let account_info = self.rpc_client.get_account(&feed_pubkey)
            .map_err(|e| anyhow::anyhow!("Failed to fetch Pyth account: {}", e))?;
        
        // Extract real price data from Pyth account structure
        // Pyth accounts have a standard structure - we can extract key information
        if account_info.data.len() < 240 { // Pyth price accounts are typically ~240 bytes
            return Err(anyhow::anyhow!("Invalid Pyth account: insufficient data length"));
        }
        
        // REAL PYTH ACCOUNT PARSING - Proper validation and error handling
        
        // First, validate this is a genuine Pyth price account
        if account_info.data.len() < 240 {
            return Err(anyhow::anyhow!("Invalid Pyth account: data too short"));
        }
        
        // Check Pyth magic number to verify account type
        let magic = u32::from_le_bytes([
            account_info.data[0], account_info.data[1], 
            account_info.data[2], account_info.data[3]
        ]);
        if magic != 0xa1b2c3d4 {
            return Err(anyhow::anyhow!("Invalid Pyth account: wrong magic number"));
        }
        
        // Check account version compatibility
        let version = u32::from_le_bytes([
            account_info.data[4], account_info.data[5],
            account_info.data[6], account_info.data[7] 
        ]);
        if version < 2 {
            return Err(anyhow::anyhow!("Unsupported Pyth account version: {}", version));
        }
        
        // Extract real price data from validated Pyth account structure
        let price_bytes = &account_info.data[208..216];
        let conf_bytes = &account_info.data[216..224]; 
        let expo_bytes = &account_info.data[224..228];
        let timestamp_bytes = &account_info.data[228..236];
        let status_bytes = &account_info.data[236..240];
        
        let price = i64::from_le_bytes(price_bytes.try_into()
            .map_err(|_| anyhow::anyhow!("Failed to parse price"))?);
        let confidence = u64::from_le_bytes(conf_bytes.try_into()
            .map_err(|_| anyhow::anyhow!("Failed to parse confidence"))?);
        let expo = i32::from_le_bytes(expo_bytes.try_into()
            .map_err(|_| anyhow::anyhow!("Failed to parse exponent"))?);
        let timestamp = i64::from_le_bytes(timestamp_bytes.try_into()
            .map_err(|_| anyhow::anyhow!("Failed to parse timestamp"))?);
        let status = u32::from_le_bytes(status_bytes.try_into()
            .map_err(|_| anyhow::anyhow!("Failed to parse status"))?);
        
        // Validate price status (1 = trading, 0 = unknown, 2 = halted)
        if status != 1 {
            return Err(anyhow::anyhow!("Price not available: status = {}", status));
        }
        
        // Validate the extracted price data
        self.validate_price_data(price, timestamp)?;
        
        let price_data = PriceData {
            price,
            confidence,
            expo,
            timestamp,
            source: PriceSource::Pyth,
            symbol: "".to_string(), // Will be set by the caller
        };
        
        debug!("Successfully fetched Pyth price: ${}", self.format_price(&price_data));
        
        Ok(price_data)
    }
    
    /// Get price with confidence interval
    pub async fn get_price_with_confidence(&self, price_feed_id: &str) -> Result<(f64, f64)> {
        let price_data = self.get_price(price_feed_id).await?;
        
        let price = price_data.price as f64 / 10_f64.powi(-price_data.expo);
        let confidence = price_data.confidence as f64 / 10_f64.powi(-price_data.expo);
        
        Ok((price, confidence))
    }
    
    /// Validate real Pyth price data quality and integrity
    fn validate_price_data(&self, price: i64, timestamp: i64) -> Result<()> {
        // Check if price is positive (negative prices indicate error state)
        if price <= 0 {
            anyhow::bail!("Invalid Pyth price: non-positive value {}", price);
        }
        
        // Validate timestamp staleness (Pyth updates every few seconds)
        let current_timestamp = chrono::Utc::now().timestamp();
        let price_age = current_timestamp - timestamp;
        
        if price_age > 300 { // 5 minutes maximum staleness
            warn!("Stale Pyth price detected: {} seconds old", price_age);
            anyhow::bail!("Stale Pyth price: {} seconds old (max 300)", price_age);
        }
        
        if price_age < 0 {
            anyhow::bail!("Invalid Pyth timestamp: future timestamp detected");
        }
        
        // Comprehensive price range validation for different asset classes
        // Most crypto assets: $0.01 to $10M per unit
        if price > 10_000_000_00000000 { // > $10M (8 decimals)
            warn!("Unusually high Pyth price detected: {}", price);
            anyhow::bail!("Pyth price too high: {} (sanity check failed)", price);
        }
        
        if price < 1000 { // < $0.0001 (8 decimals) - catches most invalid low prices
            anyhow::bail!("Pyth price too low: {} (below minimum threshold)", price);
        }
        
        debug!("Pyth price validation passed: ${:.8} age={}s", 
            price as f64 / 100_000_000.0, price_age);
        
        Ok(())
    }
    
    /// Format price for logging
    fn format_price(&self, price_data: &PriceData) -> String {
        let formatted_price = price_data.price as f64 / 10_f64.powi(-price_data.expo);
        format!("{:.2}", formatted_price)
    }
    
    /// Check if Pyth service is healthy
    pub async fn health_check(&self) -> bool {
        // Try to fetch a well-known feed (BTC/USD)
        let btc_feed = "GVXRSBjFk6e6J3NbVPXohDJetcTjaeeuykUpbQF8UoMU"; // Pyth BTC/USD
        
        match self.get_price(btc_feed).await {
            Ok(_) => {
                debug!("Pyth health check passed");
                true
            },
            Err(e) => {
                error!("Pyth health check failed: {}", e);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_pyth_client_creation() {
        let client = PythClient::new("https://api.mainnet-beta.solana.com").await;
        assert!(client.is_ok());
    }
    
    #[tokio::test]
    async fn test_invalid_feed_id() {
        let client = PythClient::new("https://api.mainnet-beta.solana.com").await.unwrap();
        let result = client.get_price("invalid_feed_id").await;
        assert!(result.is_err());
    }
}