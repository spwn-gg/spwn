//! The HTTP surface.
//!
//! `POST /api/invoke/:command` is a single generic endpoint over the backend
//! commands: the JSON body is the args object exactly as `ipc.ts` sends it
//! (camelCase), deserialized into a per-command struct that renames to the
//! snake_case Rust params. Sync commands run on a blocking thread; the
//! already-async ones are awaited directly.

use crate::commands::{self as cmd, OpenTerminalSpec};
use crate::settings::Settings;
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

type ApiResult = Result<Json<Value>, (StatusCode, String)>;

fn parse<T: DeserializeOwned>(body: &Bytes) -> Result<T, (StatusCode, String)> {
    let slice: &[u8] = if body.is_empty() { b"{}" } else { body.as_ref() };
    serde_json::from_slice(slice).map_err(|e| (StatusCode::BAD_REQUEST, format!("bad args: {e}")))
}

fn ok<T: serde::Serialize>(v: T) -> ApiResult {
    Ok(Json(serde_json::to_value(v).unwrap_or(Value::Null)))
}

/// Map a command's `Result<T, String>` to the HTTP contract: `Err(msg)` → 400 with
/// the message body, so the client `invoke` helper can `throw new Error(text)`.
fn ok_result<T: serde::Serialize>(r: Result<T, String>) -> ApiResult {
    match r {
        Ok(v) => ok(v),
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

fn join_err(e: tokio::task::JoinError) -> (StatusCode, String) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("command panicked: {e}"),
    )
}

/// Run a sync command on a blocking thread. `$st` binds a cloned `Arc<AppState>`,
/// `$a` the parsed args; `$call` produces the `ApiResult`.
macro_rules! blocking {
    ($state:expr, $body:expr, $args:ty, $st:ident, $a:ident, $call:expr) => {{
        let $a: $args = parse(&$body)?;
        let $st = $state.clone();
        tokio::task::spawn_blocking(move || $call)
            .await
            .map_err(join_err)?
    }};
}

pub async fn invoke(
    State(state): State<Arc<AppState>>,
    Path(command): Path<String>,
    body: Bytes,
) -> ApiResult {
    match command.as_str() {
        // --- Settings / misc ---
        "find_claude" => ok(cmd::find_claude()),
        "get_settings" => blocking!(state, body, NoArgs, st, _a, ok(cmd::get_settings(&st))),
        "set_settings" => {
            blocking!(state, body, SetSettingsArgs, st, a, ok_result(cmd::set_settings(&st, a.settings)))
        }
        "open_global_hooks_dir" => ok_result(
            tokio::task::spawn_blocking(cmd::open_global_hooks_dir)
                .await
                .map_err(join_err)?,
        ),

        // --- Agents ---
        "list_agents" => blocking!(state, body, NoArgs, st, _a, ok(cmd::list_agents(&st))),
        "reload_agents" => {
            blocking!(state, body, NoArgs, st, _a, ok_result(cmd::reload_agents(&st)))
        }
        "open_agents_dir" => ok_result(
            tokio::task::spawn_blocking(cmd::open_agents_dir)
                .await
                .map_err(join_err)?,
        ),

        // --- Projects ---
        "list_projects" => blocking!(state, body, NoArgs, st, _a, ok(cmd::list_projects(&st))),
        "create_project" => blocking!(
            state, body, CreateProjectArgs, st, a,
            ok_result(cmd::create_project(&st, a.name, a.directory))
        ),
        "delete_project" => {
            let a: ProjectIdArgs = parse(&body)?;
            ok_result(cmd::delete_project(&state, a.project_id).await)
        }
        "open_in_vscode" => {
            let a: OpenInVscodeArgs = parse(&body)?;
            ok_result(
                tokio::task::spawn_blocking(move || cmd::open_in_vscode(a.path))
                    .await
                    .map_err(join_err)?,
            )
        }

        // --- Context space ---
        "add_context_block" => blocking!(
            state, body, AddContextBlockArgs, st, a,
            ok_result(cmd::add_context_block(&st, a.project_id, a.kind, a.label, a.text))
        ),
        "add_context_file" => blocking!(
            state, body, AddContextFileArgs, st, a,
            ok_result(cmd::add_context_file(&st, a.project_id, a.path))
        ),
        "remove_context_block" => blocking!(
            state, body, RemoveContextBlockArgs, st, a,
            ok_result(cmd::remove_context_block(&st, a.project_id, a.block_id))
        ),
        "update_context_block" => blocking!(
            state, body, UpdateContextBlockArgs, st, a,
            ok_result(cmd::update_context_block(&st, a.project_id, a.block_id, a.text))
        ),
        "reorder_context" => blocking!(
            state, body, ReorderContextArgs, st, a,
            ok_result(cmd::reorder_context(&st, a.project_id, a.order))
        ),
        "clear_context" => blocking!(
            state, body, ProjectIdArgs, st, a,
            ok_result(cmd::clear_context(&st, a.project_id))
        ),

        // --- Scheduled tasks ---
        "add_scheduled_task" => blocking!(
            state, body, AddScheduledTaskArgs, st, a,
            ok_result(cmd::add_scheduled_task(&st, a.project_id, a.name, a.prompt, a.time, a.weekdays, a.use_context))
        ),
        "update_scheduled_task" => blocking!(
            state, body, UpdateScheduledTaskArgs, st, a,
            ok_result(cmd::update_scheduled_task(&st, a.project_id, a.task_id, a.name, a.prompt, a.time, a.weekdays, a.use_context, a.enabled))
        ),
        "set_scheduled_task_enabled" => blocking!(
            state, body, SetScheduledTaskEnabledArgs, st, a,
            ok_result(cmd::set_scheduled_task_enabled(&st, a.project_id, a.task_id, a.enabled))
        ),
        "remove_scheduled_task" => blocking!(
            state, body, TaskRefArgs, st, a,
            ok_result(cmd::remove_scheduled_task(&st, a.project_id, a.task_id))
        ),
        "run_scheduled_task_now" => blocking!(
            state, body, TaskRefArgs, st, a,
            ok_result(cmd::run_scheduled_task_now(st, a.project_id, a.task_id))
        ),
        "clear_terminal_attention" => blocking!(
            state, body, TerminalIdArgs, st, a,
            ok_result(cmd::clear_terminal_attention(&st, a.terminal_id))
        ),

        // --- Terminals ---
        "open_terminal" => {
            let a: OpenTerminalArgs = parse(&body)?;
            ok_result(cmd::open_terminal(state.clone(), a.spec).await)
        }
        "close_terminal" => blocking!(
            state, body, TerminalIdArgs, st, a,
            ok_result(cmd::close_terminal(&st, a.terminal_id))
        ),
        "delete_terminal" => {
            let a: ProjectTerminalArgs = parse(&body)?;
            ok_result(cmd::delete_terminal(&state, a.project_id, a.terminal_id).await)
        }
        "merge_session" => blocking!(
            state, body, ProjectTerminalArgs, st, a,
            ok_result(cmd::merge_session(&st, a.project_id, a.terminal_id))
        ),
        "session_merge_status" => blocking!(
            state, body, ProjectTerminalArgs, st, a,
            ok_result(cmd::session_merge_status(&st, a.project_id, a.terminal_id))
        ),
        "commit_session_turn" => blocking!(
            state, body, CommitSessionTurnArgs, st, a,
            ok_result(cmd::commit_session_turn(&st, a.terminal_id, a.message))
        ),
        "set_terminal_session" => blocking!(
            state, body, SetTerminalSessionArgs, st, a,
            ok_result(cmd::set_terminal_session(&st, a.project_id, a.terminal_id, a.session_id))
        ),

        // --- Claude chat I/O ---
        "claude_send" => blocking!(
            state, body, ClaudeSendArgs, st, a,
            ok_result(cmd::claude_send(&st, a.terminal_id, a.text))
        ),
        "claude_permission" => blocking!(
            state, body, ClaudePermissionArgs, st, a,
            ok_result(cmd::claude_permission(&st, a.terminal_id, a.id, a.allow, a.message))
        ),
        "claude_set_mode" => blocking!(
            state, body, ClaudeSetModeArgs, st, a,
            ok_result(cmd::claude_set_mode(&st, a.terminal_id, a.mode))
        ),
        "claude_interrupt" => blocking!(
            state, body, TerminalIdArgs, st, a,
            ok_result(cmd::claude_interrupt(&st, a.terminal_id))
        ),
        "claude_answer" => blocking!(
            state, body, ClaudeAnswerArgs, st, a,
            ok_result(cmd::claude_answer(&st, a.terminal_id, a.id, a.text))
        ),
        "claude_rewind" => blocking!(
            state, body, ClaudeRewindArgs, st, a,
            ok_result(cmd::claude_rewind(st, a.terminal_id, a.anchor_uuid))
        ),
        "claude_rewind_restore" => blocking!(
            state, body, ClaudeRewindRestoreArgs, st, a,
            ok_result(cmd::claude_rewind_restore(st, a.terminal_id, a.anchor_uuid, a.restore))
        ),

        // --- Checkpoints ---
        "checkpoint_project" => blocking!(
            state, body, CheckpointProjectArgs, st, a,
            ok_result(cmd::checkpoint_project(&st, a.project_id, a.session_id, a.turn_uuid, a.kind))
        ),
        "restore_checkpoint" => blocking!(
            state, body, RestoreCheckpointArgs, st, a,
            ok_result(cmd::restore_checkpoint(&st, a.project_id, a.session_id, a.checkpoint_id, a.pre_restore))
        ),
        "list_checkpoints" => blocking!(
            state, body, SessionIdArgs, st, a,
            ok(cmd::list_checkpoints(&st, a.session_id))
        ),

        // --- Agent TUI control (rmux panes) ---
        "agent_send" => {
            let a: AgentSendArgs = parse(&body)?;
            ok_result(cmd::agent_send(&state, a.terminal_id, a.text, a.submit).await)
        }
        "agent_key" => {
            let a: AgentKeyArgs = parse(&body)?;
            ok_result(cmd::agent_key(&state, a.terminal_id, a.key).await)
        }
        "agent_interrupt" => {
            let a: TerminalIdArgs = parse(&body)?;
            ok_result(cmd::agent_interrupt(&state, a.terminal_id).await)
        }
        "agent_set_mode" => {
            let a: AgentSetModeArgs = parse(&body)?;
            ok_result(cmd::agent_set_mode(&state, a.terminal_id, a.mode).await)
        }

        // --- Shell pty I/O ---
        "write_to_pty" => {
            let a: WriteToPtyArgs = parse(&body)?;
            ok_result(cmd::write_to_pty(&state, a.pty_id, a.data).await)
        }
        "prime_pty" => {
            let a: PrimePtyArgs = parse(&body)?;
            ok_result(cmd::prime_pty(&state, a.terminal_id, a.cols, a.rows).await)
        }
        "resize_pty" => {
            let a: ResizePtyArgs = parse(&body)?;
            ok_result(cmd::resize_pty(&state, a.pty_id, a.cols, a.rows).await)
        }

        // --- Transcript ---
        "read_transcript" => {
            let a: SessionIdArgs = parse(&body)?;
            ok(tokio::task::spawn_blocking(move || cmd::read_transcript(a.session_id))
                .await
                .map_err(join_err)?)
        }

        // --- Hooks ---
        "hooks_status" => {
            let a: TerminalIdArgs = parse(&body)?;
            ok_result(cmd::hooks_status(&state, a.terminal_id).await)
        }
        "hooks_run" => {
            let a: HooksRunArgs = parse(&body)?;
            ok_result(cmd::hooks_run(&state, a.terminal_id, a.event).await)
        }
        "hooks_run_turn" => {
            let a: HooksRunTurnArgs = parse(&body)?;
            ok_result(cmd::hooks_run_turn(&state, a.terminal_id, a.turn_uuid).await)
        }
        "hooks_prompt_answer" => {
            let a: HooksPromptAnswerArgs = parse(&body)?;
            ok_result(cmd::hooks_prompt_answer(&state, a.id, a.answer).await)
        }

        // --- Git source control ---
        "git_repo_status" => blocking!(
            state, body, ProjectIdArgs, st, a,
            ok_result(cmd::git_repo_status(&st, a.project_id))
        ),
        "git_branches" => blocking!(
            state, body, ProjectIdArgs, st, a,
            ok_result(cmd::git_branches(&st, a.project_id))
        ),
        "git_checkout" => blocking!(
            state, body, GitCheckoutArgs, st, a,
            ok_result(cmd::git_checkout(&st, a.project_id, a.branch))
        ),
        "git_create_branch" => blocking!(
            state, body, GitCreateBranchArgs, st, a,
            ok_result(cmd::git_create_branch(&st, a.project_id, a.name))
        ),
        "git_fetch" => {
            let a: ProjectIdArgs = parse(&body)?;
            ok_result(cmd::git_fetch(&state, a.project_id).await)
        }
        "git_pull" => {
            let a: ProjectIdArgs = parse(&body)?;
            ok_result(cmd::git_pull(&state, a.project_id).await)
        }
        "git_push" => {
            let a: ProjectIdArgs = parse(&body)?;
            ok_result(cmd::git_push(&state, a.project_id).await)
        }
        "git_sync" => {
            let a: ProjectIdArgs = parse(&body)?;
            ok_result(cmd::git_sync(&state, a.project_id).await)
        }

        other => Err((StatusCode::NOT_FOUND, format!("unknown command: {other}"))),
    }
}

pub async fn version() -> Json<Value> {
    Json(serde_json::json!({ "version": env!("CARGO_PKG_VERSION") }))
}

// ---------------------------------------------------------------------------
// Embedded SPA
// ---------------------------------------------------------------------------

#[derive(rust_embed::RustEmbed)]
#[folder = "../build"]
struct Assets;

/// Serve an embedded asset by path, falling back to `index.html` for client-side
/// routes (the app is a single-page app).
pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            asset_response(content.data.into_owned(), mime.as_ref())
        }
        None => match Assets::get("index.html") {
            Some(content) => asset_response(content.data.into_owned(), "text/html"),
            None => (
                StatusCode::NOT_FOUND,
                "spwn UI not built — run `npm run build` first",
            )
                .into_response(),
        },
    }
}

fn asset_response(body: Vec<u8>, mime: &str) -> Response {
    let mut resp = Response::new(axum::body::Body::from(body));
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime).unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    resp
}

// ---------------------------------------------------------------------------
// Per-command argument structs (camelCase keys, matching what `ipc.ts` sends).
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct NoArgs {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetSettingsArgs {
    settings: Settings,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateProjectArgs {
    name: String,
    directory: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectIdArgs {
    project_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenInVscodeArgs {
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddContextBlockArgs {
    project_id: String,
    kind: String,
    label: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddContextFileArgs {
    project_id: String,
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoveContextBlockArgs {
    project_id: String,
    block_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateContextBlockArgs {
    project_id: String,
    block_id: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReorderContextArgs {
    project_id: String,
    order: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddScheduledTaskArgs {
    project_id: String,
    name: String,
    prompt: String,
    time: String,
    weekdays: Vec<u8>,
    use_context: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateScheduledTaskArgs {
    project_id: String,
    task_id: String,
    name: String,
    prompt: String,
    time: String,
    weekdays: Vec<u8>,
    use_context: bool,
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetScheduledTaskEnabledArgs {
    project_id: String,
    task_id: String,
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskRefArgs {
    project_id: String,
    task_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TerminalIdArgs {
    terminal_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenTerminalArgs {
    spec: OpenTerminalSpec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectTerminalArgs {
    project_id: String,
    terminal_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitSessionTurnArgs {
    terminal_id: String,
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetTerminalSessionArgs {
    project_id: String,
    terminal_id: String,
    session_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeSendArgs {
    terminal_id: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudePermissionArgs {
    terminal_id: String,
    id: String,
    allow: bool,
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeSetModeArgs {
    terminal_id: String,
    mode: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeAnswerArgs {
    terminal_id: String,
    id: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeRewindArgs {
    terminal_id: String,
    anchor_uuid: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeRewindRestoreArgs {
    terminal_id: String,
    anchor_uuid: String,
    restore: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheckpointProjectArgs {
    project_id: String,
    session_id: String,
    turn_uuid: String,
    kind: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RestoreCheckpointArgs {
    project_id: String,
    session_id: String,
    checkpoint_id: String,
    pre_restore: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionIdArgs {
    session_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentSendArgs {
    terminal_id: String,
    text: String,
    /// Press the submit key after pasting. False = paste for review, which is what
    /// "→ parent" and context injection rely on.
    #[serde(default)]
    submit: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentKeyArgs {
    terminal_id: String,
    key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentSetModeArgs {
    terminal_id: String,
    mode: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrimePtyArgs {
    terminal_id: String,
    cols: u16,
    rows: u16,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteToPtyArgs {
    pty_id: String,
    data: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResizePtyArgs {
    pty_id: String,
    cols: u16,
    rows: u16,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HooksRunArgs {
    terminal_id: String,
    event: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HooksRunTurnArgs {
    terminal_id: String,
    turn_uuid: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HooksPromptAnswerArgs {
    id: String,
    answer: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitCheckoutArgs {
    project_id: String,
    branch: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitCreateBranchArgs {
    project_id: String,
    name: String,
}
