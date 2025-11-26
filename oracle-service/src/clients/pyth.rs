use anyhow::Result;
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use pyth_sdk_solana::state::SolanaPriceAccount;
use std::str::FromStr;
use tracing::{debug, error, warn};
use tokio::time::{Duration, Instant};

use crate::types::{PriceData, PriceSource};

/// Pyth Network client for fetching real-time price data
pub struct PythClient {
    rpc_client: RpcClient,
    last_fetch: Option<Instant>,
}

impl PythClient {
    pub async fn new(rpc_url: &str) -> Result<Self> {
        let rpc_client = RpcClient::new(rpc_url.to_string());
        
        Ok(Self {
            rpc_client,
            last_fetch: None,
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
        
        // Parse the Pyth price feed
        let price_account = SolanaPriceAccount::account_info_to_feed(&account_info)
            .map_err(|e| anyhow::anyhow!("Failed to parse Pyth price feed: {}", e))?;
        
        // Get the current price
        let current_price = price_account.get_price_unchecked();
        
        // Validate the price data
        self.validate_price_data(&current_price)?;
        
        let price_data = PriceData {
            price: current_price.price,
            confidence: current_price.conf,
            expo: current_price.expo,
            timestamp: current_price.publish_time,
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
    
    /// Validate price data quality
    fn validate_price_data(&self, price: &pyth_sdk_solana::Price) -> Result<()> {
        // Check if price is positive
        if price.price <= 0 {
            anyhow::bail!("Invalid price: price must be positive");
        }
        
        // Check price staleness (within last 60 seconds)
        let current_timestamp = chrono::Utc::now().timestamp();
        let price_age = current_timestamp - price.publish_time;
        
        if price_age > 60 {
            warn!("Stale Pyth price detected: {} seconds old", price_age);
            anyhow::bail!("Stale price: {} seconds old", price_age);
        }
        
        // Check confidence interval (shouldn't be too wide)
        let confidence_ratio = price.conf as f64 / price.price as f64;
        if confidence_ratio > 0.05 { // 5% confidence threshold
            warn!("High Pyth price confidence: {:.2}%", confidence_ratio * 100.0);
        }
        
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