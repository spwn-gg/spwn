//! The event hub: replaces Tauri's `AppHandle::emit`.
//!
//! Background code (the scheduler, the fs watcher, the pty output task, the Claude
//! sidecar reader threads, hook runners) publishes `{topic, payload}` messages here;
//! each connected browser holds one WebSocket that forwards every message and filters
//! by topic client-side — mirroring the old `listen(topic)` semantics.

use serde::Serialize;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Broadcast bus of pre-serialized JSON frames. Cloning is cheap (a `Sender` clone),
/// so every task can own a copy without a lock — this is what lets us drop the old
/// `Mutex<Option<AppHandle>>`.
#[derive(Clone)]
pub struct EventHub {
    tx: broadcast::Sender<Arc<str>>,
}

impl Default for EventHub {
    fn default() -> Self {
        // Generous capacity: the base64 pty stream is bursty; a lagging client skips
        // frames rather than stalling the producer (rmux replays its recent buffer).
        Self {
            tx: broadcast::channel(4096).0,
        }
    }
}

impl EventHub {
    /// Serialize `{topic, payload}` once and broadcast the shared frame. `Err` (no
    /// live subscribers) is intentionally ignored so producers keep running when no
    /// browser is connected — terminals survive reloads.
    pub fn emit<T: Serialize>(&self, topic: &str, payload: T) {
        #[derive(Serialize)]
        struct Wire<'a, T> {
            topic: &'a str,
            payload: T,
        }
        match serde_json::to_string(&Wire { topic, payload }) {
            Ok(json) => {
                let _ = self.tx.send(Arc::from(json.as_str()));
            }
            Err(e) => eprintln!("event serialize failed for {topic}: {e}"),
        }
    }

    /// A new per-client receiver for the WebSocket forward loop.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<str>> {
        self.tx.subscribe()
    }
}
