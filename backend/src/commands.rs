//! Backend commands: the frontend → backend contract, exposed over
//! `POST /api/invoke/:command` (see `server::routes`).
//!
//! spwn owns "projects" (a named working directory grouping terminals).
//! A terminal is a shell or a `claude` TUI, both running in an rmux pty under
//! stable, persistent ids so they reattach across restarts.

use crate::checkpoints::{self, CheckpointMeta};
use crate::gitwt;
use crate::hooks;
use crate::pty::{default_shell, spawn_pane};
use crate::settings::{Settings, WorktreeLocation};
use crate::state::AppState;
use crate::store::{rmux_session_name, ContextBlock, ProjectRec, ScheduledTask, TerminalRec};
use crate::transcript::{read_transcript as parse_transcript, Turn};
use rmux_sdk::{EnsureSession, EnsureSessionPolicy, Rmux, RmuxBuilder, SessionName, TerminalSizeSpec};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

pub fn list_projects(state: &AppState) -> Vec<ProjectRec> {
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

pub fn create_project(
    state: &AppState,
    name: String,
    directory: String,
) -> Result<ProjectRec, String> {
    let rec = ProjectRec {
        id: Uuid::new_v4().to_string(),
        name,
        directory,
        terminals: Vec::new(),
        context: Vec::new(),
        scheduled_tasks: Vec::new(),
    };
    state.store.lock().projects.push(rec.clone());
    persist(&state);
    Ok(rec)
}

/// Open a directory in VS Code (Insiders first, then stable), via LaunchServices.
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

pub async fn delete_project(state: &AppState, project_id: String) -> Result<(), String> {
    let terminal_ids: Vec<String> = {
        let store = state.store.lock();
        store
            .project(&project_id)
            .map(|p| p.terminals.iter().map(|t| t.id.clone()).collect())
            .unwrap_or_default()
    };
    // Delete each session properly instead of just dropping the project record.
    // `delete_terminal` kills the pane, fires both `session-deleted` scopes — the
    // repo one for the user's own teardown (a container, say), then the global one
    // that removes the worktree and its branch — and prunes the session's
    // checkpoints. Skipping it left a worktree per session on disk, a `spwn/…` branch
    // per session in the repo, and would now strand a container that a restart policy
    // brings back on every Docker restart.
    //
    // This DOES delete the session branches, and with them any commits that were only
    // ever on them. That's the same thing deleting each session by hand would do; the
    // UI warns about unmerged work before getting here.
    for tid in &terminal_ids {
        if let Err(e) = delete_terminal(state, project_id.clone(), tid.clone()).await {
            // One session's teardown failing must not strand the rest, or the project.
            emit_store_error(state, &format!("could not fully delete a session: {e}"));
        }
    }
    state.store.lock().projects.retain(|p| p.id != project_id);
    persist(&state);
    Ok(())
}

// ---------------------------------------------------------------------------
// Context space (composed per project, injected into a new session)
// ---------------------------------------------------------------------------

/// Add a block to a project's context space (kind: "note" | "session").
pub fn add_context_block(
    state: &AppState,
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
pub fn add_context_file(
    state: &AppState,
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

pub fn remove_context_block(
    state: &AppState,
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
pub fn update_context_block(
    state: &AppState,
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
pub fn reorder_context(
    state: &AppState,
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

pub fn clear_context(state: &AppState, project_id: String) -> Result<(), String> {
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
    /// `"shell"` | `"agent"` | `"claude"` (legacy sidecar), for new terminals.
    pub kind: String,
    /// Which agent definition to run when `kind == "agent"`. Defaults to the
    /// configured default agent, then the first installed one.
    #[serde(default)]
    pub agent: Option<String>,
    pub cols: u16,
    pub rows: u16,
    /// Resume this session id.
    pub claude_resume: Option<String>,
    /// Fork this session id into a new one.
    pub claude_fork: Option<String>,
    /// The terminal a fork/branch originated from (to inherit its group).
    pub parent_terminal_id: Option<String>,
    /// Initial permission/execution mode, applied at launch so the first turn can't
    /// run under the wrong one (a race a post-spawn change would lose).
    pub permission_mode: Option<String>,
}

pub async fn open_terminal(
    state: Arc<AppState>,
    spec: OpenTerminalSpec,
) -> Result<String, String> {
    let (terminal_id, kind, agent_id, cwd, resume_src, fork, is_new, project_dir, fork_base) = {
        let mut store = state.store.lock();
        let project = store
            .project(&spec.project_id)
            .ok_or_else(|| "no such project".to_string())?
            .clone();

        let existing = spec
            .terminal_id
            .as_deref()
            .and_then(|tid| store.terminal(tid).cloned());
        // A fork's worktree branches from its parent session's branch, so the code
        // tree mirrors the conversation tree.
        let fork_base = spec
            .parent_terminal_id
            .as_deref()
            .and_then(|pid| store.terminal(pid).and_then(|t| t.branch.clone()));
        // A shell opened against a session joins that session's environment, so
        // "open a terminal here" lands inside the container the agent is working in.
        //
        // Only shells: an agent FORK gets its own worktree, so inheriting the parent's
        // prefix would exec it into a container with the wrong worktree mounted. The
        // fork's own `session-created` hook reports its own environment below.
        let inherited_exec = (spec.kind != "agent" && spec.kind != "claude")
            .then(|| {
                spec.parent_terminal_id
                    .as_deref()
                    .and_then(|pid| store.terminal(pid).and_then(|t| t.exec.clone()))
            })
            .flatten();
        let terminal_id = spec
            .terminal_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let kind = existing
            .as_ref()
            .map(|t| t.kind.clone())
            .unwrap_or_else(|| spec.kind.clone());
        // Reattaching keeps whatever agent the session was created with; a new one
        // takes the caller's choice, else the configured default, else the first
        // installed agent. Resolved here so the record and the launch agree.
        let agent_id = if kind == "agent" {
            existing.as_ref().and_then(|t| t.agent.clone()).or_else(|| {
                let overrides = state.settings.lock().agent_paths.clone();
                let preferred = spec
                    .agent
                    .clone()
                    .or_else(|| state.settings.lock().default_agent.clone());
                state
                    .agents
                    .lock()
                    .default_id(preferred.as_deref(), &overrides)
            })
        } else {
            None
        };
        // Reattaching uses the stored cwd (a Claude session's own worktree, if it
        // has one); a fresh session starts from the project dir until its worktree
        // is created below.
        let cwd = existing
            .as_ref()
            .map(|t| t.cwd.clone())
            .unwrap_or_else(|| project.directory.clone());
        let saved_session = existing.as_ref().and_then(|t| t.session_id.clone());

        // Claude resume/fork resolution. Fork resumes its source then branches; a
        // plain resume continues a saved session; otherwise it's a fresh session
        // whose id arrives later via the sidecar's `init` event.
        let (resume_src, fork) = if kind == "claude" || kind == "agent" {
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
            let title = match kind.as_str() {
                "claude" => "claude".to_string(),
                "agent" => agent_id.clone().unwrap_or_else(|| "agent".to_string()),
                _ => "shell".to_string(),
            };
            if let Some(p) = store.project_mut(&spec.project_id) {
                p.terminals.push(TerminalRec {
                    id: terminal_id.clone(),
                    title,
                    kind: kind.clone(),
                    agent: agent_id.clone(),
                    cwd: cwd.clone(),
                    session_id: None,
                    group_id,
                    parent_id,
                    branch: None,
                    base_branch: None,
                    needs_attention: false,
                    attention_reason: None,
                    // A shell inherits its session's environment; an agent session
                    // gets its own below, from `setup_session_worktree`.
                    exec: inherited_exec,
                });
            }
        }
        let is_new = existing.is_none();
        (
            terminal_id,
            kind,
            agent_id,
            cwd,
            resume_src,
            fork,
            is_new,
            project.directory.clone(),
            fork_base,
        )
    };
    persist(&state);

    // A fresh Claude session in a git repo gets its own isolated worktree+branch, so
    // sessions can run concurrently without clobbering each other's files. Heavy
    // gitignored build dirs are COW-cloned in so the agent can build immediately.
    // Falls back to the project dir if it's not a git repo or the worktree fails.
    let mut cwd = cwd;
    if is_new && (kind == "claude" || kind == "agent") {
        if let Some(repo) = gitwt::repo_root(Path::new(&project_dir)) {
            if let Some(base) = fork_base.or_else(|| gitwt::current_branch(&repo)) {
                // Create the worktree via the `session-created` hooks (native fallback
                // inside). Interactive → hooks may raise UI prompts (headless = false).
                if let Some(new_cwd) =
                    setup_session_worktree(&state, &terminal_id, &project_dir, &repo, base, false)
                {
                    cwd = new_cwd;
                }
            }
        }
    }

    let cwd_path = std::fs::canonicalize(&cwd).unwrap_or_else(|_| PathBuf::from(&cwd));

    // The environment a hook stood up for this session, if any. Read AFTER
    // `setup_session_worktree`, which is what records it.
    let exec = state
        .store
        .lock()
        .terminal(&terminal_id)
        .and_then(|t| t.exec.clone());

    // Everything else runs in an rmux pane: a login shell, or an agent's real TUI.
    let (argv, env, runtime) = if kind == "agent" {
        let agent_id = agent_id
            .clone()
            .ok_or_else(|| "no agent available (none installed?)".to_string())?;
        let overrides = state.settings.lock().agent_paths.clone();
        let def = state
            .agents
            .lock()
            .get(&agent_id)
            .cloned()
            .ok_or_else(|| format!("unknown agent '{agent_id}'"))?;
        // Inside an environment the binary is resolved by ITS PATH, not the host's:
        // the host path is a macOS binary that cannot exec in a Linux container, and
        // on a host with no agent installed at all — the whole point of containerizing
        // — `resolve_binary` would fail here and the session would never launch.
        // `agent_paths` overrides are host paths too, so they don't apply either.
        let bin = match &exec {
            Some(e) => e.bin.clone().unwrap_or_else(|| def.binary.name.clone()),
            None => crate::agents::resolve_binary(&def, &overrides)
                .ok_or_else(|| format!("{} binary not found (set its path in Settings)", def.name))?
                .to_string_lossy()
                .into_owned(),
        };

        // Assign the session id up front when the agent supports it. This is what
        // makes binding synchronous: the transcript path is known before the process
        // starts, so there is no window where a session exists but can't be found.
        let session_id = match def.session.id_strategy {
            crate::agents::def::IdStrategy::Assign => Some(
                resume_src
                    .clone()
                    .filter(|_| !fork)
                    .unwrap_or_else(|| Uuid::new_v4().to_string()),
            ),
            _ => resume_src.clone(),
        };

        let template = if fork {
            def.argv.fork.as_ref().unwrap_or(&def.argv.new)
        } else if resume_src.is_some() {
            &def.argv.resume
        } else {
            &def.argv.new
        };

        let permission_mode = spec
            .permission_mode
            .as_deref()
            .and_then(|m| def.modes.launch.get(m).cloned())
            .unwrap_or_default();
        let ctx: std::collections::BTreeMap<&str, String> = [
            ("bin", bin),
            ("sessionId", session_id.clone().unwrap_or_default()),
            ("sourceSessionId", resume_src.clone().unwrap_or_default()),
            ("permissionMode", permission_mode),
            ("cwd", cwd_path.to_string_lossy().into_owned()),
        ]
        .into_iter()
        .collect();

        let argv = crate::agents::def::render_argv(template, &ctx);
        let env: Vec<String> = def.env.iter().map(|(k, v)| format!("{k}={v}")).collect();

        // Bind immediately for an assigned id — the record is what the transcript,
        // checkpoints and Timeline all key off.
        if let Some(sid) = session_id.clone() {
            if !fork {
                bind_session(&state, &terminal_id, &sid);
            }
        }

        (
            argv,
            env,
            Some(crate::pty::AgentRuntime {
                agent_id,
                session_id,
                mode: parking_lot::Mutex::new(spec.permission_mode.clone()),
            }),
        )
    } else {
        // The host's login shell (`$SHELL`, typically /bin/zsh on macOS) generally
        // doesn't exist in a Linux image, and `-l` isn't portable across shells —
        // so inside an environment use its own shell, plain.
        match &exec {
            Some(e) => (
                vec![e.shell.clone().unwrap_or_else(|| "/bin/sh".to_string())],
                Vec::new(),
                None,
            ),
            None => (vec![default_shell(), "-l".to_string()], Vec::new(), None),
        }
    };

    // Check the wrapper itself is runnable before launching into it.
    //
    // Deliberately NOT "run the prefix and see if it succeeds": the recommended
    // prefix allocates a tty (`docker exec -it`), and docker refuses that when its
    // own stdin isn't one — so an execution probe would report every healthy
    // environment as broken. What IS checkable without a pty is that argv[0]
    // resolves, which catches the failure this design is most exposed to: panes are
    // launched exec-style against the long-lived rmux daemon's PATH, not the hook's,
    // so a bare `docker` can be missing here while working fine in the hook.
    //
    // A container that has since been removed still surfaces as a pane that exits;
    // recovery is the Hooks panel's Run button on `session-created`.
    let exec_prefix = match &exec {
        None => None,
        Some(e) => {
            let argv = crate::pty::split_exec_prefix(&e.prefix);
            if let Some(argv) = &argv {
                if !wrapper_is_runnable(&argv[0]) {
                    emit_store_error(
                        &state,
                        &format!(
                            "this session's environment runs `{}`, which isn't on the PATH \
                             spwn launches panes with — the pane will not start",
                            argv[0]
                        ),
                    );
                }
            }
            argv
        }
    };

    let rmux = connect(&state).await?;
    let session_name = rmux_session_name(&terminal_id);
    let session = spawn_pane(
        rmux,
        state.hub.clone(),
        crate::pty::SpawnSpec {
            id: &terminal_id,
            session_name: &session_name,
            argv,
            cwd: &cwd_path,
            cols: spec.cols,
            rows: spec.rows,
            env,
            agent: runtime,
            exec_prefix,
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    // Opening the same terminal twice (two browser tabs, or a reload racing the
    // unmount) would otherwise leak the previous forwarding task, which keeps
    // emitting to the same topic — the client then sees every byte twice.
    let (pane_handle, activity) = (session.pane.clone(), Arc::clone(&session.activity));
    let is_agent_pane = session.agent.is_some();
    if let Some(prev) = state.sessions.lock().insert(terminal_id.clone(), session) {
        prev.output_task.abort();
    }

    if is_agent_pane {
        if let Some(def) = agent_id
            .as_ref()
            .and_then(|id| state.agents.lock().get(id).cloned())
        {
            // Seed the turn tracker with whatever turn the transcript already rests
            // on, so reattaching to an old conversation doesn't fire a commit and a
            // checkpoint for a turn that finished days ago.
            if let Some(sid) = state.store.lock().terminal(&terminal_id).and_then(|t| t.session_id.clone()) {
                if let Some(path) = crate::projects::locate_session(&sid) {
                    if let Some(u) = crate::transcript::tail_summary(&path).last_uuid {
                        state.turns.lock().prime(&terminal_id, &u);
                    }
                }
            }
            crate::agents::status::spawn_watcher(
                Arc::clone(&state),
                terminal_id.clone(),
                def,
                pane_handle,
                activity,
            );
        }
    }

    Ok(terminal_id)
}

/// Detach a terminal tab. A shell's rmux session stays alive for reattach (we just
/// drop the output task); a Claude sidecar is killed (the conversation persists in
/// its JSONL and reattaches via `--resume`).
pub fn close_terminal(state: &AppState, terminal_id: String) -> Result<(), String> {
    if let Some(session) = state.sessions.lock().remove(&terminal_id) {
        session.output_task.abort();
    }
    // An rmux pane survives detach, but its watcher does not (it exits once the
    // pane leaves `sessions`), so drop the live status with it.
    clear_agent_status(state, &terminal_id);
    Ok(())
}

pub async fn delete_terminal(
    state: &AppState,
    project_id: String,
    terminal_id: String,
) -> Result<(), String> {
    kill_terminals(&state, std::slice::from_ref(&terminal_id)).await;
    // Capture the session id (to prune checkpoints) and its worktree + branch (to
    // remove both) before dropping the record.
    let (session_id, worktree, base_branch, exec, proj_dir, cwd) = {
        let store = state.store.lock();
        let proj_dir = store.project(&project_id).map(|p| p.directory.clone());
        let t = store.terminal(&terminal_id);
        let wt = t.and_then(|t| {
            let branch = t.branch.clone()?;
            Some((proj_dir.clone()?, t.cwd.clone(), branch))
        });
        (
            t.and_then(|t| t.session_id.clone()),
            wt,
            t.and_then(|t| t.base_branch.clone()),
            t.and_then(|t| t.exec.clone()),
            proj_dir,
            t.map(|t| t.cwd.clone()),
        )
    };
    {
        let mut store = state.store.lock();
        if let Some(p) = store.project_mut(&project_id) {
            p.terminals.retain(|t| t.id != terminal_id);
        }
    }
    persist(&state);
    // A session with an environment but NO worktree (a non-git project, or one whose
    // worktree creation failed) would otherwise never fire teardown, leaking whatever
    // its `session-created` hook stood up. Fire only the REPO scope for it: the global
    // scope is worktree removal, and handing that script a session whose "worktree" is
    // really the project dir invites `git worktree remove` on a directory the user
    // very much still wants.
    if worktree.is_none() {
        if let (Some(e), Some(dir), Some(cwd)) = (&exec, &proj_dir, &cwd) {
            let ctx = hooks::HookCtx {
                terminal_id: terminal_id.clone(),
                project_dir: dir.clone(),
                worktree: PathBuf::from(cwd),
                branch: None,
                base_branch: base_branch.clone(),
                session_id: session_id.clone(),
                turn_uuid: None,
                exec: Some(e.prefix.clone()),
            };
            fire_hooks_scope(&state, &ctx, "session-deleted", hooks::Scope::Repo, false);
        }
    }
    if let Some((proj_dir, wt_path, branch)) = worktree {
        let ctx = hooks::HookCtx {
            terminal_id: terminal_id.clone(),
            project_dir: proj_dir.clone(),
            worktree: PathBuf::from(&wt_path),
            branch: Some(branch.clone()),
            base_branch,
            session_id: session_id.clone(),
            turn_uuid: None,
            exec: exec.map(|e| e.prefix),
        };
        // Repo `session-deleted` hook runs FIRST, inside the worktree (user cleanup that
        // must happen before the tree disappears) — synchronously, like all hooks.
        fire_hooks_scope(&state, &ctx, "session-deleted", hooks::Scope::Repo, false);
        // Then the GLOBAL `session-deleted` script (runs in the project dir) removes the
        // worktree + branch. Worktree removal lives entirely in the hook — if global
        // hooks are disabled or the script was deleted, the worktree/branch is left in
        // place (spwn no longer manages it); the user can prune it with git.
        fire_hooks_scope(&state, &ctx, "session-deleted", hooks::Scope::Global, false);
    }
    state.hook_runs.lock().remove(&terminal_id);
    state.hooks_running.lock().remove(&terminal_id);
    if let (Some(sid), Some(app_data)) = (session_id, app_data_dir(&state)) {
        checkpoints::remove_session(&app_data, &sid);
    }
    Ok(())
}

/// Merge a session's branch back into its base branch (manual, user-triggered).
pub fn merge_session(
    state: &AppState,
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

/// A preview of what merging a session's branch into its base would do.
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MergeStatus {
    /// The session's branch (None if it has no worktree branch — nothing to merge).
    pub branch: Option<String>,
    /// The branch it would merge into (its parent/base branch).
    pub base_branch: Option<String>,
    /// Commits on the session branch not yet in the base.
    pub ahead: u32,
    /// Files the session branch introduces relative to the base.
    pub changed_files: Vec<String>,
    /// The session worktree has uncommitted changes (they won't be part of the merge
    /// until the next turn commits them).
    pub uncommitted: bool,
    /// A human-readable reason the merge can't proceed right now (base branch isn't
    /// checked out, or its checkout is dirty). None when the merge is ready.
    pub blocker: Option<String>,
}

/// Compute a merge preview for a session: target branch, how far ahead it is, which
/// files it changes, and whether anything blocks the merge.
pub fn session_merge_status(
    state: &AppState,
    project_id: String,
    terminal_id: String,
) -> Result<MergeStatus, String> {
    let (proj_dir, branch, base, cwd) = {
        let store = state.store.lock();
        let proj_dir = store
            .project(&project_id)
            .map(|p| p.directory.clone())
            .ok_or_else(|| "no such project".to_string())?;
        let t = store
            .terminal(&terminal_id)
            .ok_or_else(|| "no such session".to_string())?;
        (proj_dir, t.branch.clone(), t.base_branch.clone(), t.cwd.clone())
    };
    // No worktree branch → nothing to merge.
    let (Some(branch), Some(base)) = (branch, base) else {
        return Ok(MergeStatus::default());
    };
    let Some(repo) = gitwt::repo_root(Path::new(&proj_dir)) else {
        return Ok(MergeStatus::default());
    };
    let wt = Path::new(&cwd);
    let ahead = gitwt::count_commits(wt, &format!("{base}..{branch}"));
    let changed_files = gitwt::changed_files(wt, &base, &branch);
    let uncommitted = !gitwt::is_clean(wt);
    // Mirror merge_into_base's preconditions so the panel can warn ahead of time.
    let blocker = match gitwt::worktree_for_branch(&repo, &base) {
        None => Some(format!(
            "'{base}' isn't checked out anywhere — check it out (e.g. in your project folder) to merge into it."
        )),
        Some(base_wt) if !gitwt::is_clean(&base_wt) => Some(format!(
            "The checkout of '{base}' has uncommitted changes — commit or stash them first."
        )),
        Some(_) => None,
    };
    Ok(MergeStatus {
        branch: Some(branch),
        base_branch: Some(base),
        ahead,
        changed_files,
        uncommitted,
        blocker,
    })
}

/// Commit a session's working changes onto its worktree branch, so the branch
/// carries real history to merge/fork from. No-op (Ok) if the session has no
/// worktree branch or nothing changed.
pub fn commit_session_turn(
    state: &AppState,
    terminal_id: String,
    message: String,
) -> Result<(), String> {
    let cwd = {
        let store = state.store.lock();
        match store.terminal(&terminal_id) {
            Some(t) if t.branch.is_some() => t.cwd.clone(),
            _ => return Ok(()),
        }
    };
    gitwt::commit_all(Path::new(&cwd), &message).map(|_| ())
}

// ---------------------------------------------------------------------------
// Source Control (VS Code-style git for a project's *main* checkout).
// These act on `project.directory`, distinct from per-session worktrees.
// ---------------------------------------------------------------------------

/// A project's git repo status: current branch, upstream tracking, how far
/// ahead/behind it is, and whether the working tree is dirty.
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RepoStatus {
    /// Whether the project directory is inside a git repository at all.
    pub is_repo: bool,
    /// Current branch (None on detached HEAD or when not a repo).
    pub branch: Option<String>,
    /// Upstream tracking branch, e.g. "origin/main" (None if none configured).
    pub upstream: Option<String>,
    /// Commits ahead of upstream.
    pub ahead: u32,
    /// Commits behind upstream.
    pub behind: u32,
    /// Working tree has staged/unstaged changes.
    pub dirty: bool,
}

/// Local + remote branches for a project's repo, with the current one flagged.
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GitBranches {
    pub current: Option<String>,
    pub local: Vec<String>,
    pub remote: Vec<String>,
}

/// Resolve a project's directory path from the store.
fn project_dir(state: &AppState, project_id: &str) -> Result<String, String> {
    state
        .store
        .lock()
        .project(project_id)
        .map(|p| p.directory.clone())
        .ok_or_else(|| "no such project".to_string())
}

/// The git status of a project's main checkout (safe to call on any project —
/// returns `is_repo: false` when the directory isn't a git repo).
pub fn git_repo_status(state: &AppState, project_id: String) -> Result<RepoStatus, String> {
    let dir = project_dir(&state, &project_id)?;
    let dir = Path::new(&dir);
    if gitwt::repo_root(dir).is_none() {
        return Ok(RepoStatus::default());
    }
    let (ahead, behind) = gitwt::ahead_behind(dir);
    Ok(RepoStatus {
        is_repo: true,
        branch: gitwt::current_branch(dir),
        upstream: gitwt::upstream_branch(dir),
        ahead,
        behind,
        dirty: !gitwt::is_clean(dir),
    })
}

/// List a project's local and remote-tracking branches.
pub fn git_branches(state: &AppState, project_id: String) -> Result<GitBranches, String> {
    let dir = project_dir(&state, &project_id)?;
    let dir = Path::new(&dir);
    if gitwt::repo_root(dir).is_none() {
        return Ok(GitBranches::default());
    }
    Ok(GitBranches {
        current: gitwt::current_branch(dir),
        local: gitwt::local_branches(dir)?,
        remote: gitwt::remote_branches(dir)?,
    })
}

/// Check out an existing branch in a project's main checkout.
pub fn git_checkout(
    state: &AppState,
    project_id: String,
    branch: String,
) -> Result<(), String> {
    let dir = project_dir(&state, &project_id)?;
    gitwt::checkout_branch(Path::new(&dir), &branch)
}

/// Create a new branch off HEAD and switch to it.
pub fn git_create_branch(
    state: &AppState,
    project_id: String,
    name: String,
) -> Result<(), String> {
    let dir = project_dir(&state, &project_id)?;
    gitwt::create_branch(Path::new(&dir), &name)
}

/// Run a blocking git-network op off the async runtime's worker threads. The
/// project dir is resolved (and cloned) up front so the store `Mutex` is never
/// held across the await.
async fn run_net<F>(
    state: &AppState,
    project_id: String,
    op: F,
) -> Result<String, String>
where
    F: FnOnce(&Path) -> Result<String, String> + Send + 'static,
{
    let dir = project_dir(state, &project_id)?;
    tokio::task::spawn_blocking(move || op(Path::new(&dir)))
        .await
        .map_err(|e| format!("git task failed: {e}"))?
}

/// Fetch all remotes for a project's repo.
pub async fn git_fetch(state: &AppState, project_id: String) -> Result<String, String> {
    run_net(state, project_id, |d| gitwt::fetch(d)).await
}

/// Fast-forward-only pull.
pub async fn git_pull(state: &AppState, project_id: String) -> Result<String, String> {
    run_net(state, project_id, |d| gitwt::pull(d)).await
}

/// Push the current branch (setting upstream if it has none yet).
pub async fn git_push(state: &AppState, project_id: String) -> Result<String, String> {
    run_net(state, project_id, |d| {
        let set_upstream = gitwt::upstream_branch(d).is_none();
        gitwt::push(d, set_upstream)
    })
    .await
}

/// VS Code "Sync": fetch, fast-forward pull, then push. Stops at the first error.
pub async fn git_sync(state: &AppState, project_id: String) -> Result<String, String> {
    run_net(state, project_id, |d| {
        gitwt::fetch(d)?;
        gitwt::pull(d)?;
        let set_upstream = gitwt::upstream_branch(d).is_none();
        gitwt::push(d, set_upstream)?;
        Ok("Synced.".to_string())
    })
    .await
}

/// Persist a discovered claude session id onto a terminal (looked up by id across
/// all projects, so headless/scheduled runs can bind without knowing the project).
pub(crate) fn bind_session(state: &AppState, terminal_id: &str, session_id: &str) {
    let newly_bound = {
        let mut store = state.store.lock();
        match store.terminal_mut(terminal_id) {
            Some(t) => {
                let was_unset = t.session_id.is_none();
                t.session_id = Some(session_id.to_string());
                was_unset
            }
            None => false,
        }
    };
    persist(state);
    // On the first binding, run the project `session-ready` hook (synchronous).
    // Only for sessions that have a worktree (a branch) to discover hooks in.
    if newly_bound {
        if let Some(ctx) = hook_ctx_by_id(state, terminal_id) {
            if ctx.branch.is_some() {
                fire_hooks(state, &ctx, "session-ready", false);
            }
        }
    }
}

/// Persist a discovered claude session id onto a terminal.
pub fn set_terminal_session(
    state: &AppState,
    project_id: String,
    terminal_id: String,
    session_id: String,
) -> Result<(), String> {
    let _ = project_id; // terminal ids are globally unique; kept for the FE contract
    bind_session(state, &terminal_id, &session_id);
    Ok(())
}

// ---------------------------------------------------------------------------
// Scheduled tasks (per-project, headless read-only runs on a daily/weekly cadence)
// ---------------------------------------------------------------------------

pub fn add_scheduled_task(
    state: &AppState,
    project_id: String,
    name: String,
    prompt: String,
    time: String,
    weekdays: Vec<u8>,
    use_context: bool,
) -> Result<ScheduledTask, String> {
    let task = ScheduledTask {
        id: Uuid::new_v4().to_string(),
        name,
        prompt,
        time,
        weekdays,
        enabled: true,
        use_context,
        last_run: None,
    };
    {
        let mut store = state.store.lock();
        let p = store
            .project_mut(&project_id)
            .ok_or_else(|| "no such project".to_string())?;
        p.scheduled_tasks.push(task.clone());
    }
    persist(&state);
    Ok(task)
}

#[allow(clippy::too_many_arguments)]
pub fn update_scheduled_task(
    state: &AppState,
    project_id: String,
    task_id: String,
    name: String,
    prompt: String,
    time: String,
    weekdays: Vec<u8>,
    use_context: bool,
    enabled: bool,
) -> Result<(), String> {
    {
        let mut store = state.store.lock();
        let p = store
            .project_mut(&project_id)
            .ok_or_else(|| "no such project".to_string())?;
        let t = p
            .scheduled_tasks
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or_else(|| "no such task".to_string())?;
        t.name = name;
        t.prompt = prompt;
        t.time = time;
        t.weekdays = weekdays;
        t.use_context = use_context;
        t.enabled = enabled;
    }
    persist(&state);
    Ok(())
}

pub fn set_scheduled_task_enabled(
    state: &AppState,
    project_id: String,
    task_id: String,
    enabled: bool,
) -> Result<(), String> {
    {
        let mut store = state.store.lock();
        if let Some(p) = store.project_mut(&project_id) {
            if let Some(t) = p.scheduled_tasks.iter_mut().find(|t| t.id == task_id) {
                t.enabled = enabled;
            }
        }
    }
    persist(&state);
    Ok(())
}

pub fn remove_scheduled_task(
    state: &AppState,
    project_id: String,
    task_id: String,
) -> Result<(), String> {
    {
        let mut store = state.store.lock();
        if let Some(p) = store.project_mut(&project_id) {
            p.scheduled_tasks.retain(|t| t.id != task_id);
        }
    }
    persist(&state);
    Ok(())
}

/// Fire a scheduled task immediately (the "Run now" button). Reuses the same
/// headless path as the scheduler tick.
pub fn run_scheduled_task_now(
    state: Arc<AppState>,
    project_id: String,
    task_id: String,
) -> Result<(), String> {
    crate::scheduler::fire(&state, &project_id, &task_id);
    Ok(())
}

/// Clear the persisted attention flag on a terminal (called when its session is viewed).
pub fn clear_terminal_attention(state: &AppState, terminal_id: String) -> Result<(), String> {
    {
        let mut store = state.store.lock();
        if let Some(t) = store.terminal_mut(&terminal_id) {
            t.needs_attention = false;
            t.attention_reason = None;
        }
    }
    // Drop any live "needs you" status too, so the sidebar clears immediately
    // rather than waiting for the pane's next observable change.
    {
        let mut map = state.agent_status.lock();
        if !matches!(map.get(&terminal_id), Some(crate::agents::SessionStatus::Thinking)) {
            map.remove(&terminal_id);
        }
    }
    persist(&state);
    Ok(())
}

// ---------------------------------------------------------------------------
// Code checkpoints (APFS-clone snapshots of the project dir)
// ---------------------------------------------------------------------------

/// The app data dir (parent of projects.json).
pub(crate) fn app_data_dir(state: &AppState) -> Option<PathBuf> {
    state
        .store_path
        .lock()
        .clone()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

/// Where per-session worktrees live under the app data dir (the legacy `appData`
/// layout), keyed by terminal id.
pub(crate) fn worktrees_dir(state: &AppState) -> Option<PathBuf> {
    app_data_dir(state).map(|d| d.join("worktrees"))
}

/// The full worktree path for a session in `repo`, honoring the configured
/// `worktreeLocation` setting. Performs any side effects the chosen layout needs
/// (registering the in-repo dir in `.git/info/exclude` for the `internal` layout).
/// Returns None only when the legacy app-data dir can't be resolved.
pub(crate) fn session_worktree_path(
    state: &AppState,
    repo: &Path,
    terminal_id: &str,
) -> Option<PathBuf> {
    let base = match state.settings.lock().worktree_location {
        WorktreeLocation::Sibling => gitwt::sibling_worktrees_dir(repo),
        WorktreeLocation::Internal => {
            // Exclude only the generated worktrees dir, not all of `.spwn/` — so
            // committed hooks under `.spwn/hooks/` stay tracked and reach worktrees.
            gitwt::ensure_git_excludes(repo, "/.spwn/worktrees/");
            gitwt::internal_worktrees_dir(repo)
        }
        WorktreeLocation::AppData => worktrees_dir(state)?,
    };
    Some(base.join(terminal_id))
}

// ---------------------------------------------------------------------------
// Project shell hooks (discovered scripts run on session lifecycle events)
// ---------------------------------------------------------------------------

/// Emit an advisory error toast to the UI (non-fatal; the session continues).
fn emit_store_error(state: &AppState, msg: &str) {
    state.hub.emit("store://error", msg.to_string());
}

/// Build a hook context from a session's worktree + owning project dir, reading its
/// branch/base/session id from the store.
fn hook_ctx(
    state: &AppState,
    terminal_id: &str,
    project_dir: &str,
    worktree: &Path,
) -> hooks::HookCtx {
    let (branch, base_branch, session_id, exec) = {
        let store = state.store.lock();
        match store.terminal(terminal_id) {
            Some(t) => (
                t.branch.clone(),
                t.base_branch.clone(),
                t.session_id.clone(),
                t.exec.clone(),
            ),
            None => (None, None, None, None),
        }
    };
    hooks::HookCtx {
        terminal_id: terminal_id.to_string(),
        project_dir: project_dir.to_string(),
        worktree: worktree.to_path_buf(),
        branch,
        base_branch,
        session_id,
        turn_uuid: None,
        exec: exec.map(|e| e.prefix),
    }
}

/// Resolve a hook context for a session by terminal id (worktree cwd + owning
/// project dir from the store). None if the session isn't known.
fn hook_ctx_by_id(state: &AppState, terminal_id: &str) -> Option<hooks::HookCtx> {
    let (cwd, project_dir) = {
        let store = state.store.lock();
        let cwd = store.terminal(terminal_id)?.cwd.clone();
        let project_dir = store
            .projects
            .iter()
            .find(|p| p.terminals.iter().any(|t| t.id == terminal_id))
            .map(|p| p.directory.clone())?;
        (cwd, project_dir)
    };
    Some(hook_ctx(state, terminal_id, &project_dir, &PathBuf::from(cwd)))
}

/// The shared global hooks dir (`~/.spwn/hooks`), or None when global hooks are
/// disabled in settings — in which case hook discovery uses the repo scope only and
/// worktree create/remove fall back to spwn's native behavior.
fn enabled_global_hooks_dir(state: &AppState) -> Option<PathBuf> {
    if !state.settings.lock().global_hooks_enabled {
        return None;
    }
    hooks::global_hooks_dir()
}

/// Store one scope's latest run for an event and notify the Hooks panel; surface a
/// one-line advisory if the hook failed. Replaces any prior run for the same scope,
/// keeping the other scope's run intact.
fn record_hook_run(state: &AppState, terminal_id: &str, run: hooks::HookRun) {
    let failed = (!run.ok).then(|| (run.event.clone(), run.script.clone()));
    {
        let mut all = state.hook_runs.lock();
        let runs = all
            .entry(terminal_id.to_string())
            .or_default()
            .entry(run.event.clone())
            .or_default();
        // Keyed by (scope, script): there can be several scripts per scope now
        // (a bare `<event>.sh` plus `<event>.d/*`), each with its own last run.
        runs.retain(|r| !(r.scope == run.scope && r.script == run.script));
        runs.push(run);
    }
    if let Some((event, script)) = failed {
        emit_store_error(state, &format!("Hook failed on {event}: {script}"));
    }
    emit_hooks_event(state, terminal_id);
}

fn emit_hooks_event(state: &AppState, terminal_id: &str) {
    state.hub.emit(&format!("hooks://event/{terminal_id}"), ());
}

/// One streamed line of a running hook's output, pushed to the session's panel live.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct HookOutput<'a> {
    event: &'a str,
    line: &'a str,
}

/// A session's hook starting or finishing. Broadcast globally (not keyed by session)
/// so the tab bar and project tree can drive a "running" spinner without wiring a
/// listener per session. `event` is None when the hook finished.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct HookRunning {
    terminal_id: String,
    event: Option<String>,
}

/// Mark (or clear) a session's currently-running hook and broadcast the change so the
/// tab / tree spinner tracks it.
fn set_hook_running(state: &AppState, terminal_id: &str, event: Option<&str>) {
    {
        let mut running = state.hooks_running.lock();
        match event {
            Some(e) => {
                running.insert(terminal_id.to_string(), e.to_string());
            }
            None => {
                running.remove(terminal_id);
            }
        }
    }
    state.hub.emit(
        "hooks://running",
        HookRunning {
            terminal_id: terminal_id.to_string(),
            event: event.map(str::to_string),
        },
    );
}

/// Publish an agent-session status derived from its pane.
///
/// Live map, persisted `needs_attention` for restart survival, and a broadcast —
/// so a session with no mounted tab still drives the sidebar, and the flag survives
/// a restart where no pane has been observed yet.
pub(crate) fn emit_agent_status(
    state: &Arc<AppState>,
    terminal_id: &str,
    status: crate::agents::SessionStatus,
) {
    use crate::agents::SessionStatus as S;
    {
        let mut map = state.agent_status.lock();
        if status == S::Idle {
            map.remove(terminal_id);
        } else {
            map.insert(terminal_id.to_string(), status);
        }
    }
    let reason = match status {
        S::BlockedPermission | S::BlockedQuestion => Some("blocked"),
        S::Done => Some("done"),
        S::Error => Some("error"),
        _ => None,
    };
    if let Some(reason) = reason {
        let mut changed = false;
        {
            let mut store = state.store.lock();
            if let Some(t) = store.terminal_mut(terminal_id) {
                if !t.needs_attention || t.attention_reason.as_deref() != Some(reason) {
                    t.needs_attention = true;
                    t.attention_reason = Some(reason.to_string());
                    changed = true;
                }
            }
        }
        if changed {
            persist(state);
        }
    }
    state.hub.emit(
        "agent://status",
        serde_json::json!({ "terminalId": terminal_id, "status": status }),
    );

    // A settled pane is the second half of turn detection. The transcript watcher
    // sees records land while the agent is still mid-response and correctly declines
    // to fire; without this re-check the turn would never fire at all, because the
    // file stops changing before the pane goes quiet.
    if matches!(status, S::Done | S::BlockedPermission | S::BlockedQuestion) {
        crate::agents::turns::on_status_settled(state, terminal_id);
    }
}

/// Clear an agent session's live status (intentional teardown).
pub(crate) fn clear_agent_status(state: &AppState, terminal_id: &str) {
    state.agent_status.lock().remove(terminal_id);
    state.turns.lock().forget(terminal_id);
    state.hub.emit(
        "agent://status",
        serde_json::json!({
            "terminalId": terminal_id,
            "status": crate::agents::SessionStatus::Idle,
        }),
    );
}

/// Fire the `session-turn` hooks for a finished turn (commit + checkpoint).
///
/// Backend-driven, unlike the old frontend-triggered path: a session with no open
/// tab still commits and checkpoints.
pub(crate) fn fire_turn_hooks(state: &AppState, terminal_id: &str, turn_uuid: &str) {
    let Some(mut ctx) = hook_ctx_by_id(state, terminal_id) else {
        return;
    };
    // Only sessions with their own worktree branch get per-turn commit/checkpoint.
    if ctx.branch.is_none() {
        return;
    }
    ctx.turn_uuid = Some(turn_uuid.to_string());
    fire_hooks(state, &ctx, "session-turn", false);
}


/// Stream one output line of a running hook to the session's Hooks panel.
fn emit_hook_output(state: &AppState, terminal_id: &str, event: &str, line: &str) {
    state.hub.emit(
        &format!("hooks://output/{terminal_id}"),
        HookOutput { event, line },
    );
}

/// How long a blocking hook prompt waits for a user answer before auto-declining, so a
/// forgotten prompt (or a closed window) can't wedge a session forever.
const HOOK_PROMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// A hook is asking the user a multiple-choice question. Broadcast globally (not keyed
/// by session in the channel name) because hooks fire on session create/delete when no
/// Claude pane is mounted — a global listener in the root renders the picker. `event`
/// and the flattened request fields let the UI reuse the existing question picker.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HookPromptPayload<'a> {
    terminal_id: &'a str,
    id: &'a str,
    event: &'a str,
    #[serde(flatten)]
    request: &'a hooks::HookPromptRequest,
}

/// A hook prompt is no longer answerable (answered, timed out, or the hook died) — the
/// UI drops the card by id.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HookPromptClosePayload<'a> {
    terminal_id: &'a str,
    id: &'a str,
}

/// Broadcast a hook's multiple-choice prompt to the UI.
fn emit_hook_prompt(
    state: &AppState,
    terminal_id: &str,
    event: &str,
    id: &str,
    request: &hooks::HookPromptRequest,
) {
    state.hub.emit(
        "hooks://prompt",
        HookPromptPayload { terminal_id, id, event, request },
    );
}

/// Tell the UI to dismiss a hook prompt card (it's been answered/timed out).
fn emit_hook_prompt_close(state: &AppState, terminal_id: &str, id: &str) {
    state.hub.emit(
        "hooks://prompt-close",
        HookPromptClosePayload { terminal_id, id },
    );
}

/// Resolve a blocking hook prompt with the user's chosen label(s). Called from the UI;
/// unblocks the synchronous hook runner waiting on the matching receiver.
pub async fn hooks_prompt_answer(
    state: &AppState,
    id: String,
    answer: String,
) -> Result<(), String> {
    let tx = state.hook_prompts.lock().remove(&id);
    if let Some(tx) = tx {
        let _ = tx.send(answer);
    }
    Ok(())
}

/// Run the event's hook (if one exists) synchronously, recording the result and
/// notifying the panel. Hooks are intentionally **synchronous**: the session waits for
/// the script to finish. A hook that wants background work should background it itself
/// (e.g. `my-server & disown`), which keeps the model simple and predictable.
///
/// While it runs, the session is flagged "running" (driving a spinner on its tab / tree
/// row) and each output line is streamed to the Hooks panel live.
///
/// A hook may raise a blocking multiple-choice prompt (a `SPWN_PROMPT` stdout line):
/// spwn shows a picker and waits (up to [`HOOK_PROMPT_TIMEOUT`]) for the user's answer,
/// which is written back to the script's stdin. When `headless` (no UI window, e.g. a
/// scheduled run) prompts auto-decline immediately so the run can't deadlock.
/// Run a set of already-discovered `(scope, script)` entries for an event, streaming
/// output to the panel and recording each run. Returns the union of values the scripts
/// reported via `::spwn:set::` (repo overrides global on key collision, since entries
/// are global-first). No-op when `entries` is empty.
fn run_hook_entries(
    state: &AppState,
    ctx: &hooks::HookCtx,
    event: &str,
    entries: Vec<(hooks::Scope, PathBuf)>,
    headless: bool,
) -> std::collections::BTreeMap<String, String> {
    let mut reported = std::collections::BTreeMap::new();
    if entries.is_empty() {
        return reported;
    }
    let terminal_id = ctx.terminal_id.clone();
    set_hook_running(state, &terminal_id, Some(event));
    let runs = {
        let mut on_line = |line: &str| emit_hook_output(state, &terminal_id, event, line);
        let mut on_prompt = |req: hooks::HookPromptRequest| -> String {
            // No window to answer → decline at once (don't emit/leak a pending prompt).
            if headless {
                return hooks::PROMPT_DECLINED.to_string();
            }
            let id = Uuid::new_v4().to_string();
            let (tx, rx) = std::sync::mpsc::channel::<String>();
            state.hook_prompts.lock().insert(id.clone(), tx);
            emit_hook_prompt(state, &terminal_id, event, &id, &req);
            let answer = rx
                .recv_timeout(HOOK_PROMPT_TIMEOUT)
                .unwrap_or_else(|_| hooks::PROMPT_DECLINED.to_string());
            state.hook_prompts.lock().remove(&id);
            emit_hook_prompt_close(state, &terminal_id, &id);
            answer
        };
        hooks::run_entries(ctx, event, &entries, &mut on_line, &mut on_prompt)
    };
    set_hook_running(state, &terminal_id, None);
    for run in runs {
        for (k, v) in &run.reported {
            reported.insert(k.clone(), v.clone());
        }
        record_hook_run(state, &terminal_id, run);
    }
    reported
}

/// Fire an event's hooks across BOTH scopes (global first, then repo), each in its
/// proper working directory. Returns the merged reported values.
fn fire_hooks(
    state: &AppState,
    ctx: &hooks::HookCtx,
    event: &str,
    headless: bool,
) -> std::collections::BTreeMap<String, String> {
    let entries =
        hooks::discover_all(enabled_global_hooks_dir(state).as_deref(), &ctx.worktree, event);
    run_hook_entries(state, ctx, event, entries, headless)
}

/// Fire only ONE scope's hook for an event. Used where scope ordering matters relative
/// to native worktree create/remove: the global `session-created` script (creates the
/// worktree, runs in the project dir) fires before the repo one (runs in the worktree),
/// and on delete the repo script (in the worktree) fires before the global one.
fn fire_hooks_scope(
    state: &AppState,
    ctx: &hooks::HookCtx,
    event: &str,
    scope: hooks::Scope,
    headless: bool,
) -> std::collections::BTreeMap<String, String> {
    let entries = hooks::discover_scope(
        enabled_global_hooks_dir(state).as_deref(),
        &ctx.worktree,
        event,
        scope,
    )
    .into_iter()
    .map(|p| (scope, p))
    .collect();
    run_hook_entries(state, ctx, event, entries, headless)
}

/// Create a fresh Claude session's worktree via the `session-created` hooks (with a
/// native fallback), returning the resolved worktree path. Shared by interactive
/// `open_terminal` and the headless scheduler.
///
/// Flow: the GLOBAL `session-created` script runs in the project dir and creates +
/// seeds the worktree, reporting it back via `::spwn:set::`. If no global script
/// created one (e.g. it was deleted) or it failed, spwn falls back to native
/// `add_worktree` + `seed_heavy_dirs`. The resolved path/branch/base are stored on the
/// `TerminalRec`, then the REPO `session-created` script runs inside the worktree.
/// Returns None (and leaves the session in the project dir) if the worktree couldn't be
/// created.
pub(crate) fn setup_session_worktree(
    state: &AppState,
    terminal_id: &str,
    project_dir: &str,
    repo: &Path,
    base: String,
    headless: bool,
) -> Option<String> {
    let wt_path = session_worktree_path(state, repo, terminal_id)?;
    let short = terminal_id.split('-').next().unwrap_or(terminal_id);
    let branch = format!("{}{short}", gitwt::SESSION_BRANCH_PREFIX);

    // Context carrying the INTENDED worktree/branch/base, so the global hook can create
    // it. (session_id is unknown until the sidecar binds it later.)
    let ctx = hooks::HookCtx {
        terminal_id: terminal_id.to_string(),
        project_dir: project_dir.to_string(),
        worktree: wt_path.clone(),
        branch: Some(branch.clone()),
        base_branch: Some(base.clone()),
        session_id: None,
        turn_uuid: None,
        exec: None,
    };

    // Worktree creation lives ENTIRELY in the hook: the global `session-created` script
    // (runs in the project dir) creates the worktree and reports it back. If no hook
    // creates one — the script was deleted, global hooks are disabled, or it failed —
    // the session simply runs in the project dir with no isolated worktree/branch.
    let reported = fire_hooks_scope(state, &ctx, "session-created", hooks::Scope::Global, headless);

    // Resolve the worktree from what the hook reported (or the intended path if a custom
    // hook created it there without reporting). None → no worktree; stay in project dir.
    let wt = reported
        .get("worktree")
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .or_else(|| wt_path.exists().then(|| wt_path.clone()))?;

    let final_branch = reported.get("branch").cloned().unwrap_or(branch);
    let final_base = reported.get("base").cloned().unwrap_or(base);
    // Canonicalize before anything downstream sees this path. The pane's cwd is
    // canonicalized in `open_terminal`, and the transcript is located by a slug of
    // THAT path — so if the two disagree (a symlinked worktree, `/tmp` vs
    // `/private/tmp`), session binding and the Timeline die silently. An environment
    // hook mounting the raw path would compound it.
    let wt = std::fs::canonicalize(&wt).unwrap_or(wt);
    let cwd = wt.to_string_lossy().into_owned();
    {
        let mut store = state.store.lock();
        if let Some(t) = store.terminal_mut(terminal_id) {
            t.cwd = cwd.clone();
            t.branch = Some(final_branch);
            t.base_branch = Some(final_base);
        }
    }
    apply_reported_exec(state, terminal_id, &reported);
    persist(state);

    // 3) Repo session-created hook runs inside the now-existing worktree (unchanged
    //    behavior for committed `.spwn/hooks/session-created.sh`). This is also where
    //    an environment hook belongs — it can only bind-mount a worktree that exists —
    //    so its report is captured too rather than discarded.
    let ctx2 = hook_ctx(state, terminal_id, project_dir, &wt);
    let reported_repo = fire_hooks_scope(state, &ctx2, "session-created", hooks::Scope::Repo, headless);
    if apply_reported_exec(state, terminal_id, &reported_repo) {
        persist(state);
    }

    Some(cwd)
}

/// Whether the first word of an exec prefix names something spwn can actually launch:
/// an existing path, or a name resolvable on `PATH`.
fn wrapper_is_runnable(bin: &str) -> bool {
    if bin.contains('/') {
        return Path::new(bin).exists();
    }
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {bin}"))
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Record an environment reported by a `session-created` hook (`::spwn:set:: exec=…`)
/// onto the session. Returns whether anything changed.
///
/// Absent `exec`, nothing is written — a hook that reports only `execShell` has not
/// created an environment, and running the host shell through no prefix is correct.
fn apply_reported_exec(
    state: &AppState,
    terminal_id: &str,
    reported: &std::collections::BTreeMap<String, String>,
) -> bool {
    let Some(prefix) = reported.get("exec").map(|s| s.trim()).filter(|s| !s.is_empty()) else {
        return false;
    };
    // Reject an unusable prefix HERE, where there's a UI to complain to, rather than
    // persisting it and discovering it at spawn time as a pane that dies blank.
    if crate::pty::split_exec_prefix(prefix).is_none() {
        emit_store_error(
            state,
            &format!("hook reported an unparseable environment prefix: {prefix}"),
        );
        return false;
    }
    let spec = crate::store::ExecSpec {
        prefix: prefix.to_string(),
        headless_prefix: reported.get("execHeadless").map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        bin: reported.get("execBin").map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        shell: reported.get("execShell").map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
    };
    let mut store = state.store.lock();
    match store.terminal_mut(terminal_id) {
        Some(t) if t.exec.as_ref() != Some(&spec) => {
            t.exec = Some(spec);
            true
        }
        _ => false,
    }
}

/// Discovered hooks + last-run results for a session's worktree, for the Hooks panel.
pub async fn hooks_status(
    state: &AppState,
    terminal_id: String,
) -> Result<hooks::HooksStatus, String> {
    let Some(ctx) = hook_ctx_by_id(&state, &terminal_id) else {
        return Ok(hooks::HooksStatus::default());
    };
    if ctx.branch.is_none() {
        // No worktree → nothing to discover; the panel shows an opt-in hint.
        return Ok(hooks::HooksStatus::default());
    }
    let last = state
        .hook_runs
        .lock()
        .get(&terminal_id)
        .cloned()
        .unwrap_or_default();
    let mut st = hooks::status(enabled_global_hooks_dir(&state).as_deref(), &ctx.worktree, &last);
    st.running = state.hooks_running.lock().get(&terminal_id).cloned();
    Ok(st)
}

/// Manually re-run one event's hook for a session. Runs synchronously (awaits the
/// script), so the caller's refresh sees the result immediately.
pub async fn hooks_run(
    state: &AppState,
    terminal_id: String,
    event: String,
) -> Result<(), String> {
    let ctx = hook_ctx_by_id(&state, &terminal_id)
        .ok_or_else(|| "this session has no worktree".to_string())?;
    let reported = fire_hooks(&state, &ctx, &event, false);
    // Re-running `session-created` by hand is the documented way to rebuild a session's
    // environment after its container has been removed, so the fresh prefix has to
    // land on the record. Discarding it happened to work only while the prefix was
    // derivable from the terminal id — one change to a hook's naming and the session
    // would keep exec'ing into something that no longer exists.
    if apply_reported_exec(&state, &terminal_id, &reported) {
        persist(&state);
    }
    Ok(())
}

/// Fire the `session-turn` hooks for a session after a completed Claude turn (the
/// default global script commits the turn onto the session branch and snapshots a
/// checkpoint). No-op for sessions without a worktree branch. Called by the frontend
/// once each turn finishes.
pub async fn hooks_run_turn(
    state: &AppState,
    terminal_id: String,
    turn_uuid: String,
) -> Result<(), String> {
    let Some(mut ctx) = hook_ctx_by_id(&state, &terminal_id) else {
        return Ok(());
    };
    // Only sessions with their own worktree branch get per-turn commit/checkpoint.
    if ctx.branch.is_none() {
        return Ok(());
    }
    ctx.turn_uuid = Some(turn_uuid);
    fire_hooks(&state, &ctx, "session-turn", false);
    Ok(())
}

/// The working dir a session's checkpoints target: the owning terminal's worktree
/// `cwd` if it has one, else the project directory. Checkpoints are keyed by session
/// id, so we resolve the owning terminal by session id.
fn session_checkpoint_dir(state: &AppState, project_id: &str, session_id: &str) -> Option<String> {
    let store = state.store.lock();
    store
        .projects
        .iter()
        .flat_map(|p| p.terminals.iter())
        .find(|t| t.session_id.as_deref() == Some(session_id))
        .map(|t| t.cwd.clone())
        .or_else(|| store.project(project_id).map(|p| p.directory.clone()))
}

/// Snapshot the project directory (kind: "turn" | "baseline" | ...).
pub fn checkpoint_project(
    state: &AppState,
    project_id: String,
    session_id: String,
    turn_uuid: String,
    kind: String,
) -> Result<CheckpointMeta, String> {
    let project_dir = session_checkpoint_dir(&state, &project_id, &session_id)
        .ok_or_else(|| "no such project".to_string())?;
    let app_data = app_data_dir(&state).ok_or_else(|| "no app data dir".to_string())?;
    checkpoints::capture(&app_data, Path::new(&project_dir), &session_id, &turn_uuid, &kind)
}

/// Restore the project's working files to a checkpoint. Takes a pre-restore safety
/// snapshot first (returned, so the restore is itself undoable). The caller must
/// ensure the session isn't mid-turn (the frontend gates on `busy`).
pub fn restore_checkpoint(
    state: &AppState,
    project_id: String,
    session_id: String,
    checkpoint_id: String,
    pre_restore: bool,
) -> Result<Option<CheckpointMeta>, String> {
    let project_dir = session_checkpoint_dir(&state, &project_id, &session_id)
        .ok_or_else(|| "no such project".to_string())?;
    let app_data = app_data_dir(&state).ok_or_else(|| "no app data dir".to_string())?;
    let pd = Path::new(&project_dir);
    let safety = if pre_restore {
        checkpoints::capture(&app_data, pd, &session_id, "pre-restore", "pre-restore").ok()
    } else {
        None
    };
    checkpoints::restore(&app_data, &session_id, &checkpoint_id, pd)?;
    Ok(safety)
}

pub fn list_checkpoints(state: &AppState, session_id: String) -> Vec<CheckpointMeta> {
    app_data_dir(&state)
        .map(|ad| checkpoints::list(&ad, &session_id))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Agent rewind (drives the agent's own rewind UI)
// ---------------------------------------------------------------------------

/// Which menu row to select in order to KEEP everything up to `anchor_uuid`.
///
/// The menu's own wording is "restore to the point **before** you sent this
/// message", so a row labelled with prompt P discards P and everything after it.
/// To keep the anchor turn, the row to select is therefore the one labelled with the
/// **next** user prompt — not the anchor's own.
///
/// Getting this backwards is not a cosmetic error: it silently throws away one more
/// turn than the user asked for, and paired with a file restore it reverts the
/// working tree further than they expected. The row text is also the only join key
/// between spwn's uuid anchors and the agent's UI, since the menu lists messages
/// rather than ids.
fn rewind_target_text(session_id: &str, anchor_uuid: &str) -> Result<String, String> {
    let path = crate::projects::locate_session(session_id)
        .ok_or_else(|| "could not find this session's transcript".to_string())?;
    let turns = crate::transcript::read_transcript(&path);
    let idx = turns
        .iter()
        .position(|t| t.uuid == anchor_uuid)
        .ok_or_else(|| "could not find that turn in the transcript".to_string())?;
    let text = |t: &crate::transcript::Turn| {
        t.blocks
            .iter()
            .find(|b| b.kind == "text")
            .and_then(|b| b.text.clone())
    };
    turns[idx + 1..]
        .iter()
        .find(|t| t.role == "user")
        .and_then(text)
        .ok_or_else(|| {
            "this is already the latest turn — there is nothing after it to undo".to_string()
        })
}

/// Return a session's conversation to an earlier turn, optionally restoring files.
///
/// The conversation rewind is performed by the agent itself; spwn only drives the
/// menu, and refuses if it cannot positively identify the requested turn. Files are
/// spwn's own checkpoints, restored only AFTER the conversation rewind succeeded —
/// so a failed or mis-targeted rewind can never leave the working tree reverted to a
/// point the conversation didn't reach.
pub async fn agent_rewind(
    state: Arc<AppState>,
    terminal_id: String,
    anchor_uuid: String,
    restore_files: bool,
) -> Result<(), String> {
    let (pane, def) = agent_pane(&state, &terminal_id)?;
    if def.rewind.strategy == crate::agents::def::RewindStrategy::None {
        return Err(format!("{} does not support rewinding", def.name));
    }
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

    let anchor_text = rewind_target_text(&session_id, &anchor_uuid)?;

    // Drive the agent's menu. Errors here mean nothing was changed.
    crate::agents::rewind::drive(&pane, &def, &anchor_text, restore_files).await?;

    // Only now touch the working tree. `/rewind` branches the transcript in place
    // and keeps the same session id, so checkpoints stay correctly keyed and there
    // is no re-binding to do.
    if restore_files && def.rewind.tui.as_ref().is_some_and(|t| t.restore_both.is_none()) {
        if let Some(app_data) = app_data_dir(&state) {
            let pd = Path::new(&cwd);
            if let Some(cp) = checkpoints::find_for_turn(&app_data, &session_id, &anchor_uuid) {
                // Safety snapshot before overwriting anything.
                let _ =
                    checkpoints::capture(&app_data, pd, &session_id, "pre-restore", "pre-restore");
                checkpoints::restore(&app_data, &session_id, &cp.id, pd)?;
            }
        }
    }

    // The conversation now rests on the anchor turn. Re-seed the turn tracker so the
    // restored turn isn't mistaken for a newly-finished one and re-committed.
    state.turns.lock().prime(&terminal_id, &anchor_uuid);
    Ok(())
}

// ---------------------------------------------------------------------------
// Agent TUI control (rmux panes)
// ---------------------------------------------------------------------------

/// The pane handle and agent definition for a terminal, or an error naming which
/// half is missing.
fn agent_pane(
    state: &AppState,
    terminal_id: &str,
) -> Result<(rmux_sdk::Pane, crate::agents::AgentDef), String> {
    let (pane, agent_id) = {
        let sessions = state.sessions.lock();
        let s = sessions
            .get(terminal_id)
            .ok_or_else(|| "no such terminal".to_string())?;
        let id = s
            .agent_id()
            .ok_or_else(|| "not an agent session".to_string())?
            .to_string();
        (s.pane.clone(), id)
    };
    let def = state
        .agents
        .lock()
        .get(&agent_id)
        .cloned()
        .ok_or_else(|| format!("unknown agent '{agent_id}'"))?;
    Ok((pane, def))
}

/// Send a prompt to an agent's composer.
///
/// `submit` false pastes without pressing Enter — that is the "→ parent" and
/// context-injection contract: the text lands in the composer for the human to read
/// and edit, and is never sent on their behalf.
///
/// The paste is bracketed so a multi-line blob arrives as ONE paste; sent literally,
/// a 20-line prompt would be 20 separate submissions.
pub async fn agent_send(
    state: &AppState,
    terminal_id: String,
    text: String,
    submit: bool,
) -> Result<(), String> {
    let (pane, def) = agent_pane(state, &terminal_id)?;
    let input = &def.input;

    // "Send this text" has to send exactly this text, so empty the composer first.
    //
    // The composer is not reliably empty: an agent pre-fills it after a rewind (with
    // the message you rewound past), and a user may have typed something. Appending
    // produced a real corruption — a follow-up prompt arrived as
    // "…word: charlieReply with exactly one word: delta" and was sent to the model.
    //
    // Only for `submit`: a paste-for-review deliberately leaves the human in
    // control of the composer, and clearing there could discard what they typed.
    if submit {
        for k in &def.keys.clear {
            pane.send_key(k.clone()).await.map_err(|e| e.to_string())?;
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }

    match input.paste {
        crate::agents::def::PasteMode::Bracketed => {
            pane.send_text(format!("{}{}{}", input.paste_prefix, text, input.paste_suffix))
                .await
                .map_err(|e| e.to_string())?;
        }
        crate::agents::def::PasteMode::Literal => {
            pane.send_text(&text).await.map_err(|e| e.to_string())?;
        }
        crate::agents::def::PasteMode::Chunked => {
            for chunk in text.as_bytes().chunks(input.chunk_bytes.max(1)) {
                pane.send_text(String::from_utf8_lossy(chunk).into_owned())
                    .await
                    .map_err(|e| e.to_string())?;
                tokio::time::sleep(std::time::Duration::from_millis(input.chunk_delay_ms)).await;
            }
        }
    }
    if submit {
        // Let the paste land before Enter, or Enter arrives while the TUI is still
        // absorbing the bracketed paste and is swallowed — leaving the prompt
        // sitting in the composer, unsent, with no error anywhere. Observed live at
        // the original 120ms.
        tokio::time::sleep(std::time::Duration::from_millis(input.submit_delay_ms)).await;

        // Verify rather than trust. A successful submit always changes the screen
        // (the composer clears, or the message queues); if nothing moved, the key
        // didn't register and we press once more.
        //
        // Comparing whole-screen text is deliberately agent-agnostic — it needs no
        // knowledge of where this particular TUI draws its composer, so it keeps
        // working for an agent whose definition someone else wrote.
        let before = pane.snapshot().await.ok().map(|s| s.visible_text());
        send_keys(&pane, &def.keys.submit).await?;
        if let Some(before) = before {
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;
            let after = pane.snapshot().await.ok().map(|s| s.visible_text());
            if after.as_deref() == Some(before.as_str()) {
                send_keys(&pane, &def.keys.submit).await?;
            }
        }
    }
    Ok(())
}

/// Send raw key tokens (tmux names like `Enter`, `Escape`, `C-c`, `BTab`).
pub async fn agent_key(state: &AppState, terminal_id: String, key: String) -> Result<(), String> {
    let (pane, _) = agent_pane(state, &terminal_id)?;
    pane.send_key(key).await.map_err(|e| e.to_string())
}

/// Interrupt a running turn.
pub async fn agent_interrupt(state: &AppState, terminal_id: String) -> Result<(), String> {
    let (pane, def) = agent_pane(state, &terminal_id)?;
    send_keys(&pane, &def.keys.interrupt).await
}

/// Cycle the agent's permission mode until the screen reports the target.
///
/// The mode key is a blind cycle — there is no "set mode to X" input — so this
/// presses and reads back, bounded by the number of modes. Reading back matters:
/// the starting mode comes from the user's own config and is not knowable in
/// advance, so a fixed number of presses would land somewhere arbitrary.
pub async fn agent_set_mode(
    state: &AppState,
    terminal_id: String,
    mode: String,
) -> Result<(), String> {
    let (pane, def) = agent_pane(state, &terminal_id)?;
    if def.keys.mode_cycle.is_empty() {
        return Err("this agent has no mode cycle".to_string());
    }
    let re = def
        .modes
        .line
        .as_deref()
        .and_then(|p| regex::Regex::new(p).ok());

    let read_mode = |pane: rmux_sdk::Pane, re: Option<regex::Regex>| async move {
        let snap = pane.snapshot().await.ok()?;
        let text = snap.visible_text();
        let re = re?;
        let caps = re.captures(&text)?;
        Some(caps.get(1)?.as_str().trim().to_string())
    };

    let target = mode.to_lowercase();
    // +1 attempt so a full cycle can return to where it started before giving up.
    let limit = def.modes.cycle.len().max(1) + 1;
    for _ in 0..limit {
        if let Some(cur) = read_mode(pane.clone(), re.clone()).await {
            if cur.to_lowercase().replace(' ', "") == target.replace(' ', "") {
                if let Some(s) = state.sessions.lock().get(&terminal_id) {
                    if let Some(a) = &s.agent {
                        *a.mode.lock() = Some(mode.clone());
                    }
                }
                return Ok(());
            }
        } else {
            // Can't read the mode: press once and stop rather than cycling blindly
            // through every mode, which could land on bypassPermissions.
            send_keys(&pane, &def.keys.mode_cycle).await?;
            return Ok(());
        }
        send_keys(&pane, &def.keys.mode_cycle).await?;
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    }
    Err(format!("could not reach '{mode}' mode"))
}

async fn send_keys(pane: &rmux_sdk::Pane, keys: &[String]) -> Result<(), String> {
    for k in keys {
        pane.send_key(k.clone()).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Shell terminal I/O (rmux)
// ---------------------------------------------------------------------------

pub async fn write_to_pty(
    state: &AppState,
    pty_id: String,
    data: String,
) -> Result<(), String> {
    let pane = state.sessions.lock().get(&pty_id).map(|s| s.pane.clone());
    pane.ok_or_else(|| "no such terminal".to_string())?
        .send_text(&data)
        .await
        .map_err(|e| e.to_string())
}

/// Repaint a freshly-attached client with the pane's current screen.
///
/// Called by the client AFTER it has subscribed to `pty://output/<id>` — the
/// subscription can't exist any earlier, since the terminal id only comes back from
/// `open_terminal`. Anything the backend emits before that point is broadcast to
/// nobody.
pub async fn prime_pty(
    state: &AppState,
    pty_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let pane = state.sessions.lock().get(&pty_id).map(|s| s.pane.clone());
    let pane = pane.ok_or_else(|| "no such terminal".to_string())?;
    crate::pty::prime_pane(&pane, &state.hub, &pty_id, cols, rows).await;
    Ok(())
}

pub async fn resize_pty(
    state: &AppState,
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

pub fn get_settings(state: &AppState) -> Settings {
    state.settings.lock().clone()
}

pub fn set_settings(state: &AppState, mut settings: Settings) -> Result<(), String> {
    // Normalize on the way in, not just on load — the client may still send the
    // legacy `claudePath` shape.
    settings.migrate();
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

/// Reveal the shared global hooks folder (`~/.spwn/hooks`) in Finder, creating it first
/// if needed. So users can view/edit the default hook scripts spwn installed.
pub fn open_global_hooks_dir() -> Result<(), String> {
    let dir = hooks::global_hooks_dir().ok_or_else(|| "could not resolve ~/.spwn/hooks".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::process::Command::new("open")
        .arg(&dir)
        .status()
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Agent registry
// ---------------------------------------------------------------------------

/// Every agent definition spwn knows about, with its resolved binary and
/// capabilities, for the session picker and the Settings panel.
pub fn list_agents(state: &AppState) -> Vec<crate::agents::AgentSummary> {
    let overrides = state.settings.lock().agent_paths.clone();
    state.agents.lock().summaries(&overrides)
}

/// Re-read agent definitions from disk. Editing a TOML to track an upstream TUI
/// change should not require restarting the server (and killing nothing — panes
/// are unaffected, since a definition is only consulted when a session starts or
/// a key is sent).
pub fn reload_agents(state: &AppState) -> Result<Vec<String>, String> {
    let reg = crate::agents::AgentRegistry::load(None);
    let errors = reg.errors().to_vec();
    *state.agents.lock() = reg;
    Ok(errors)
}

/// Reveal `~/.spwn/agents` so the user can edit definitions.
pub fn open_agents_dir() -> Result<(), String> {
    let dir = crate::agents::global_agents_dir()
        .ok_or_else(|| "could not resolve ~/.spwn/agents".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::process::Command::new("open")
        .arg(&dir)
        .status()
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn read_transcript(session_id: String) -> Vec<Turn> {
    match crate::projects::locate_session(&session_id) {
        Some(path) => parse_transcript(&path),
        None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) async fn connect(state: &AppState) -> Result<&Rmux, String> {
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

/// Permanently kill the given terminals (their rmux panes) by id.
async fn kill_terminals(state: &AppState, terminal_ids: &[String]) {
    let mut rmux_ids = Vec::new();
    for tid in terminal_ids {
        if let Some(session) = state.sessions.lock().remove(tid) {
            session.output_task.abort();
        }
        clear_agent_status(state, tid);
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

pub(crate) fn persist(state: &AppState) {
    let Some(path) = state.store_path.lock().clone() else {
        return;
    };
    if let Err(e) = state.store.lock().save(&path) {
        eprintln!("failed to persist projects.json: {e}");
        state.hub.emit("store://error", format!("Couldn't save changes to disk: {e}"));
    }
}

