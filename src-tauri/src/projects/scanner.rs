//! Helpers for locating Claude Code's own session files (used by the transcript
//! panel and new-session discovery). spwn's project model lives in
//! `store.rs`; this only deals with `~/.claude/projects`.

use serde_json::Value;
use std::fs;
use std::path::PathBuf;

/// The Claude Code projects directory: `$CLAUDE_CONFIG_DIR/projects` if set, else
/// `$HOME/.claude/projects`.
pub fn projects_root() -> PathBuf {
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir).join("projects");
        }
    }
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".claude").join("projects"))
        .unwrap_or_else(|| PathBuf::from(".claude/projects"))
}

/// Claude's human-readable name for a session: the latest `ai-title` (it evolves),
/// falling back to the first user prompt. Returns None if the session isn't found.
pub fn session_title(session_id: &str) -> Option<String> {
    let path = locate_session(session_id)?;
    let text = fs::read_to_string(&path).ok()?;
    let mut title: Option<String> = None;
    let mut first_prompt: Option<String> = None;
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match v.get("type").and_then(Value::as_str) {
            Some("ai-title") => {
                if let Some(t) = v.get("aiTitle").and_then(Value::as_str) {
                    title = Some(t.to_string());
                }
            }
            Some("summary") if title.is_none() => {
                if let Some(t) = v.get("summary").and_then(Value::as_str) {
                    title = Some(t.to_string());
                }
            }
            Some("user")
                if first_prompt.is_none()
                    && !v.get("isSidechain").and_then(Value::as_bool).unwrap_or(false) =>
            {
                if let Some(t) = v.get("message").and_then(message_text) {
                    if !t.trim().is_empty() {
                        first_prompt = Some(t);
                    }
                }
            }
            _ => {}
        }
    }
    title.or(first_prompt).map(|s| truncate(&s, 60))
}

fn message_text(message: &Value) -> Option<String> {
    let content = message.get("content")?;
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    content.as_array().and_then(|arr| {
        arr.iter().find_map(|b| {
            (b.get("type").and_then(Value::as_str) == Some("text"))
                .then(|| b.get("text").and_then(Value::as_str).map(str::to_string))
                .flatten()
        })
    })
}

fn truncate(s: &str, max: usize) -> String {
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max {
        collapsed
    } else {
        format!("{}…", collapsed.chars().take(max).collect::<String>())
    }
}

/// Locate a Claude session's JSONL file by id, searching across all project dirs.
pub fn locate_session(session_id: &str) -> Option<PathBuf> {
    let root = projects_root();
    let target = format!("{session_id}.jsonl");
    for entry in fs::read_dir(&root).ok()?.flatten() {
        let dir = entry.path();
        if dir.is_dir() {
            let candidate = dir.join(&target);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
