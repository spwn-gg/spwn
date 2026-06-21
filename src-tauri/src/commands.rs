//! Tauri commands: the frontend → backend contract.
//!
//! Context Manager owns "projects" (a named working directory grouping terminals).
//! A terminal is either a SHELL (rmux pty) or a CLAUDE chat (Agent SDK sidecar).
//! Both run under stable, persistent ids and reattach across restarts.

use crate::pty::{default_shell, find_claude_bin, spawn_rmux_session};
use crate::settings::Settings;
use crate::state::AppState;
use crate::store::{rmux_session_name, ContextBlock, ProjectRec, TerminalRec};
use crate::transcript::{read_transcript as parse_transcript, Turn};
use rmux_sdk::{EnsureSession, EnsureSessionPolicy, Rmux, RmuxBuilder, SessionName, TerminalSizeSpec};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_projects(state: State<AppState>) -> Vec<ProjectRec> {
    let mut projects = state.store.lock().unwrap().projects.clone();
    // Show Claude's own session name (ai-title) for claude terminals.
    for project in &mut projects {
        for terminal in &mut project.terminals {
            if terminal.kind == "claude" {
                if let Some(sid) = &terminal.session_id {
                    if let Some(name) = crate::projects::session_title(sid) {
                        terminal.title = name;
                    }
                }
            }
        }
    }
    projects
}

#[tauri::command]
pub fn create_project(
    state: State<AppState>,
    name: String,
    directory: String,
) -> Result<ProjectRec, String> {
    let rec = ProjectRec {
        id: Uuid::new_v4().to_string(),
        name,
        directory,
        terminals: Vec::new(),
        context: Vec::new(),
    };
    state.store.lock().unwrap().projects.push(rec.clone());
    persist(&state);
    Ok(rec)
}

/// Open a directory in VS Code (Insiders first, then stable), via LaunchServices.
#[tauri::command]
pub fn open_in_vscode(path: String) -> Result<(), String> {
    for app in ["Visual Studio Code - Insiders", "Visual Studio Code"] {
        if let Ok(status) = std::process::Command::new("open")
            .arg("-a")
            .arg(app)
            .arg(&path)
            .status()
        {
            if status.success() {
                return Ok(());
            }
        }
    }
    Err("Visual Studio Code not found".to_string())
}

#[tauri::command]
pub async fn delete_project(state: State<'_, AppState>, project_id: String) -> Result<(), String> {
    let terminal_ids: Vec<String> = {
        let store = state.store.lock().unwrap();
        store
            .project(&project_id)
            .map(|p| p.terminals.iter().map(|t| t.id.clone()).collect())
            .unwrap_or_default()
    };
    kill_terminals(&state, &terminal_ids).await;
    state.store.lock().unwrap().projects.retain(|p| p.id != project_id);
    persist(&state);
    Ok(())
}

// ---------------------------------------------------------------------------
// Context space (composed per project, injected into a new session)
// ---------------------------------------------------------------------------

/// Add a block to a project's context space (kind: "note" | "session").
#[tauri::command]
pub fn add_context_block(
    state: State<AppState>,
    project_id: String,
    kind: String,
    label: String,
    text: String,
) -> Result<(), String> {
    push_block(&state, &project_id, ContextBlock {
        id: Uuid::new_v4().to_string(),
        kind,
        label,
        text,
    })
}

/// Add a file's contents as a context block (capped to keep the prompt sane).
#[tauri::command]
pub fn add_context_file(
    state: State<AppState>,
    project_id: String,
    path: String,
) -> Result<(), String> {
    let content = std::fs::read_to_string(&path).map_err(|e| format!("read {path}: {e}"))?;
    let text = if content.chars().count() > 200_000 {
        content.chars().take(200_000).collect()
    } else {
        content
    };
    let label = Path::new(&path)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    push_block(&state, &project_id, ContextBlock {
        id: Uuid::new_v4().to_string(),
        kind: "file".into(),
        label,
        text,
    })
}

#[tauri::command]
pub fn remove_context_block(
    state: State<AppState>,
    project_id: String,
    block_id: String,
) -> Result<(), String> {
    {
        let mut store = state.store.lock().unwrap();
        if let Some(p) = store.project_mut(&project_id) {
            p.context.retain(|b| b.id != block_id);
        }
    }
    persist(&state);
    Ok(())
}

#[tauri::command]
pub fn clear_context(state: State<AppState>, project_id: String) -> Result<(), String> {
    {
        let mut store = state.store.lock().unwrap();
        if let Some(p) = store.project_mut(&project_id) {
            p.context.clear();
        }
    }
    persist(&state);
    Ok(())
}

fn push_block(state: &AppState, project_id: &str, block: ContextBlock) -> Result<(), String> {
    {
        let mut store = state.store.lock().unwrap();
        let p = store
            .project_mut(project_id)
            .ok_or_else(|| "no such project".to_string())?;
        p.context.push(block);
    }
    persist(state);
    Ok(())
}

// ---------------------------------------------------------------------------
// Terminals
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenTerminalSpec {
    pub project_id: String,
    pub terminal_id: Option<String>,
    /// "shell" | "claude" (for new terminals).
    pub kind: String,
    pub cols: u16,
    pub rows: u16,
    /// Resume this claude session id.
    pub claude_resume: Option<String>,
    /// Fork this claude session id into a new one.
    pub claude_fork: Option<String>,
    /// The terminal a fork/branch originated from (to inherit its group).
    pub parent_terminal_id: Option<String>,
    /// Seed a new Claude session with this composed context: it is pasted into the
    /// input box (not auto-submitted), for the user to review and send.
    pub initial_prompt: Option<String>,
}

#[tauri::command]
pub async fn open_terminal(
    app: AppHandle,
    state: State<'_, AppState>,
    spec: OpenTerminalSpec,
) -> Result<String, String> {
    let (terminal_id, kind, cwd, resume_src, fork) = {
        let mut store = state.store.lock().unwrap();
        let project = store
            .project(&spec.project_id)
            .ok_or_else(|| "no such project".to_string())?
            .clone();

        let existing = spec
            .terminal_id
            .as_deref()
            .and_then(|tid| store.terminal(tid).cloned());
        let terminal_id = spec
            .terminal_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let kind = existing
            .as_ref()
            .map(|t| t.kind.clone())
            .unwrap_or_else(|| spec.kind.clone());
        let cwd = existing
            .as_ref()
            .map(|t| t.cwd.clone())
            .unwrap_or_else(|| project.directory.clone());
        let saved_session = existing.as_ref().and_then(|t| t.session_id.clone());

        // Claude resume/fork resolution (the Claude session runs in a real pty).
        let (resume_src, fork) = if kind == "claude" {
            if let Some(fork_id) = spec.claude_fork.clone() {
                (Some(fork_id), true)
            } else if let Some(r) = spec.claude_resume.clone().or(saved_session.clone()) {
                (Some(r), false)
            } else {
                (None, false)
            }
        } else {
            (None, false)
        };

        if existing.is_none() {
            // Forks/branches inherit their source's group; fresh sessions get None
            // (their own group, keyed by their id).
            let group_id = spec.parent_terminal_id.as_deref().and_then(|pid| {
                store
                    .terminal(pid)
                    .map(|t| t.group_id.clone().unwrap_or_else(|| pid.to_string()))
            });
            if let Some(p) = store.project_mut(&spec.project_id) {
                p.terminals.push(TerminalRec {
                    id: terminal_id.clone(),
                    title: if kind == "claude" { "claude" } else { "shell" }.to_string(),
                    kind: kind.clone(),
                    cwd: cwd.clone(),
                    session_id: None,
                    group_id,
                });
            }
        }
        (terminal_id, kind, cwd, resume_src, fork)
    };
    persist(&state);

    let cwd_path = std::fs::canonicalize(&cwd).unwrap_or_else(|_| PathBuf::from(&cwd));

    // Both shell and claude run in a real pty; only the argv differs.
    let argv = if kind == "claude" {
        let claude_bin = resolved_claude(state.inner())
            .ok_or_else(|| "claude binary not found (set its path in Settings)".to_string())?;
        let mut argv = vec![claude_bin.to_string_lossy().into_owned()];
        if fork {
            if let Some(src) = &resume_src {
                argv.push("--resume".into());
                argv.push(src.clone());
            }
            argv.push("--fork-session".into());
        } else if let Some(src) = &resume_src {
            argv.push("--resume".into());
            argv.push(src.clone());
        }
        // Note: the composed context is NOT passed as a CLI prompt arg (that would
        // auto-submit it). It is pasted into the input box after the TUI is ready
        // (see the paste-after-ready task below), so the user can review/edit and
        // press Enter themselves.
        argv
    } else {
        vec![default_shell(), "-l".to_string()]
    };

    // new/fork claude sessions create a new JSONL whose id we discover on disk.
    let want_discovery = kind == "claude" && (resume_src.is_none() || fork);
    let before = if want_discovery {
        snapshot_session_paths()
    } else {
        HashSet::new()
    };

    let rmux = connect(&state).await?;
    let session_name = rmux_session_name(&terminal_id);
    let session = spawn_rmux_session(
        rmux,
        app.clone(),
        &terminal_id,
        &session_name,
        argv,
        &cwd_path,
        spec.cols,
        spec.rows,
    )
    .await
    .map_err(|e| e.to_string())?;
    // Paste the composed context into the Claude input box without submitting it:
    // wait until the TUI has rendered and gone quiet, then send the text wrapped in
    // bracketed-paste markers (so embedded newlines are inserted literally instead
    // of submitting) with NO trailing Enter. The user reviews and sends it.
    if kind == "claude" {
        if let Some(prompt) = spec.initial_prompt.clone().filter(|p| !p.is_empty()) {
            let pane = session.pane.clone();
            tauri::async_runtime::spawn(async move {
                // Wait for the initial render to settle (prompt-ready), bounded so a
                // never-quiet TUI still gets the paste.
                let _ = pane
                    .wait_until_stable_for(Duration::from_millis(400))
                    .timeout(Duration::from_secs(15))
                    .await;
                let payload = format!("\x1b[200~{prompt}\x1b[201~");
                let _ = pane.send_text(&payload).await;
            });
        }
    }

    state.sessions.lock().unwrap().insert(terminal_id.clone(), session);

    if want_discovery {
        let cwd_str = cwd_path.to_string_lossy().into_owned();
        let tid = terminal_id.clone();
        std::thread::spawn(move || discover_session_id(app, tid, cwd_str, before));
    }

    Ok(terminal_id)
}

/// Detach a terminal tab. Shell sessions stay alive (rmux) for reattach; Claude
/// sidecars are killed (the conversation persists in the session JSONL and
/// reattaches via resume).
#[tauri::command]
pub fn close_terminal(state: State<AppState>, terminal_id: String) -> Result<(), String> {
    if let Some(session) = state.sessions.lock().unwrap().remove(&terminal_id) {
        session.output_task.abort();
    }
    if let Some(mut agent) = state.claude_agents.lock().unwrap().remove(&terminal_id) {
        agent.kill();
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_terminal(
    state: State<'_, AppState>,
    project_id: String,
    terminal_id: String,
) -> Result<(), String> {
    kill_terminals(&state, std::slice::from_ref(&terminal_id)).await;
    {
        let mut store = state.store.lock().unwrap();
        if let Some(p) = store.project_mut(&project_id) {
            p.terminals.retain(|t| t.id != terminal_id);
        }
    }
    persist(&state);
    Ok(())
}

/// Persist a claude session id (from the sidecar init event) onto a terminal.
#[tauri::command]
pub fn set_terminal_session(
    state: State<AppState>,
    project_id: String,
    terminal_id: String,
    session_id: String,
) -> Result<(), String> {
    {
        let mut store = state.store.lock().unwrap();
        if let Some(p) = store.project_mut(&project_id) {
            if let Some(t) = p.terminals.iter_mut().find(|t| t.id == terminal_id) {
                t.session_id = Some(session_id);
            }
        }
    }
    persist(&state);
    Ok(())
}

// ---------------------------------------------------------------------------
// Claude chat I/O
// ---------------------------------------------------------------------------

/// Send a user message to a Claude terminal's sidecar.
#[tauri::command]
pub fn claude_send(state: State<AppState>, terminal_id: String, text: String) -> Result<(), String> {
    let payload = serde_json::json!({ "t": "user", "text": text }).to_string();
    let mut agents = state.claude_agents.lock().unwrap();
    agents
        .get_mut(&terminal_id)
        .ok_or_else(|| "no such claude terminal".to_string())?
        .send_json(&payload)
        .map_err(|e| e.to_string())
}

/// Answer a tool-permission request for a Claude terminal.
#[tauri::command]
pub fn claude_permission(
    state: State<AppState>,
    terminal_id: String,
    id: String,
    allow: bool,
    message: Option<String>,
) -> Result<(), String> {
    let payload = serde_json::json!({
        "t": "permission", "id": id, "allow": allow, "message": message
    })
    .to_string();
    let mut agents = state.claude_agents.lock().unwrap();
    agents
        .get_mut(&terminal_id)
        .ok_or_else(|| "no such claude terminal".to_string())?
        .send_json(&payload)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Shell terminal I/O (rmux)
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn write_to_pty(
    state: State<'_, AppState>,
    pty_id: String,
    data: String,
) -> Result<(), String> {
    let pane = state.sessions.lock().unwrap().get(&pty_id).map(|s| s.pane.clone());
    pane.ok_or_else(|| "no such terminal".to_string())?
        .send_text(&data)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn resize_pty(
    state: State<'_, AppState>,
    pty_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let pane = state.sessions.lock().unwrap().get(&pty_id).map(|s| s.pane.clone());
    pane.ok_or_else(|| "no such terminal".to_string())?
        .resize(TerminalSizeSpec::new(cols.max(1), rows.max(1)))
        .await
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Claude transcript (prior history on reattach)
// ---------------------------------------------------------------------------

/// Auto-detected `claude` path (probe only; ignores the configured override).
/// Used by Settings to show "detected: …".
#[tauri::command]
pub fn find_claude() -> Option<String> {
    find_claude_bin().map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub fn set_settings(state: State<AppState>, settings: Settings) -> Result<(), String> {
    *state.settings.lock().unwrap() = settings;
    let path = state.settings_path.lock().unwrap().clone();
    if let Some(path) = path {
        state
            .settings
            .lock()
            .unwrap()
            .save(&path)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The `claude` binary to use: the configured override (if it exists), else auto-detect.
fn resolved_claude(state: &AppState) -> Option<PathBuf> {
    let configured = state.settings.lock().unwrap().claude_path.clone();
    if let Some(p) = configured.filter(|s| !s.trim().is_empty()) {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    find_claude_bin()
}

#[tauri::command]
pub fn read_transcript(session_id: String) -> Vec<Turn> {
    match crate::projects::locate_session(&session_id) {
        Some(path) => parse_transcript(&path),
        None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn connect(state: &AppState) -> Result<&Rmux, String> {
    state
        .rmux
        .get_or_try_init(|| async {
            RmuxBuilder::new()
                .default_timeout(Duration::from_secs(20))
                .connect_or_start()
                .await
        })
        .await
        .map_err(|e| e.to_string())
}

/// Kill the given terminals (both shell and claude) by id.
async fn kill_terminals(state: &AppState, terminal_ids: &[String]) {
    let mut rmux_ids = Vec::new();
    for tid in terminal_ids {
        if let Some(session) = state.sessions.lock().unwrap().remove(tid) {
            session.output_task.abort();
            rmux_ids.push(tid.clone());
        }
        if let Some(mut agent) = state.claude_agents.lock().unwrap().remove(tid) {
            agent.kill();
        }
    }
    if !rmux_ids.is_empty() {
        if let Ok(rmux) = connect(state).await {
            for tid in rmux_ids {
                if let Ok(name) = SessionName::new(rmux_session_name(&tid)) {
                    if let Ok(session) = EnsureSession::named(name)
                        .policy(EnsureSessionPolicy::ReuseOnly)
                        .ensure(rmux)
                        .await
                    {
                        let _ = session.kill().await;
                    }
                }
            }
        }
    }
}

fn persist(state: &AppState) {
    let path = state.store_path.lock().unwrap().clone();
    if let Some(path) = path {
        let _ = state.store.lock().unwrap().save(&path);
    }
}

/// Snapshot all claude session `*.jsonl` paths across all project dirs.
fn snapshot_session_paths() -> HashSet<PathBuf> {
    let mut set = HashSet::new();
    let Ok(dirs) = std::fs::read_dir(crate::projects::projects_root()) else {
        return set;
    };
    for d in dirs.flatten() {
        let dir = d.path();
        if !dir.is_dir() {
            continue;
        }
        if let Ok(files) = std::fs::read_dir(&dir) {
            for f in files.flatten() {
                let p = f.path();
                if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    set.insert(p);
                }
            }
        }
    }
    set
}

/// The authoritative cwd recorded inside a transcript (first `cwd` field).
fn file_cwd(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    for line in text.lines().take(50) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(c) = v.get("cwd").and_then(|c| c.as_str()) {
                return Some(c.to_string());
            }
        }
    }
    None
}

/// Poll for the new claude session file (created by a new/fork session) matching
/// `cwd`, and emit its id on `pty://session-id/<terminal_id>`.
fn discover_session_id(app: AppHandle, terminal_id: String, cwd: String, before: HashSet<PathBuf>) {
    let event = format!("pty://session-id/{terminal_id}");
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(300));
        let after = snapshot_session_paths();
        for path in after.difference(&before) {
            if file_cwd(path).as_deref() == Some(cwd.as_str()) {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let _ = app.emit(&event, stem.to_string());
                    return;
                }
            }
        }
    }
}
