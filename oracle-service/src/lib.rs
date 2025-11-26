pub mod manager;
pub mod clients;
pub mod aggregator;
pub mod cache;
pub mod types;
pub mod api;
pub mod websocket;

use anyhow::Result;
use std::sync::Arc;
use tracing::{info, error};
use tokio::signal;

use crate::{
    manager::OracleManager,
    api::start_server,
    websocket::start_websocket_server,
    types::{Config, Symbol},
};

/// Main application entry point
pub async fn run() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();
    
    info!("Starting Oracle Integration Service");
    
    // Load configuration
    let config = load_config()?;
    
    // Initialize Oracle Manager
    let oracle_manager = Arc::new(
        OracleManager::new(
            &config.solana.rpc_url,
            &config.redis.url,
            config.oracles,
        ).await?
    );
    
    info!("Oracle Manager initialized successfully");
    
    // Start the oracle price fetching in background
    let manager_clone = oracle_manager.clone();
    let oracle_task = tokio::spawn(async move {
        if let Err(e) = manager_clone.start().await {
            error!("Oracle manager failed: {}", e);
        }
    });
    
    // Start REST API server
    let api_manager = oracle_manager.clone();
    let api_task = tokio::spawn(async move {
        if let Err(e) = start_server(&config.server.host, config.server.port, api_manager).await {
            error!("API server failed: {}", e);
        }
    });
    
    // Start WebSocket server
    let ws_port = config.server.port + 1; // WebSocket on port + 1
    let ws_manager = oracle_manager.clone();
    let ws_task = tokio::spawn(async move {
        if let Err(e) = start_websocket_server(&config.server.host, ws_port, ws_manager).await {
            error!("WebSocket server failed: {}", e);
        }
    });
    
    info!("All services started successfully");
    info!("REST API: http://{}:{}", config.server.host, config.server.port);
    info!("WebSocket: ws://{}:{}", config.server.host, ws_port);
    
    // Wait for shutdown signal
    match signal::ctrl_c().await {
        Ok(()) => {
            info!("Received shutdown signal, stopping services...");
            oracle_manager.stop().await;
        },
        Err(err) => {
            error!("Unable to listen for shutdown signal: {}", err);
        },
    }
    
    // Wait for tasks to complete
    let _ = tokio::join!(oracle_task, api_task, ws_task);
    
    info!("Oracle Integration Service stopped");
    Ok(())
}

/// Load configuration from file and environment
fn load_config() -> Result<Config> {
    // Load from config file if available, otherwise use defaults
    let default_symbols = vec![
        Symbol {
            name: "BTC/USD".to_string(),
            pyth_feed_id: "GVXRSBjFk6e6J3NbVPXohDJetcTjaeeuykUpbQF8UoMU".to_string(),
            switchboard_aggregator: "8SXvChNYFhRq4EZuZvnhjrB3jJRQCv4k3P4W6hesH3Ee".to_string(),
            max_staleness: 60,
            max_confidence: 10000, // 100% in basis points
            max_deviation: 500,    // 5% in basis points
        },
        Symbol {
            name: "ETH/USD".to_string(),
            pyth_feed_id: "JBu1AL4obBcCMqKBBxhpWCNUt136ijcuMZLFvTP7iWdB".to_string(),
            switchboard_aggregator: "2V7t5NiKWCxh8nMp6Cmmmp3vVpQJWZTjdVa2G1VkqTEp".to_string(),
            max_staleness: 60,
            max_confidence: 10000,
            max_deviation: 500,
        },
        Symbol {
            name: "SOL/USD".to_string(),
            pyth_feed_id: "H6ARHf6YXhGYeQfUzQNGk6rDNnLBQKrenN712K4AQJEG".to_string(),
            switchboard_aggregator: "7VJsBtJzgTftYzEeooSDYyjKXvYRWJHdwvbwfBvTg9K".to_string(),
            max_staleness: 60,
            max_confidence: 10000,
            max_deviation: 500,
        },
    ];
    
    let config = Config {
        solana: crate::types::SolanaConfig {
            rpc_url: std::env::var("SOLANA_RPC_URL")
                .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string()),
            commitment: "confirmed".to_string(),
        },
        redis: crate::types::RedisConfig {
            url: std::env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string()),
            pool_size: 10,
        },
        database: crate::types::DatabaseConfig {
            url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgresql://postgres:password@localhost/oracle_db".to_string()),
            max_connections: 10,
        },
        server: crate::types::ServerConfig {
            host: std::env::var("HOST")
                .unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: std::env::var("PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()
                .unwrap_or(8080),
            cors_origins: vec!["*".to_string()],
        },
        oracles: default_symbols,
    };
    
    Ok(config)
}