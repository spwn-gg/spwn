//! Watch the Claude Code projects directory and notify the frontend (debounced)
//! whenever sessions appear/change, so the navigation tree can refresh.

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, FileIdMap};
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// The concrete debouncer type; kept alive in AppState (dropping it stops watching).
pub type ProjectsWatcher = Debouncer<RecommendedWatcher, FileIdMap>;

/// Start watching `root` recursively. Each debounced batch of FS events emits a
/// single `projects://changed` event whose payload is the list of changed session
/// ids (the `.jsonl` file stems) — so a mirror only reloads its own transcript.
pub fn start(app: AppHandle, root: &Path) -> anyhow::Result<ProjectsWatcher> {
    let mut debouncer = new_debouncer(
        Duration::from_millis(400),
        None,
        move |result: DebounceEventResult| {
            let Ok(events) = result else {
                return;
            };
            let mut ids: HashSet<String> = HashSet::new();
            for event in &events {
                for path in &event.paths {
                    if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            ids.insert(stem.to_string());
                        }
                    }
                }
            }
            let ids: Vec<String> = ids.into_iter().collect();
            let _ = app.emit("projects://changed", ids);
        },
    )?;
    debouncer.watcher().watch(root, RecursiveMode::Recursive)?;
    Ok(debouncer)
}
