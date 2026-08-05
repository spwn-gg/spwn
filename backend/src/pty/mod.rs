//! Session management: locating/launching programs under rmux and streaming I/O.

mod launcher;
mod manager;

pub use launcher::{default_shell, find_rmux_bin};
pub use manager::{
    prime_pane, spawn_pane, split_exec_prefix, AgentRuntime, PaneActivity, PaneSession, SpawnSpec,
};
