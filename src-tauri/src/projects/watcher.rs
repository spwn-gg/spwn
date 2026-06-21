//! Watch the Claude Code projects directory and notify the frontend (debounced)
//! whenever sessions appear/change, so the navigation tree can refresh.

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, FileIdMap};
use std::path::Path;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// The concrete debouncer type; kept alive in AppState (dropping it stops watching).
pub type ProjectsWatcher = Debouncer<RecommendedWatcher, FileIdMap>;

/// Start watching `root` recursively. Each debounced batch of FS events emits a
/// single `projects://changed` event to the frontend.
pub fn start(app: AppHandle, root: &Path) -> anyhow::Result<ProjectsWatcher> {
    let mut debouncer = new_debouncer(
        Duration::from_millis(400),
        None,
        move |result: DebounceEventResult| {
            if result.is_ok() {
                let _ = app.emit("projects://changed", ());
            }
        },
    )?;
    debouncer.watcher().watch(root, RecursiveMode::Recursive)?;
    Ok(debouncer)
}
