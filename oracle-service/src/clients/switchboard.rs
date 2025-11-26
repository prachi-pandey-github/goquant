use anyhow::Result;
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use tracing::{debug, error, warn};
use switchboard_solana::{AggregatorAccountData, SwitchboardDecimal};

use crate::types::{PriceData, PriceSource};

/// Switchboard client for fetching decentralized oracle data
pub struct SwitchboardClient {
    rpc_client: RpcClient,
}

impl SwitchboardClient {
    pub async fn new(rpc_url: &str) -> Result<Self> {
        let rpc_client = RpcClient::new(rpc_url.to_string());
        
        Ok(Self {
            rpc_client,
        })
    }
    
    /// Get price from Switchboard aggregator
    pub async fn get_price(&self, aggregator_address: &str) -> Result<PriceData> {
        let aggregator_pubkey = Pubkey::from_str(aggregator_address)
            .map_err(|e| anyhow::anyhow!("Invalid Switchboard aggregator address: {}", e))?;
        
        debug!("Fetching Switchboard price from aggregator: {}", aggregator_address);
        
        // Get account info from Solana RPC
        let account_info = self.rpc_client.get_account(&aggregator_pubkey)
            .map_err(|e| anyhow::anyhow!("Failed to fetch Switchboard account: {}", e))?;
        
        // Parse Switchboard aggregator data
        let aggregator_data = AggregatorAccountData::new(&account_info.data)
            .map_err(|e| anyhow::anyhow!("Failed to parse Switchboard aggregator: {}", e))?;
        
        // Get the latest result
        let latest_result = aggregator_data
            .latest_confirmed_round
            .result
            .ok_or_else(|| anyhow::anyhow!("No confirmed round available"))?;
        
        // Validate the result
        self.validate_result(&latest_result, &aggregator_data)?;
        
        // Convert SwitchboardDecimal to our format
        let price_data = PriceData {
            price: latest_result.mantissa,
            confidence: self.calculate_confidence(&aggregator_data)?,
            expo: -(latest_result.scale as i32),
            timestamp: aggregator_data.latest_confirmed_round.round_open_timestamp,
            source: PriceSource::Switchboard,
            symbol: "".to_string(), // Will be set by the caller
        };
        
        debug!("Successfully fetched Switchboard price: ${}", self.format_price(&price_data));
        
        Ok(price_data)
    }
    
    /// Calculate confidence interval based on oracle variance
    fn calculate_confidence(&self, aggregator_data: &AggregatorAccountData) -> Result<u64> {
        // Use the variance from the aggregator if available
        // This is a simplified confidence calculation
        let variance = aggregator_data
            .latest_confirmed_round
            .std_deviation
            .unwrap_or(SwitchboardDecimal { mantissa: 100000, scale: 8 }); // Default 1% std dev
        
        Ok(variance.mantissa)
    }
    
    /// Validate Switchboard result data
    fn validate_result(&self, result: &SwitchboardDecimal, aggregator_data: &AggregatorAccountData) -> Result<()> {
        // Check if price is positive
        if result.mantissa <= 0 {
            anyhow::bail!("Invalid Switchboard price: price must be positive");
        }
        
        // Check data freshness (within last 60 seconds)
        let current_timestamp = chrono::Utc::now().timestamp();
        let data_age = current_timestamp - aggregator_data.latest_confirmed_round.round_open_timestamp;
        
        if data_age > 60 {
            warn!("Stale Switchboard data detected: {} seconds old", data_age);
            anyhow::bail!("Stale data: {} seconds old", data_age);
        }
        
        // Check minimum oracle count (ensure decentralization)
        let oracle_count = aggregator_data.latest_confirmed_round.num_success;
        if oracle_count < 3 {
            warn!("Low Switchboard oracle count: {} oracles", oracle_count);
        }
        
        Ok(())
    }
    
    /// Format price for logging
    fn format_price(&self, price_data: &PriceData) -> String {
        let formatted_price = price_data.price as f64 / 10_f64.powi(-price_data.expo);
        format!("{:.2}", formatted_price)
    }
    
    /// Get detailed oracle information
    pub async fn get_oracle_info(&self, aggregator_address: &str) -> Result<OracleInfo> {
        let aggregator_pubkey = Pubkey::from_str(aggregator_address)?;
        let account_info = self.rpc_client.get_account(&aggregator_pubkey)?;
        let aggregator_data = AggregatorAccountData::new(&account_info.data)?;
        
        Ok(OracleInfo {
            aggregator_address: aggregator_address.to_string(),
            oracle_count: aggregator_data.oracle_request_batch_size,
            min_oracle_results: aggregator_data.min_oracle_results,
            update_interval: aggregator_data.min_update_delay_seconds,
            variance: aggregator_data.latest_confirmed_round.std_deviation,
            last_update: aggregator_data.latest_confirmed_round.round_open_timestamp,
        })
    }
    
    /// Check if Switchboard service is healthy
    pub async fn health_check(&self) -> bool {
        // Try to fetch a well-known aggregator (example BTC/USD)
        let btc_aggregator = "8SXvChNYFhRq4EZuZvnhjrB3jJRQCv4k3P4W6hesH3Ee"; // Example Switchboard BTC/USD
        
        match self.get_price(btc_aggregator).await {
            Ok(_) => {
                debug!("Switchboard health check passed");
                true
            },
            Err(e) => {
                error!("Switchboard health check failed: {}", e);
                false
            }
        }
    }
}

/// Detailed oracle information for monitoring
#[derive(Debug, Clone)]
pub struct OracleInfo {
    pub aggregator_address: String,
    pub oracle_count: u32,
    pub min_oracle_results: u32,
    pub update_interval: u32,
    pub variance: Option<SwitchboardDecimal>,
    pub last_update: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_switchboard_client_creation() {
        let client = SwitchboardClient::new("https://api.mainnet-beta.solana.com").await;
        assert!(client.is_ok());
    }
    
    #[tokio::test]
    async fn test_invalid_aggregator_address() {
        let client = SwitchboardClient::new("https://api.mainnet-beta.solana.com").await.unwrap();
        let result = client.get_price("invalid_address").await;
        assert!(result.is_err());
    }
}