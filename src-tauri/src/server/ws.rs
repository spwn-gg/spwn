//! The single multiplexed WebSocket per browser tab.
//!
//! Every `{topic, payload}` frame from the [`EventHub`](super::hub::EventHub) is
//! forwarded verbatim; the client filters by topic (matching the old `listen(topic)`
//! semantics, including dynamic per-terminal topics). Inbound messages are ignored —
//! all client→server calls go over HTTP `/api/invoke`.

use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;

pub async fn handler(State(state): State<Arc<AppState>>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| forward(socket, state))
}

async fn forward(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.hub.subscribe();
    loop {
        tokio::select! {
            frame = rx.recv() => match frame {
                Ok(msg) => {
                    if socket.send(Message::Text(msg.to_string())).await.is_err() {
                        break;
                    }
                }
                // A slow client fell behind: drop the gap and keep going (rmux replays
                // its recent buffer; other streams are re-fetchable).
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(_)) => {}
                Some(Err(_)) | None => break,
            }
        }
    }
}
