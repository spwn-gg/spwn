//! Shared backend state, managed by Tauri and accessed from commands.

use crate::claude::ClaudeAgent;
use crate::projects::ProjectsWatcher;
use crate::pty::RmuxSession;
use crate::settings::Settings;
use crate::store::ProjectStore;
use rmux_sdk::Rmux;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;
use tauri::AppHandle;
use tokio::sync::OnceCell;

/// Live in-memory state plus the persisted spwn project store.
#[derive(Default)]
pub struct AppState {
    /// Lazily-connected rmux daemon handle.
    pub rmux: OnceCell<Rmux>,
    /// Live shell terminals (rmux), keyed by terminal id.
    pub sessions: Mutex<HashMap<String, RmuxSession>>,
    /// Live Claude chat sessions (Agent SDK sidecars), keyed by terminal id.
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
    /// App handle (set at startup) so background helpers can emit events.
    pub app: Mutex<Option<AppHandle>>,
    /// Cache of Claude session titles keyed by session id → (file mtime, title),
    /// so list_projects doesn't re-read every transcript on each refresh.
    pub title_cache: Mutex<HashMap<String, (SystemTime, String)>>,
    /// Scheduled-task ids currently mid-run, so the scheduler (and Run-now) never
    /// start a second instance of the same task while one is in flight.
    pub running_tasks: Mutex<HashSet<String>>,
    /// Set true only for a real quit (tray Quit / updater relaunch) so the
    /// ExitRequested handler knows to let the process die instead of staying
    /// alive in the background for the scheduler.
    pub quitting: AtomicBool,
}
