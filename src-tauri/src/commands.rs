//! Tauri commands: the frontend → backend contract.
//!
//! Context Manager owns "projects" (a named working directory grouping terminals).
//! A terminal is a shell or a `claude` TUI, both running in an rmux pty under
//! stable, persistent ids so they reattach across restarts.

use crate::gitwt;
use crate::pty::{default_shell, find_claude_bin, spawn_rmux_session};
use crate::settings::Settings;
use crate::state::AppState;
use crate::store::{rmux_session_name, ContextBlock, ProjectRec, TerminalRec};
use crate::transcript::{read_transcript as parse_transcript, Turn};
use rmux_sdk::{EnsureSession, EnsureSessionPolicy, Rmux, RmuxBuilder, SessionName, TerminalSizeSpec};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_projects(state: State<AppState>) -> Vec<ProjectRec> {
    let mut projects = state.store.lock().projects.clone();
    // Show Claude's own session name (ai-title) for claude terminals.
    for project in &mut projects {
        for terminal in &mut project.terminals {
            if terminal.kind == "claude" {
                if let Some(sid) = &terminal.session_id {
                    if let Some(name) = cached_session_title(&state, sid) {
                        terminal.title = name;
                    }
                }
            }
        }
    }
    projects
}

/// A session's title, cached by transcript mtime so an unchanged session isn't
/// re-read and re-parsed on every refresh.
fn cached_session_title(state: &AppState, session_id: &str) -> Option<String> {
    let path = crate::projects::locate_session(session_id)?;
    let mtime = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok());
    if let Some(mtime) = mtime {
        if let Some((cached_mtime, title)) = state.title_cache.lock().get(session_id).cloned() {
            if cached_mtime == mtime {
                return Some(title);
            }
        }
    }
    let title = crate::projects::session_title(session_id)?;
    if let Some(mtime) = mtime {
        state
            .title_cache
            .lock()
            .insert(session_id.to_string(), (mtime, title.clone()));
    }
    Some(title)
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
    state.store.lock().projects.push(rec.clone());
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
        let store = state.store.lock();
        store
            .project(&project_id)
            .map(|p| p.terminals.iter().map(|t| t.id.clone()).collect())
            .unwrap_or_default()
    };
    kill_terminals(&state, &terminal_ids).await;
    state.store.lock().projects.retain(|p| p.id != project_id);
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
        let mut store = state.store.lock();
        if let Some(p) = store.project_mut(&project_id) {
            p.context.retain(|b| b.id != block_id);
        }
    }
    persist(&state);
    Ok(())
}

/// Replace the text/label of an existing context block (inline edit).
#[tauri::command]
pub fn update_context_block(
    state: State<AppState>,
    project_id: String,
    block_id: String,
    text: String,
) -> Result<(), String> {
    {
        let mut store = state.store.lock();
        if let Some(p) = store.project_mut(&project_id) {
            if let Some(b) = p.context.iter_mut().find(|b| b.id == block_id) {
                b.text = text;
            }
        }
    }
    persist(&state);
    Ok(())
}

/// Reorder a project's context blocks to match the given id order. Ids not
/// present are ignored; missing ids keep their relative order at the end.
#[tauri::command]
pub fn reorder_context(
    state: State<AppState>,
    project_id: String,
    order: Vec<String>,
) -> Result<(), String> {
    {
        let mut store = state.store.lock();
        if let Some(p) = store.project_mut(&project_id) {
            let rank = |id: &str| order.iter().position(|o| o == id).unwrap_or(usize::MAX);
            p.context.sort_by_key(|b| rank(&b.id));
        }
    }
    persist(&state);
    Ok(())
}

#[tauri::command]
pub fn clear_context(state: State<AppState>, project_id: String) -> Result<(), String> {
    {
        let mut store = state.store.lock();
        if let Some(p) = store.project_mut(&project_id) {
            p.context.clear();
        }
    }
    persist(&state);
    Ok(())
}

fn push_block(state: &AppState, project_id: &str, block: ContextBlock) -> Result<(), String> {
    {
        let mut store = state.store.lock();
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
}

#[tauri::command]
pub async fn open_terminal(
    app: AppHandle,
    state: State<'_, AppState>,
    spec: OpenTerminalSpec,
) -> Result<String, String> {
    let (terminal_id, kind, cwd, resume_src, fork, is_new, project_dir, fork_base) = {
        let mut store = state.store.lock();
        let project = store
            .project(&spec.project_id)
            .ok_or_else(|| "no such project".to_string())?
            .clone();

        let existing = spec
            .terminal_id
            .as_deref()
            .and_then(|tid| store.terminal(tid).cloned());
        // A fork's worktree branches from its parent session's branch.
        let fork_base = spec
            .parent_terminal_id
            .as_deref()
            .and_then(|pid| store.terminal(pid).and_then(|t| t.branch.clone()));
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

        // Claude resume/fork resolution. Fork resumes its source then branches; a
        // plain resume continues a saved session; otherwise it's a fresh session
        // whose id arrives later via the sidecar's `init` event.
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
            // (their own group, keyed by their id). The session id is bound later
            // from the sidecar's `init` event (set_terminal_session).
            let group_id = spec.parent_terminal_id.as_deref().and_then(|pid| {
                store
                    .terminal(pid)
                    .map(|t| t.group_id.clone().unwrap_or_else(|| pid.to_string()))
            });
            // The direct parent in the branch tree (the terminal we forked from).
            let parent_id = spec.parent_terminal_id.clone();
            if let Some(p) = store.project_mut(&spec.project_id) {
                p.terminals.push(TerminalRec {
                    id: terminal_id.clone(),
                    title: if kind == "claude" { "claude" } else { "shell" }.to_string(),
                    kind: kind.clone(),
                    cwd: cwd.clone(),
                    session_id: None,
                    group_id,
                    parent_id,
                    branch: None,
                    base_branch: None,
                });
            }
        }
        let is_new = existing.is_none();
        (
            terminal_id,
            kind,
            cwd,
            resume_src,
            fork,
            is_new,
            project.directory.clone(),
            fork_base,
        )
    };
    persist(&state);

    // A fresh Claude session in a git repo gets its own isolated worktree+branch,
    // so sessions don't clobber each other's files. Falls back to the project dir
    // if the project isn't a git repo or the worktree can't be created.
    let mut cwd = cwd;
    if is_new && kind == "claude" {
        if let Some(repo) = gitwt::repo_root(Path::new(&project_dir)) {
            let base = fork_base.or_else(|| gitwt::current_branch(&repo));
            if let Some(base) = base {
                let short = terminal_id.split('-').next().unwrap_or(terminal_id.as_str());
                let branch = format!("cm/{short}");
                let wt_path = worktrees_dir(&app).join(&terminal_id);
                match gitwt::add_worktree(&repo, &wt_path, &branch, &base) {
                    Ok(()) => {
                        cwd = wt_path.to_string_lossy().into_owned();
                        {
                            let mut store = state.store.lock();
                            if let Some(t) = store.terminal_mut(&terminal_id) {
                                t.cwd = cwd.clone();
                                t.branch = Some(branch);
                                t.base_branch = Some(base);
                            }
                        }
                        persist(&state);
                    }
                    Err(e) => {
                        eprintln!("worktree create failed (using project dir): {e}");
                        if let Some(app2) = state.app.lock().as_ref() {
                            let _ = app2.emit(
                                "store://error",
                                format!("Couldn't create a git worktree for the session; using the project folder. {e}"),
                            );
                        }
                    }
                }
            }
        }
    }

    let cwd_path = std::fs::canonicalize(&cwd).unwrap_or_else(|_| PathBuf::from(&cwd));

    if kind == "claude" {
        // Claude sessions run via the Agent SDK sidecar (a node process), NOT rmux.
        // The chat UI drives it over stdin/stdout; its `init` event supplies the
        // session id (bound by the frontend via set_terminal_session).
        let claude_bin = resolved_claude(state.inner())
            .ok_or_else(|| "claude binary not found (set its path in Settings)".to_string())?;
        let agent = crate::claude::spawn_claude_agent(
            app.clone(),
            &terminal_id,
            &cwd_path,
            resume_src.as_deref(),
            None, // resume_at: per-turn rewind is a v2 feature
            fork,
            &claude_bin,
        )
        .map_err(|e| e.to_string())?;
        state.claude_agents.lock().insert(terminal_id.clone(), agent);
        return Ok(terminal_id);
    }

    // Shell: a real login shell in a persistent rmux pty.
    let argv = vec![default_shell(), "-l".to_string()];
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
    state.sessions.lock().insert(terminal_id.clone(), session);

    Ok(terminal_id)
}

/// Detach a terminal tab. A shell's rmux session stays alive for reattach (we just
/// drop the output task); a Claude sidecar is killed (the conversation persists in
/// its JSONL and reattaches via `--resume`).
#[tauri::command]
pub fn close_terminal(state: State<AppState>, terminal_id: String) -> Result<(), String> {
    if let Some(session) = state.sessions.lock().remove(&terminal_id) {
        session.output_task.abort();
    }
    if let Some(mut agent) = state.claude_agents.lock().remove(&terminal_id) {
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
    // Capture the session's worktree (if any) before dropping its record, so we
    // can remove it. The branch is kept (its commits aren't lost).
    let worktree = {
        let store = state.store.lock();
        let proj_dir = store.project(&project_id).map(|p| p.directory.clone());
        store.terminal(&terminal_id).and_then(|t| {
            t.branch.as_ref()?;
            Some((proj_dir?, t.cwd.clone()))
        })
    };
    {
        let mut store = state.store.lock();
        if let Some(p) = store.project_mut(&project_id) {
            p.terminals.retain(|t| t.id != terminal_id);
        }
    }
    persist(&state);
    if let Some((proj_dir, wt_path)) = worktree {
        if let Some(repo) = gitwt::repo_root(Path::new(&proj_dir)) {
            if let Err(e) = gitwt::remove_worktree(&repo, Path::new(&wt_path)) {
                eprintln!("worktree remove failed: {e}");
            }
        }
    }
    Ok(())
}

/// Merge a session's branch back into its base branch.
#[tauri::command]
pub fn merge_session(
    state: State<AppState>,
    project_id: String,
    terminal_id: String,
) -> Result<String, String> {
    let (proj_dir, branch, base) = {
        let store = state.store.lock();
        let proj_dir = store
            .project(&project_id)
            .map(|p| p.directory.clone())
            .ok_or_else(|| "no such project".to_string())?;
        let t = store
            .terminal(&terminal_id)
            .ok_or_else(|| "no such session".to_string())?;
        let branch = t
            .branch
            .clone()
            .ok_or_else(|| "this session has no git branch to merge".to_string())?;
        let base = t
            .base_branch
            .clone()
            .ok_or_else(|| "this session has no base branch to merge into".to_string())?;
        (proj_dir, branch, base)
    };
    let repo = gitwt::repo_root(Path::new(&proj_dir))
        .ok_or_else(|| "project is not a git repository".to_string())?;
    gitwt::merge_into_base(&repo, &base, &branch)
}

/// Persist a discovered claude session id onto a terminal.
#[tauri::command]
pub fn set_terminal_session(
    state: State<AppState>,
    project_id: String,
    terminal_id: String,
    session_id: String,
) -> Result<(), String> {
    {
        let mut store = state.store.lock();
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
// Claude chat I/O (Agent SDK sidecar)
// ---------------------------------------------------------------------------

/// Send a user turn to a Claude session's sidecar.
#[tauri::command]
pub fn claude_send(state: State<AppState>, terminal_id: String, text: String) -> Result<(), String> {
    let payload = serde_json::json!({ "t": "user", "text": text }).to_string();
    send_to_agent(&state, &terminal_id, &payload)
}

/// Answer a tool-permission request for a Claude session.
#[tauri::command]
pub fn claude_permission(
    state: State<AppState>,
    terminal_id: String,
    id: String,
    allow: bool,
    message: Option<String>,
) -> Result<(), String> {
    let payload =
        serde_json::json!({ "t": "permission", "id": id, "allow": allow, "message": message })
            .to_string();
    send_to_agent(&state, &terminal_id, &payload)
}

/// Change the permission mode live (the Shift-Tab affordance): default → acceptEdits → plan.
#[tauri::command]
pub fn claude_set_mode(
    state: State<AppState>,
    terminal_id: String,
    mode: String,
) -> Result<(), String> {
    let payload = serde_json::json!({ "t": "set_mode", "mode": mode }).to_string();
    send_to_agent(&state, &terminal_id, &payload)
}

/// Interrupt the in-flight turn (Esc).
#[tauri::command]
pub fn claude_interrupt(state: State<AppState>, terminal_id: String) -> Result<(), String> {
    let payload = serde_json::json!({ "t": "interrupt" }).to_string();
    send_to_agent(&state, &terminal_id, &payload)
}

/// Answer an AskUserQuestion picker (id is the tool_use id from the question event).
#[tauri::command]
pub fn claude_answer(
    state: State<AppState>,
    terminal_id: String,
    id: String,
    text: String,
) -> Result<(), String> {
    let payload = serde_json::json!({ "t": "answer", "id": id, "text": text }).to_string();
    send_to_agent(&state, &terminal_id, &payload)
}

/// Rewind a session to an earlier turn: restart its sidecar resumed at `anchor_uuid`,
/// truncating the conversation to that point (later turns become an abandoned branch
/// the transcript no longer renders).
#[tauri::command]
pub fn claude_rewind(
    app: AppHandle,
    state: State<AppState>,
    terminal_id: String,
    anchor_uuid: String,
) -> Result<(), String> {
    let (session_id, cwd) = {
        let store = state.store.lock();
        let t = store
            .terminal(&terminal_id)
            .ok_or_else(|| "no such session".to_string())?;
        let sid = t
            .session_id
            .clone()
            .ok_or_else(|| "this session hasn't started yet".to_string())?;
        (sid, t.cwd.clone())
    };
    let claude_bin = resolved_claude(state.inner())
        .ok_or_else(|| "claude binary not found (set its path in Settings)".to_string())?;
    let cwd_path = std::fs::canonicalize(&cwd).unwrap_or_else(|_| PathBuf::from(&cwd));
    if let Some(mut agent) = state.claude_agents.lock().remove(&terminal_id) {
        agent.kill();
    }
    let agent = crate::claude::spawn_claude_agent(
        app,
        &terminal_id,
        &cwd_path,
        Some(session_id.as_str()),
        Some(anchor_uuid.as_str()),
        false,
        &claude_bin,
    )
    .map_err(|e| e.to_string())?;
    state.claude_agents.lock().insert(terminal_id, agent);
    Ok(())
}

/// Write one JSON-line command to a Claude sidecar's stdin.
fn send_to_agent(state: &AppState, terminal_id: &str, payload: &str) -> Result<(), String> {
    let mut agents = state.claude_agents.lock();
    agents
        .get_mut(terminal_id)
        .ok_or_else(|| "no such claude session".to_string())?
        .send_json(payload)
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
    let pane = state.sessions.lock().get(&pty_id).map(|s| s.pane.clone());
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
    let pane = state.sessions.lock().get(&pty_id).map(|s| s.pane.clone());
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
    state.settings.lock().clone()
}

#[tauri::command]
pub fn set_settings(state: State<AppState>, settings: Settings) -> Result<(), String> {
    *state.settings.lock() = settings;
    let path = state.settings_path.lock().clone();
    if let Some(path) = path {
        state
            .settings
            .lock()
            .save(&path)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The `claude` binary to use: the configured override (if it exists), else auto-detect.
fn resolved_claude(state: &AppState) -> Option<PathBuf> {
    let configured = state.settings.lock().claude_path.clone();
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

/// Permanently kill the given terminals (rmux sessions and/or Claude sidecars) by id.
async fn kill_terminals(state: &AppState, terminal_ids: &[String]) {
    let mut rmux_ids = Vec::new();
    for tid in terminal_ids {
        if let Some(session) = state.sessions.lock().remove(tid) {
            session.output_task.abort();
        }
        if let Some(mut agent) = state.claude_agents.lock().remove(tid) {
            agent.kill();
        }
        rmux_ids.push(tid.clone());
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

/// Where per-session worktrees live (under the app data dir, keyed by terminal id).
fn worktrees_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .map(|d| d.join("worktrees"))
        .unwrap_or_else(|_| PathBuf::from("worktrees"))
}

fn persist(state: &AppState) {
    let Some(path) = state.store_path.lock().clone() else {
        return;
    };
    if let Err(e) = state.store.lock().save(&path) {
        eprintln!("failed to persist projects.json: {e}");
        if let Some(app) = state.app.lock().as_ref() {
            let _ = app.emit("store://error", format!("Couldn't save changes to disk: {e}"));
        }
    }
}

