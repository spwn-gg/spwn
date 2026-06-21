//! Helpers over Claude Code's own `~/.claude/projects` (session-file location for
//! the transcript panel) plus a filesystem watcher for live transcript refresh.

mod scanner;
mod watcher;

pub use scanner::{locate_session, projects_root, session_title};
pub use watcher::{start as start_watcher, ProjectsWatcher};
