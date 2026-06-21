//! Shared backend state, managed by Tauri and accessed from commands.

use crate::claude::ClaudeAgent;
use crate::projects::ProjectsWatcher;
use crate::pty::RmuxSession;
use crate::settings::Settings;
use crate::store::ProjectStore;
use rmux_sdk::Rmux;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tokio::sync::OnceCell;

/// Live in-memory state plus the persisted Context Manager project store.
#[derive(Default)]
pub struct AppState {
    /// Lazily-connected rmux daemon handle.
    pub rmux: OnceCell<Rmux>,
    /// Live shell terminals (rmux), keyed by terminal id.
    pub sessions: Mutex<HashMap<String, RmuxSession>>,
    /// Live Claude chat terminals (Agent SDK sidecars), keyed by terminal id.
    pub claude_agents: Mutex<HashMap<String, ClaudeAgent>>,
    /// Watches ~/.claude/projects for live transcript refresh.
    pub watcher: Mutex<Option<ProjectsWatcher>>,
    /// CM-owned projects/terminals (persisted to disk).
    pub store: Mutex<ProjectStore>,
    /// Path to projects.json (resolved at startup).
    pub store_path: Mutex<Option<PathBuf>>,
    /// User settings (persisted to disk).
    pub settings: Mutex<Settings>,
    /// Path to settings.json (resolved at startup).
    pub settings_path: Mutex<Option<PathBuf>>,
}
