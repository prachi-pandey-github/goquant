use axum::{
    extract::{Path, Query, State},
    http::{StatusCode, HeaderMap},
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tower_http::cors::CorsLayer;
use tracing::{info, error};

use crate::{
    manager::OracleManager,
    types::{PriceResponse, HealthResponse, OracleHealthStatus, CacheHealthStatus},
    cache::PriceCache,
};

/// REST API server state
#[derive(Clone)]
pub struct ApiState {
    pub oracle_manager: Arc<OracleManager>,
}

/// Query parameters for price history
#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<usize>,
    pub since: Option<i64>,
}

/// Request body for batch price queries
#[derive(Debug, Deserialize)]
pub struct BatchPriceRequest {
    pub symbols: Vec<String>,
}

/// Build the REST API router
pub fn create_router(state: ApiState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/oracle/price/:symbol", get(get_price))
        .route("/oracle/prices", get(get_all_prices))
        .route("/oracle/prices/batch", post(get_batch_prices))
        .route("/oracle/history/:symbol", get(get_price_history))
        .route("/oracle/sources/:symbol", get(get_source_prices))
        .route("/oracle/health", get(get_oracle_health))
        .route("/oracle/stats", get(get_oracle_stats))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Health check endpoint
pub async fn health_check() -> Result<Json<serde_json::Value>, StatusCode> {
    Ok(Json(serde_json::json!({
        "status": "healthy",
        "service": "oracle-integration",
        "timestamp": chrono::Utc::now().timestamp()
    })))
}

/// Get current price for a specific symbol
pub async fn get_price(
    State(state): State<ApiState>,
    Path(symbol): Path<String>,
) -> Result<Json<PriceResponse>, (StatusCode, Json<serde_json::Value>)> {
    info!("Fetching price for symbol: {}", symbol);
    
    match state.oracle_manager.get_current_price(&symbol).await {
        Ok(price_data) => {
            let response = PriceResponse::from_price_data(&price_data);
            Ok(Json(response))
        },
        Err(e) => {
            error!("Failed to get price for {}: {}", symbol, e);
            Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": "Price not available",
                    "symbol": symbol,
                    "message": e.to_string()
                }))
            ))
        }
    }
}

/// Get current prices for all configured symbols
pub async fn get_all_prices(
    State(state): State<ApiState>,
) -> Result<Json<HashMap<String, PriceResponse>>, (StatusCode, Json<serde_json::Value>)> {
    info!("Fetching all prices");
    
    let prices = state.oracle_manager.get_all_prices().await;
    
    let response: HashMap<String, PriceResponse> = prices
        .iter()
        .map(|(symbol, price_data)| {
            (symbol.clone(), PriceResponse::from_price_data(price_data))
        })
        .collect();
    
    Ok(Json(response))
}

/// Get prices for multiple symbols in batch
pub async fn get_batch_prices(
    State(state): State<ApiState>,
    Json(request): Json<BatchPriceRequest>,
) -> Result<Json<HashMap<String, Option<PriceResponse>>>, (StatusCode, Json<serde_json::Value>)> {
    info!("Fetching batch prices for {} symbols", request.symbols.len());
    
    let mut response = HashMap::new();
    
    for symbol in request.symbols {
        match state.oracle_manager.get_current_price(&symbol).await {
            Ok(price_data) => {
                response.insert(symbol, Some(PriceResponse::from_price_data(&price_data)));
            },
            Err(_) => {
                response.insert(symbol, None);
            }
        }
    }
    
    Ok(Json(response))
}

/// Get price history for a symbol
pub async fn get_price_history(
    State(state): State<ApiState>,
    Path(symbol): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<Vec<PriceResponse>>, (StatusCode, Json<serde_json::Value>)> {
    info!("Fetching price history for symbol: {}", symbol);
    
    let limit = query.limit.unwrap_or(100).min(1000); // Cap at 1000 entries
    
    // This would typically come from a database
    // For now, we'll return a placeholder response
    let response = vec![];
    
    Ok(Json(response))
}

/// Get individual source prices for a symbol (before aggregation)
pub async fn get_source_prices(
    State(state): State<ApiState>,
    Path(symbol): Path<String>,
) -> Result<Json<SourcePricesResponse>, (StatusCode, Json<serde_json::Value>)> {
    info!("Fetching source prices for symbol: {}", symbol);
    
    // This would fetch individual oracle prices
    // For now, return a placeholder
    let response = SourcePricesResponse {
        symbol: symbol.clone(),
        sources: HashMap::new(),
        aggregated: None,
    };
    
    Ok(Json(response))
}

/// Get oracle health status
pub async fn get_oracle_health(
    State(state): State<ApiState>,
) -> Result<Json<HealthResponse>, (StatusCode, Json<serde_json::Value>)> {
    info!("Fetching oracle health status");
    
    let health_status = state.oracle_manager.get_health_status().await;
    
    let oracles: HashMap<String, OracleHealthStatus> = health_status
        .iter()
        .map(|(symbol, health)| (symbol.clone(), health.into()))
        .collect();
    
    let overall_healthy = oracles.values().all(|status| status.is_healthy);
    
    let response = HealthResponse {
        overall_status: if overall_healthy { "healthy".to_string() } else { "degraded".to_string() },
        oracles,
        cache_status: CacheHealthStatus {
            is_connected: true, // This would be checked against actual cache
            total_keys: 0,      // This would be fetched from cache
            memory_usage: None,
        },
        uptime: 0, // This would be calculated from service start time
    };
    
    Ok(Json(response))
}

/// Get oracle statistics and metrics
pub async fn get_oracle_stats(
    State(_state): State<ApiState>,
) -> Result<Json<OracleStatsResponse>, StatusCode> {
    info!("Fetching oracle statistics");
    
    // This would collect various metrics
    let response = OracleStatsResponse {
        total_symbols: 0,
        active_connections: 0,
        cache_hit_rate: 0.0,
        average_response_time: 0.0,
        requests_per_second: 0.0,
        error_rate: 0.0,
    };
    
    Ok(Json(response))
}

/// Response structure for source prices
#[derive(Debug, Serialize)]
pub struct SourcePricesResponse {
    pub symbol: String,
    pub sources: HashMap<String, PriceResponse>,
    pub aggregated: Option<PriceResponse>,
}

/// Response structure for oracle statistics
#[derive(Debug, Serialize)]
pub struct OracleStatsResponse {
    pub total_symbols: usize,
    pub active_connections: usize,
    pub cache_hit_rate: f64,
    pub average_response_time: f64,
    pub requests_per_second: f64,
    pub error_rate: f64,
}

/// Start the REST API server
pub async fn start_server(
    host: &str,
    port: u16,
    oracle_manager: Arc<OracleManager>,
) -> anyhow::Result<()> {
    let state = ApiState {
        oracle_manager,
    };
    
    let app = create_router(state);
    let addr = format!("{}:{}", host, port);
    
    info!("Starting REST API server on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, Method},
    };
    use tower::ServiceExt;
    
    #[tokio::test]
    async fn test_health_check() {
        let state = ApiState {
            oracle_manager: Arc::new(
                // This would need a proper mock oracle manager
                // OracleManager::new("", "", vec![]).await.unwrap()
            ),
        };
        
        let app = create_router(state);
        
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
    }
}