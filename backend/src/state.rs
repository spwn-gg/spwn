//! Shared backend state, wrapped in an `Arc` and handed to commands + the server.

use crate::claude::{ClaudeAgent, SessionStatus};
use crate::hooks::HookRun;
use crate::projects::ProjectsWatcher;
use crate::pty::RmuxSession;
use crate::server::hub::EventHub;
use crate::settings::Settings;
use crate::store::ProjectStore;
use rmux_sdk::Rmux;
use parking_lot::Mutex;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;
use tokio::sync::OnceCell;

/// Live in-memory state plus the persisted spwn project store.
#[derive(Default)]
pub struct AppState {
    /// Event bus to connected browsers (WebSocket fan-out of backend events).
    pub hub: EventHub,
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
    /// Most recent hook runs per session: terminal id → (event name → runs, one per
    /// scope that ran, global-first). Drives the Hooks panel; cleared on session delete.
    pub hook_runs: Mutex<HashMap<String, BTreeMap<String, Vec<HookRun>>>>,
    /// Sessions with a hook executing right now: terminal id → event name. Drives the
    /// live "running" spinner on tabs / the project tree while a hook streams output.
    pub hooks_running: Mutex<HashMap<String, String>>,
    /// Outstanding hook UI prompts awaiting a user answer: prompt id → answer sender.
    /// The synchronous hook runner blocks on the receiver; `hooks_prompt_answer` sends
    /// the chosen label(s) to unblock it.
    pub hook_prompts: Mutex<HashMap<String, std::sync::mpsc::Sender<String>>>,
    /// Agent definitions spwn can drive (built-in + `~/.spwn/agents` + per-repo).
    /// Loaded at startup and reloadable at runtime, so editing a TOML to fix a
    /// changed TUI doesn't need a restart.
    pub agents: Mutex<crate::agents::AgentRegistry>,
    /// Live Claude session status: terminal id → status. Derived in the sidecar reader
    /// so background sessions (no pane) still drive the sidebar. Ephemeral (empty on
    /// launch; restart falls back to persisted `needs_attention`/`attention_reason`).
    pub claude_status: Mutex<HashMap<String, SessionStatus>>,
}
