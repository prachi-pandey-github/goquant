use axum::{
    extract::{
        ws::{WebSocket, Message},
        WebSocketUpgrade,
        State,
    },
    response::Response,
};
use futures_util::{sink::SinkExt, stream::StreamExt};
use serde_json;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tracing::{info, error, warn};

use crate::{
    manager::OracleManager,
    types::{WsMessage, PriceData},
};

/// WebSocket server state
#[derive(Clone)]
pub struct WsState {
    pub oracle_manager: Arc<OracleManager>,
    pub broadcast_sender: broadcast::Sender<WsMessage>,
}

/// WebSocket connection handler
pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<WsState>,
) -> Response {
    ws.on_upgrade(|socket| handle_websocket(socket, state))
}

/// Handle individual WebSocket connection
async fn handle_websocket(socket: WebSocket, state: WsState) {
    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));
    let mut broadcast_receiver = state.broadcast_sender.subscribe();
    
    info!("New WebSocket connection established");
    
    // Task for handling incoming messages from client
    let sender_clone = sender.clone();
    let client_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    match serde_json::from_str::<WsMessage>(&text) {
                        Ok(ws_msg) => {
                            handle_client_message(ws_msg, &state).await;
                        },
                        Err(e) => {
                            warn!("Failed to parse WebSocket message: {}", e);
                            let error_msg = WsMessage::Error {
                                message: "Invalid message format".to_string(),
                            };
                            if let Ok(json) = serde_json::to_string(&error_msg) {
                                let mut sender = sender_clone.lock().await;
                                if sender.send(Message::Text(json)).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                },
                Ok(Message::Close(_)) => {
                    info!("WebSocket client disconnected");
                    break;
                },
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }
    });
    
    // Task for broadcasting updates to client
    let sender_clone = sender.clone();
    let broadcast_task = tokio::spawn(async move {
        while let Ok(message) = broadcast_receiver.recv().await {
            if let Ok(json) = serde_json::to_string(&message) {
                let mut sender = sender_clone.lock().await;
                if sender.send(Message::Text(json)).await.is_err() {
                    break;
                }
            }
        }
    });
    
    // Wait for either task to complete
    tokio::select! {
        _ = client_task => {},
        _ = broadcast_task => {},
    }
    
    info!("WebSocket connection closed");
}

/// Handle messages from WebSocket clients
async fn handle_client_message(message: WsMessage, _state: &WsState) {
    match message {
        WsMessage::Subscribe { symbols } => {
            info!("Client subscribed to symbols: {:?}", symbols);
            // In a production system, you'd track subscriptions per client
            // For now, we'll just acknowledge the subscription
        },
        WsMessage::Unsubscribe { symbols } => {
            info!("Client unsubscribed from symbols: {:?}", symbols);
        },
        _ => {
            warn!("Unexpected message type from client");
        }
    }
}

/// Broadcast price update to all connected clients
pub async fn broadcast_price_update(
    sender: &broadcast::Sender<WsMessage>,
    symbol: &str,
    price_data: &PriceData,
) {
    let message = WsMessage::PriceUpdate {
        symbol: symbol.to_string(),
        price: price_data.to_decimal(),
        confidence: price_data.confidence_to_decimal(),
        timestamp: price_data.timestamp,
        source: price_data.source.clone(),
    };
    
    if let Err(e) = sender.send(message) {
        error!("Failed to broadcast price update: {}", e);
    }
}

/// Broadcast health alert to all connected clients
pub async fn broadcast_health_alert(
    sender: &broadcast::Sender<WsMessage>,
    oracle: &str,
    status: &str,
    message: &str,
) {
    let alert = WsMessage::HealthAlert {
        oracle: oracle.to_string(),
        status: status.to_string(),
        message: message.to_string(),
        timestamp: chrono::Utc::now().timestamp(),
    };
    
    if let Err(e) = sender.send(alert) {
        error!("Failed to broadcast health alert: {}", e);
    }
}

/// Start WebSocket server
pub async fn start_websocket_server(
    host: &str,
    port: u16,
    oracle_manager: Arc<OracleManager>,
) -> anyhow::Result<()> {
    use axum::{routing::get, Router};
    use tower_http::cors::CorsLayer;
    
    let (broadcast_sender, _) = broadcast::channel(1000);
    
    let state = WsState {
        oracle_manager,
        broadcast_sender,
    };
    
    let app = Router::new()
        .route("/ws", get(websocket_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);
    
    let addr = format!("{}:{}", host, port);
    info!("Starting WebSocket server on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}

/// WebSocket client for testing
#[cfg(test)]
pub struct WebSocketTestClient {
    sender: futures_util::stream::SplitSink<WebSocket, Message>,
    receiver: futures_util::stream::SplitStream<WebSocket>,
}

#[cfg(test)]
impl WebSocketTestClient {
    pub async fn send_message(&mut self, message: WsMessage) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string(&message)?;
        self.sender.send(Message::Text(json)).await?;
        Ok(())
    }
    
    pub async fn receive_message(&mut self) -> Result<Option<WsMessage>, Box<dyn std::error::Error>> {
        if let Some(msg) = self.receiver.next().await {
            match msg? {
                Message::Text(text) => {
                    let ws_msg = serde_json::from_str(&text)?;
                    Ok(Some(ws_msg))
                },
                _ => Ok(None),
            }
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PriceSource;
    
    #[test]
    fn test_websocket_message_serialization() {
        let message = WsMessage::PriceUpdate {
            symbol: "BTC/USD".to_string(),
            price: 50000.0,
            confidence: 10.0,
            timestamp: 1640995200,
            source: PriceSource::Pyth,
        };
        
        let json = serde_json::to_string(&message).unwrap();
        let deserialized: WsMessage = serde_json::from_str(&json).unwrap();
        
        match deserialized {
            WsMessage::PriceUpdate { symbol, price, .. } => {
                assert_eq!(symbol, "BTC/USD");
                assert_eq!(price, 50000.0);
            },
            _ => panic!("Wrong message type"),
        }
    }
    
    #[tokio::test]
    async fn test_broadcast_functionality() {
        let (sender, mut receiver) = broadcast::channel(10);
        
        let message = WsMessage::PriceUpdate {
            symbol: "ETH/USD".to_string(),
            price: 3000.0,
            confidence: 5.0,
            timestamp: 1640995200,
            source: PriceSource::Switchboard,
        };
        
        sender.send(message).unwrap();
        
        let received = receiver.recv().await.unwrap();
        match received {
            WsMessage::PriceUpdate { symbol, price, .. } => {
                assert_eq!(symbol, "ETH/USD");
                assert_eq!(price, 3000.0);
            },
            _ => panic!("Wrong message type"),
        }
    }
}