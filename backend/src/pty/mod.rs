//! Session management: locating/launching programs under rmux and streaming I/O.

mod launcher;
mod manager;

pub use launcher::{default_shell, find_claude_bin, find_rmux_bin};
pub use manager::{spawn_rmux_session, RmuxSession};
