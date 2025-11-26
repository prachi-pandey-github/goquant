use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, error, warn};
use std::collections::HashMap;
use std::time::Duration;

use crate::clients::{PythClient, SwitchboardClient};
use crate::aggregator::PriceAggregator;
use crate::cache::PriceCache;
use crate::types::{PriceData, OracleHealth, Symbol};

/// Core Oracle Manager that orchestrates all oracle operations
pub struct OracleManager {
    pyth_client: Arc<PythClient>,
    switchboard_client: Arc<SwitchboardClient>, 
    price_aggregator: Arc<PriceAggregator>,
    price_cache: Arc<PriceCache>,
    health_status: Arc<RwLock<HashMap<String, OracleHealth>>>,
    symbols: Vec<Symbol>,
    is_running: Arc<RwLock<bool>>,
}

impl OracleManager {
    pub async fn new(
        rpc_url: &str,
        redis_url: &str,
        symbols: Vec<Symbol>
    ) -> Result<Self> {
        info!("Initializing Oracle Manager with {} symbols", symbols.len());
        
        // Initialize clients
        let pyth_client = Arc::new(PythClient::new(rpc_url).await?);
        let switchboard_client = Arc::new(SwitchboardClient::new(rpc_url).await?);
        
        // Initialize aggregator and cache
        let price_aggregator = Arc::new(PriceAggregator::new());
        let price_cache = Arc::new(PriceCache::new(redis_url).await?);
        
        // Initialize health status tracking
        let mut health_status = HashMap::new();
        for symbol in &symbols {
            health_status.insert(symbol.name.clone(), OracleHealth::default());
        }
        
        Ok(Self {
            pyth_client,
            switchboard_client,
            price_aggregator,
            price_cache,
            health_status: Arc::new(RwLock::new(health_status)),
            symbols,
            is_running: Arc::new(RwLock::new(false)),
        })
    }
    
    /// Start the oracle manager with continuous price fetching
    pub async fn start(&self) -> Result<()> {
        info!("Starting Oracle Manager");
        *self.is_running.write().await = true;
        
        // Start price fetching for all symbols
        let tasks: Vec<_> = self.symbols.iter().map(|symbol| {
            let symbol = symbol.clone();
            let manager = self.clone();
            tokio::spawn(async move {
                manager.price_fetch_loop(symbol).await;
            })
        }).collect();
        
        // Wait for all tasks to complete
        for task in tasks {
            if let Err(e) = task.await {
                error!("Price fetch task failed: {}", e);
            }
        }
        
        Ok(())
    }
    
    /// Stop the oracle manager
    pub async fn stop(&self) {
        info!("Stopping Oracle Manager");
        *self.is_running.write().await = false;
    }
    
    /// Main price fetching loop for a specific symbol
    async fn price_fetch_loop(&self, symbol: Symbol) {
        info!("Starting price fetch loop for {}", symbol.name);
        
        while *self.is_running.read().await {
            match self.fetch_and_aggregate_price(&symbol).await {
                Ok(price_data) => {
                    // Cache the aggregated price
                    if let Err(e) = self.price_cache.set_price(&symbol.name, &price_data).await {
                        error!("Failed to cache price for {}: {}", symbol.name, e);
                    }
                    
                    // Update health status
                    self.update_health_status(&symbol.name, true).await;
                },
                Err(e) => {
                    error!("Failed to fetch price for {}: {}", symbol.name, e);
                    self.update_health_status(&symbol.name, false).await;
                }
            }
            
            // Wait before next fetch (configurable interval)
            tokio::time::sleep(Duration::from_millis(500)).await; // 500ms for sub-second updates
        }
    }
    
    /// Fetch prices from all sources and aggregate them
    async fn fetch_and_aggregate_price(&self, symbol: &Symbol) -> Result<PriceData> {
        let mut prices = Vec::new();
        
        // Fetch from Pyth
        match self.pyth_client.get_price(&symbol.pyth_feed_id).await {
            Ok(pyth_price) => {
                prices.push(pyth_price);
            },
            Err(e) => {
                warn!("Pyth price fetch failed for {}: {}", symbol.name, e);
            }
        }
        
        // Fetch from Switchboard  
        match self.switchboard_client.get_price(&symbol.switchboard_aggregator).await {
            Ok(sb_price) => {
                prices.push(sb_price);
            },
            Err(e) => {
                warn!("Switchboard price fetch failed for {}: {}", symbol.name, e);
            }
        }
        
        // Ensure we have at least one price
        if prices.is_empty() {
            anyhow::bail!("No price sources available for {}", symbol.name);
        }
        
        // Aggregate prices using consensus algorithm
        let aggregated_price = self.price_aggregator.aggregate_prices(&prices, &symbol)?;
        
        Ok(aggregated_price)
    }
    
    /// Get current price for a symbol from cache or fetch fresh
    pub async fn get_current_price(&self, symbol: &str) -> Result<PriceData> {
        // Try cache first
        if let Ok(Some(cached_price)) = self.price_cache.get_price(symbol).await {
            // Check if price is not stale (within last 5 seconds)
            if cached_price.is_fresh(Duration::from_secs(5)) {
                return Ok(cached_price);
            }
        }
        
        // Find symbol configuration
        let symbol_config = self.symbols.iter()
            .find(|s| s.name == symbol)
            .ok_or_else(|| anyhow::anyhow!("Symbol {} not configured", symbol))?;
        
        // Fetch fresh price
        self.fetch_and_aggregate_price(symbol_config).await
    }
    
    /// Get prices for all configured symbols
    pub async fn get_all_prices(&self) -> HashMap<String, PriceData> {
        let mut prices = HashMap::new();
        
        for symbol in &self.symbols {
            if let Ok(price) = self.get_current_price(&symbol.name).await {
                prices.insert(symbol.name.clone(), price);
            }
        }
        
        prices
    }
    
    /// Get health status for all oracles
    pub async fn get_health_status(&self) -> HashMap<String, OracleHealth> {
        self.health_status.read().await.clone()
    }
    
    /// Update health status for a symbol
    async fn update_health_status(&self, symbol: &str, is_healthy: bool) {
        let mut health = self.health_status.write().await;
        if let Some(status) = health.get_mut(symbol) {
            status.update(is_healthy);
        }
    }
}

// Implement Clone for OracleManager to enable sharing across async tasks
impl Clone for OracleManager {
    fn clone(&self) -> Self {
        Self {
            pyth_client: self.pyth_client.clone(),
            switchboard_client: self.switchboard_client.clone(),
            price_aggregator: self.price_aggregator.clone(),
            price_cache: self.price_cache.clone(),
            health_status: self.health_status.clone(),
            symbols: self.symbols.clone(),
            is_running: self.is_running.clone(),
        }
    }
}