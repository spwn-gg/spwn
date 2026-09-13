//! The host side of the `spwn` API: what `__host(op, json)` and `__hostSync(op, json)`
//! do. Every op answers `{"ok": value}` or `{"err": message, "code"?}`; the prelude turns
//! the latter into a thrown `Error`.
//!
//! Ops run on the run's thread, but anything touching sessions or panes is handed to
//! the server's runtime ([`RunCtx::on_main`]): rmux connections and pane watchers live
//! there, and must outlive a run.

use super::Run;
use crate::agents::SessionStatus;
use crate::commands::{self, OpenTerminalSpec};
use crate::hooks::{self, HookPromptRequest};
use crate::state::AppState;
use crate::store::{TerminalRec, WorkflowTag};
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

/// How long a hook's `spwn prompt` waits for the workflow's answer.
const PROMPT_TIMEOUT: Duration = Duration::from_secs(300);

/// Captured output kept per stream from `spwn.exec`.
const EXEC_OUTPUT_CAP: usize = 4 * 1024 * 1024;

const GITHUB_API: &str = "https://api.github.com";

// ---------------------------------------------------------------------------
// Flags
// ---------------------------------------------------------------------------

/// A one-way switch that can be awaited.
#[derive(Clone)]
pub struct Flag(Arc<watch::Sender<bool>>);

impl Default for Flag {
    fn default() -> Self {
        Self(Arc::new(watch::channel(false).0))
    }
}

impl Flag {
    pub fn set(&self) {
        self.0.send_replace(true);
    }

    pub fn is_set(&self) -> bool {
        *self.0.borrow()
    }

    pub async fn wait(&self) {
        let mut rx = self.0.subscribe();
        let _ = rx.wait_for(|set| *set).await;
    }
}

/// The two ways an attempt ends: the run is stopped, or `main` finished.
#[derive(Clone)]
pub struct StopFlags {
    run: Flag,
    attempt: Flag,
}

impl StopFlags {
    pub fn any_set(&self) -> bool {
        self.run.is_set() || self.attempt.is_set()
    }

    pub async fn wait(&self) {
        tokio::select! {
            _ = self.run.wait() => {}
            _ = self.attempt.wait() => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Run context
// ---------------------------------------------------------------------------

/// One attempt of a run, as seen by its host calls.
pub struct RunCtx {
    pub state: Arc<AppState>,
    pub run: Arc<Run>,
    pub project_id: String,
    pub project_dir: PathBuf,
    pub workflow: String,
    pub inputs: Value,
    main: tokio::runtime::Handle,
    attempt_done: Flag,
    events_tx: mpsc::UnboundedSender<Value>,
    events_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<Value>>,
    /// prompt id → where the hook runner waits for the answer.
    pending_prompts: Mutex<HashMap<String, std::sync::mpsc::Sender<String>>>,
    /// terminal id → the JS prompt handler registered for it.
    prompt_handlers: Mutex<HashMap<String, String>>,
    /// Events `spwn.on` listens for.
    subscribed: Mutex<HashSet<String>>,
    listening: AtomicBool,
    kv_path: Option<PathBuf>,
    kv: Mutex<serde_json::Map<String, Value>>,
    http: OnceLock<reqwest::Client>,
}

impl RunCtx {
    pub fn new(
        state: Arc<AppState>,
        run: Arc<Run>,
        project_id: String,
        project_dir: PathBuf,
        workflow: String,
        inputs: Value,
        main: tokio::runtime::Handle,
    ) -> Self {
        let kv_path = commands::app_data_dir(&state).map(|d| {
            d.join("workflows")
                .join(&project_id)
                .join(format!("{workflow}.state.json"))
        });
        let kv = kv_path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        Self {
            state,
            run,
            project_id,
            project_dir,
            workflow,
            inputs,
            main,
            attempt_done: Flag::default(),
            events_tx,
            events_rx: tokio::sync::Mutex::new(events_rx),
            pending_prompts: Mutex::new(HashMap::new()),
            prompt_handlers: Mutex::new(HashMap::new()),
            subscribed: Mutex::new(HashSet::new()),
            listening: AtomicBool::new(false),
            kv_path,
            kv: Mutex::new(kv),
            http: OnceLock::new(),
        }
    }

    pub fn log(&self, level: &str, msg: impl Into<String>) {
        self.run.log(&self.state, level, msg);
    }

    pub fn stop_flags(&self) -> StopFlags {
        StopFlags {
            run: self.run.cancel.clone(),
            attempt: self.attempt_done.clone(),
        }
    }

    pub fn is_stopping(&self) -> bool {
        self.run.cancel.is_set() || self.attempt_done.is_set()
    }

    /// `main` has returned (or thrown): pending host calls resolve as stopped.
    pub fn end_attempt(&self) {
        self.attempt_done.set();
    }

    /// Release what outlives the JS: hook prompts still waiting are declined at once.
    pub fn wind_down(&self) {
        self.end_attempt();
        self.pending_prompts.lock().clear();
    }

    /// Run a future on the server's runtime and wait for it.
    async fn on_main<F, T>(&self, fut: F) -> Result<T, OpError>
    where
        F: Future<Output = Result<T, String>> + Send + 'static,
        T: Send + 'static,
    {
        self.main
            .spawn(fut)
            .await
            .map_err(|e| OpError::Msg(format!("internal error: {e}")))?
            .map_err(OpError::Msg)
    }

    /// Record that this run owns a session, and which JS handler answers its prompts.
    fn claim(self: &Arc<Self>, terminal_id: &str, handler: Option<String>) {
        self.state.workflows.claim(terminal_id, self);
        if let Some(h) = handler {
            self.prompt_handlers.lock().insert(terminal_id.to_string(), h);
        }
        self.run.add_session(&self.state, terminal_id);
    }

    /// Ask the workflow to answer a hook's prompt. Blocks the hook runner's thread
    /// (never this run's) until the JS handler answers, or declines.
    fn ask_prompt(&self, event: &str, terminal_id: &str, req: HookPromptRequest) -> String {
        let Some(handler) = self.prompt_handlers.lock().get(terminal_id).cloned() else {
            return hooks::PROMPT_DECLINED.to_string();
        };
        if self.is_stopping() {
            return hooks::PROMPT_DECLINED.to_string();
        }
        let id = Uuid::new_v4().to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        self.pending_prompts.lock().insert(id.clone(), tx);
        self.log("info", format!("{event} hook asks: {}", req.question));
        let event = json!({
            "type": "hookPrompt",
            "id": id,
            "handler": handler,
            "event": event,
            "terminalId": terminal_id,
            "prompt": req,
        });
        if self.events_tx.send(event).is_err() {
            self.pending_prompts.lock().remove(&id);
            return hooks::PROMPT_DECLINED.to_string();
        }
        let answer = rx
            .recv_timeout(PROMPT_TIMEOUT)
            .unwrap_or_else(|_| hooks::PROMPT_DECLINED.to_string());
        self.pending_prompts.lock().remove(&id);
        answer
    }

    /// Start forwarding this project's session events to `spwn.on` listeners.
    fn listen(self: &Arc<Self>) {
        if self.listening.swap(true, Ordering::SeqCst) {
            return;
        }
        let weak = Arc::downgrade(self);
        let flags = self.stop_flags();
        let mut rx = self.state.hub.subscribe_internal();
        self.main.spawn(async move {
            loop {
                let frame = tokio::select! {
                    _ = flags.wait() => break,
                    frame = rx.recv() => frame,
                };
                let frame = match frame {
                    Ok(f) => f,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                };
                let Some(rc) = weak.upgrade() else { break };
                rc.route(&frame);
            }
        });
    }

    fn route(&self, frame: &str) {
        let Ok(v) = serde_json::from_str::<Value>(frame) else {
            return;
        };
        let topic = v.get("topic").and_then(Value::as_str).unwrap_or("");
        let payload = v.get("payload").cloned().unwrap_or(Value::Null);
        let event = match topic {
            "hooks://fired" => {
                if payload.get("projectId").and_then(Value::as_str) != Some(&self.project_id) {
                    return;
                }
                payload.get("event").and_then(Value::as_str).unwrap_or("").to_string()
            }
            "agent://status" => {
                let tid = payload.get("terminalId").and_then(Value::as_str).unwrap_or("");
                if self.project_terminal(tid).is_err() {
                    return;
                }
                "status".to_string()
            }
            _ => return,
        };
        if self.subscribed.lock().contains(&event) {
            let _ = self
                .events_tx
                .send(json!({ "type": "event", "event": event, "payload": payload }));
        }
    }

    /// A session of this run's project, or an error.
    fn project_terminal(&self, terminal_id: &str) -> Result<TerminalRec, OpError> {
        let store = self.state.store.lock();
        store
            .project(&self.project_id)
            .and_then(|p| p.terminals.iter().find(|t| t.id == terminal_id))
            .cloned()
            .ok_or_else(|| OpError::msg("no such session in this project"))
    }

    fn http(&self) -> &reqwest::Client {
        self.http.get_or_init(|| {
            reqwest::Client::builder()
                .user_agent(concat!("spwn/", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_default()
        })
    }

    fn save_kv(&self, kv: &serde_json::Map<String, Value>) -> Result<(), OpError> {
        let Some(path) = &self.kv_path else {
            return Ok(()); // no app data dir: state lives for the process only
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| OpError::Msg(e.to_string()))?;
        }
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(kv).unwrap_or_else(|_| "{}".to_string());
        std::fs::write(&tmp, body)
            .and_then(|_| std::fs::rename(&tmp, path))
            .map_err(|e| OpError::Msg(format!("couldn't save workflow state: {e}")))
    }
}

/// A [`hooks::Prompter`] that routes a hook's prompt to the run that owns the session.
pub fn prompter(rc: Weak<RunCtx>) -> hooks::Prompter {
    Arc::new(move |event: &str, terminal_id: &str, req: HookPromptRequest| {
        match rc.upgrade() {
            Some(rc) => rc.ask_prompt(event, terminal_id, req),
            None => hooks::PROMPT_DECLINED.to_string(),
        }
    })
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

enum OpError {
    Stopped,
    Msg(String),
}

impl OpError {
    fn msg(m: impl Into<String>) -> Self {
        Self::Msg(m.into())
    }
}

impl From<String> for OpError {
    fn from(m: String) -> Self {
        Self::Msg(m)
    }
}

type OpResult = Result<Value, OpError>;

fn envelope(res: OpResult) -> String {
    match res {
        Ok(v) => json!({ "ok": v }).to_string(),
        Err(OpError::Stopped) => json!({ "err": "the workflow is stopping", "code": "stopped" }).to_string(),
        Err(OpError::Msg(m)) => json!({ "err": m }).to_string(),
    }
}

fn parse<T: DeserializeOwned>(args: Value) -> Result<T, OpError> {
    serde_json::from_value(args).map_err(|e| OpError::Msg(format!("bad arguments: {e}")))
}

/// A synchronous op: logging, state, and facts about the run.
pub fn dispatch_sync(rc: &Arc<RunCtx>, op: &str, args: &str) -> String {
    let args: Value = serde_json::from_str(args).unwrap_or(Value::Null);
    envelope(sync_op(rc, op, args))
}

fn sync_op(rc: &Arc<RunCtx>, op: &str, args: Value) -> OpResult {
    #[derive(Deserialize)]
    struct LogArgs {
        level: String,
        msg: String,
    }
    #[derive(Deserialize)]
    struct KeyArgs {
        key: String,
        #[serde(default)]
        value: Value,
    }
    #[derive(Deserialize)]
    struct SubscribeArgs {
        events: Vec<String>,
    }
    match op {
        "log" => {
            let a: LogArgs = parse(args)?;
            rc.log(&a.level, a.msg);
            Ok(Value::Null)
        }
        "project" => {
            let name = rc
                .state
                .store
                .lock()
                .project(&rc.project_id)
                .map(|p| p.name.clone());
            Ok(json!({
                "id": rc.project_id,
                "name": name,
                "dir": rc.project_dir,
                "workflow": rc.workflow,
                "runId": rc.run.id,
            }))
        }
        "stopping" => Ok(Value::Bool(rc.is_stopping())),
        "state.get" => {
            let a: KeyArgs = parse(args)?;
            Ok(rc.kv.lock().get(&a.key).cloned().unwrap_or(Value::Null))
        }
        "state.set" => {
            let a: KeyArgs = parse(args)?;
            let mut kv = rc.kv.lock();
            if a.value.is_null() {
                kv.remove(&a.key);
            } else {
                kv.insert(a.key, a.value);
            }
            rc.save_kv(&kv)?;
            Ok(Value::Null)
        }
        "state.delete" => {
            let a: KeyArgs = parse(args)?;
            let mut kv = rc.kv.lock();
            kv.remove(&a.key);
            rc.save_kv(&kv)?;
            Ok(Value::Null)
        }
        "state.all" => Ok(Value::Object(rc.kv.lock().clone())),
        "events.subscribe" => {
            let a: SubscribeArgs = parse(args)?;
            *rc.subscribed.lock() = a.events.into_iter().collect();
            rc.listen();
            Ok(Value::Null)
        }
        other => Err(OpError::Msg(format!("unknown op `{other}`"))),
    }
}

/// An asynchronous op. Resolves as stopped as soon as the run is stopped or `main` has
/// finished, whatever the op was doing.
pub async fn dispatch(rc: Arc<RunCtx>, op: String, args: String) -> String {
    let args: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
    let flags = rc.stop_flags();
    if op == "untilStopped" {
        flags.wait().await;
        return envelope(Ok(Value::Null));
    }
    if flags.any_set() {
        return envelope(Err(OpError::Stopped));
    }
    let res = tokio::select! {
        biased;
        _ = flags.wait() => Err(OpError::Stopped),
        r = async_op(&rc, &op, args) => r,
    };
    envelope(res)
}

async fn async_op(rc: &Arc<RunCtx>, op: &str, args: Value) -> OpResult {
    match op {
        "sleep" => {
            #[derive(Deserialize)]
            struct A {
                ms: f64,
            }
            let a: A = parse(args)?;
            tokio::time::sleep(Duration::from_millis(a.ms.max(0.0) as u64)).await;
            Ok(Value::Null)
        }
        "events.next" => {
            let mut rx = rc.events_rx.lock().await;
            rx.recv().await.ok_or(OpError::Stopped)
        }
        "prompt.answer" => {
            #[derive(Deserialize)]
            struct A {
                id: String,
                answer: Option<String>,
            }
            let a: A = parse(args)?;
            if let Some(tx) = rc.pending_prompts.lock().remove(&a.id) {
                let _ = tx.send(a.answer.unwrap_or_else(|| hooks::PROMPT_DECLINED.to_string()));
            }
            Ok(Value::Null)
        }
        "exec" => exec(rc, parse(args)?).await,
        "fetch" => fetch(rc, parse(args)?).await,
        "github.graphql" => github_graphql(rc, parse(args)?).await,
        "github.rest" => github_rest(rc, parse(args)?).await,
        "fs.read" => {
            let a: PathArgs = parse(args)?;
            let p = in_project(rc, &a.path)?;
            tokio::fs::read_to_string(&p)
                .await
                .map(Value::String)
                .map_err(|e| OpError::Msg(format!("{}: {e}", a.path)))
        }
        "fs.write" => {
            let a: PathArgs = parse(args)?;
            let p = in_project(rc, &a.path)?;
            if let Some(dir) = p.parent() {
                tokio::fs::create_dir_all(dir).await.map_err(|e| OpError::Msg(e.to_string()))?;
            }
            tokio::fs::write(&p, a.content.unwrap_or_default())
                .await
                .map(|_| Value::Null)
                .map_err(|e| OpError::Msg(format!("{}: {e}", a.path)))
        }
        "fs.exists" => {
            let a: PathArgs = parse(args)?;
            Ok(Value::Bool(in_project(rc, &a.path)?.exists()))
        }
        "agents.list" => Ok(serde_json::to_value(commands::list_agents(&rc.state)).unwrap_or(Value::Null)),
        "agents.run" => agents_run(rc, parse(args)?).await,
        "sessions.create" => sessions_create(rc, parse(args)?).await,
        "sessions.list" => {
            let ids: Vec<String> = {
                let store = rc.state.store.lock();
                store
                    .project(&rc.project_id)
                    .map(|p| {
                        p.terminals
                            .iter()
                            .filter(|t| t.workflow.as_ref().is_some_and(|w| w.name == rc.workflow))
                            .map(|t| t.id.clone())
                            .collect()
                    })
                    .unwrap_or_default()
            };
            Ok(Value::Array(ids.iter().filter_map(|id| session_json(&rc.state, id)).collect()))
        }
        "sessions.find" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                key: String,
                prompt_handler: Option<String>,
            }
            let a: A = parse(args)?;
            let id = {
                let store = rc.state.store.lock();
                store.project(&rc.project_id).and_then(|p| {
                    p.terminals
                        .iter()
                        .find(|t| {
                            t.workflow.as_ref().is_some_and(|w| {
                                w.name == rc.workflow && w.key.as_deref() == Some(a.key.as_str())
                            })
                        })
                        .map(|t| t.id.clone())
                })
            };
            match id {
                Some(id) => {
                    rc.claim(&id, a.prompt_handler);
                    Ok(session_json(&rc.state, &id).unwrap_or(Value::Null))
                }
                None => Ok(Value::Null),
            }
        }
        "session.get" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A {
                id: String,
                #[serde(default)]
                claim: bool,
                prompt_handler: Option<String>,
            }
            let a: A = parse(args)?;
            if rc.project_terminal(&a.id).is_err() {
                return Ok(Value::Null);
            }
            if a.claim {
                rc.claim(&a.id, a.prompt_handler);
            }
            Ok(session_json(&rc.state, &a.id).unwrap_or(Value::Null))
        }
        "session.status" => {
            let a: IdArgs = parse(args)?;
            rc.project_terminal(&a.id)?;
            Ok(json!(commands::agent_status_of(&rc.state, &a.id)))
        }
        "session.send" => {
            #[derive(Deserialize)]
            struct A {
                id: String,
                text: String,
                #[serde(default = "yes")]
                submit: bool,
            }
            let a: A = parse(args)?;
            rc.project_terminal(&a.id)?;
            let state = rc.state.clone();
            let mark = rc
                .on_main(async move {
                    commands::ensure_attached(&state, &a.id).await?;
                    let mark = tail_uuid(&state, &a.id);
                    commands::agent_send(&state, a.id, a.text, a.submit).await?;
                    Ok(mark)
                })
                .await?;
            Ok(json!(mark))
        }
        "session.waitForTurn" => wait_for_turn(rc, parse(args)?).await,
        "session.waitForStatus" => wait_for_status(rc, parse(args)?).await,
        "session.screen" => {
            let a: IdArgs = parse(args)?;
            rc.project_terminal(&a.id)?;
            let state = rc.state.clone();
            Ok(Value::String(rc.on_main(async move { screen(&state, &a.id).await }).await?))
        }
        "session.key" => {
            #[derive(Deserialize)]
            struct A {
                id: String,
                key: String,
            }
            let a: A = parse(args)?;
            rc.project_terminal(&a.id)?;
            let state = rc.state.clone();
            rc.on_main(async move {
                commands::ensure_attached(&state, &a.id).await?;
                commands::agent_key(&state, a.id, a.key).await
            })
            .await?;
            Ok(Value::Null)
        }
        "session.interrupt" => {
            let a: IdArgs = parse(args)?;
            rc.project_terminal(&a.id)?;
            let state = rc.state.clone();
            rc.on_main(async move {
                commands::ensure_attached(&state, &a.id).await?;
                commands::agent_interrupt(&state, a.id).await
            })
            .await?;
            Ok(Value::Null)
        }
        "session.transcript" => {
            let a: IdArgs = parse(args)?;
            let t = rc.project_terminal(&a.id)?;
            let turns = t
                .session_id
                .as_deref()
                .and_then(crate::projects::locate_session)
                .map(|p| crate::transcript::read_transcript(&p))
                .unwrap_or_default();
            Ok(serde_json::to_value(turns).unwrap_or(Value::Null))
        }
        "session.lastMessage" => {
            let a: IdArgs = parse(args)?;
            let t = rc.project_terminal(&a.id)?;
            Ok(json!(t
                .session_id
                .as_deref()
                .and_then(crate::projects::locate_session)
                .and_then(|p| final_text(&p))))
        }
        "session.delete" => {
            let a: IdArgs = parse(args)?;
            rc.project_terminal(&a.id)?;
            let (state, pid) = (rc.state.clone(), rc.project_id.clone());
            rc.on_main(async move { commands::delete_terminal(&state, pid, a.id).await })
                .await?;
            rc.state.hub.emit("projects://changed", Vec::<String>::new());
            Ok(Value::Null)
        }
        other => Err(OpError::Msg(format!("unknown op `{other}`"))),
    }
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
struct IdArgs {
    id: String,
}

#[derive(Deserialize)]
struct PathArgs {
    path: String,
    content: Option<String>,
}

/// A path relative to (and kept inside) the project dir.
fn in_project(rc: &RunCtx, path: &str) -> Result<PathBuf, OpError> {
    let p = super::runtime::normalize(&rc.project_dir.join(path));
    if !p.starts_with(&rc.project_dir) {
        return Err(OpError::Msg(format!("{path} is outside the project")));
    }
    Ok(p)
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

/// A session as the workflow sees it.
fn session_json(state: &AppState, terminal_id: &str) -> Option<Value> {
    let status = commands::agent_status_of(state, terminal_id);
    let store = state.store.lock();
    let t = store.terminal(terminal_id)?;
    Some(json!({
        "id": t.id,
        "title": t.title,
        "kind": t.kind,
        "agent": t.agent,
        "workflow": t.workflow.as_ref().map(|w| w.name.clone()),
        "key": t.workflow.as_ref().and_then(|w| w.key.clone()),
        "branch": t.branch,
        "baseBranch": t.base_branch,
        "cwd": t.cwd,
        "sessionId": t.session_id,
        "status": status,
    }))
}

/// The uuid of the last turn in a session's transcript.
fn tail_uuid(state: &AppState, terminal_id: &str) -> Option<String> {
    let sid = state.store.lock().terminal(terminal_id)?.session_id.clone()?;
    let path = crate::projects::locate_session(&sid)?;
    crate::transcript::tail_summary(&path).last_uuid
}

/// The assistant's reply to the latest prompt: every text block after the last user
/// message that carried text (tool results don't count as a prompt).
fn final_text(path: &Path) -> Option<String> {
    let turns = crate::transcript::read_transcript(path);
    let last_prompt = turns
        .iter()
        .rposition(|t| t.role == "user" && t.blocks.iter().any(|b| b.kind == "text"))
        .map(|i| i + 1)
        .unwrap_or(0);
    let parts: Vec<String> = turns[last_prompt..]
        .iter()
        .filter(|t| t.role == "assistant")
        .flat_map(|t| t.blocks.iter())
        .filter(|b| b.kind == "text")
        .filter_map(|b| b.text.clone())
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

async fn screen(state: &Arc<AppState>, terminal_id: &str) -> Result<String, String> {
    commands::ensure_attached(state, terminal_id).await?;
    let pane = state
        .sessions
        .lock()
        .get(terminal_id)
        .map(|s| s.pane.clone())
        .ok_or_else(|| "no such terminal".to_string())?;
    pane.snapshot()
        .await
        .map(|s| s.visible_text())
        .map_err(|e| e.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateArgs {
    agent: Option<String>,
    title: Option<String>,
    key: Option<String>,
    prompt: Option<String>,
    permission_mode: Option<String>,
    prompt_handler: Option<String>,
    ready_timeout_ms: Option<u64>,
}

async fn sessions_create(rc: &Arc<RunCtx>, a: CreateArgs) -> OpResult {
    let terminal_id = Uuid::new_v4().to_string();
    // Claimed before it exists: its `session-created` hooks run inside the open, and
    // their prompts must already reach this run.
    rc.claim(&terminal_id, a.prompt_handler);
    let spec = OpenTerminalSpec {
        project_id: rc.project_id.clone(),
        terminal_id: Some(terminal_id.clone()),
        kind: "agent".to_string(),
        agent: a.agent,
        cols: 120,
        rows: 40,
        claude_resume: None,
        claude_fork: None,
        parent_terminal_id: None,
        permission_mode: a.permission_mode,
        title: Some(a.title.unwrap_or_else(|| rc.workflow.clone())),
        workflow: Some(WorkflowTag {
            name: rc.workflow.clone(),
            key: a.key,
        }),
        prompt_mode: hooks::PromptMode::Workflow(prompter(Arc::downgrade(rc))),
    };
    let state = rc.state.clone();
    let tid = terminal_id.clone();
    let ready_timeout = Duration::from_millis(a.ready_timeout_ms.unwrap_or(90_000));
    let prompt = a.prompt.filter(|p| !p.trim().is_empty());
    let mark = rc
        .on_main(async move {
            commands::open_terminal(state.clone(), spec).await?;
            state.hub.emit("projects://changed", Vec::<String>::new());
            let Some(prompt) = prompt else {
                return Ok(None);
            };
            commands::wait_agent_ready(&state, &tid, ready_timeout).await?;
            let mark = tail_uuid(&state, &tid);
            commands::agent_send(&state, tid, prompt, true).await?;
            Ok(mark)
        })
        .await?;
    let mut rec = session_json(&rc.state, &terminal_id)
        .ok_or_else(|| OpError::msg("the session disappeared while it was being created"))?;
    rec["mark"] = json!(mark);
    Ok(rec)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WaitTurnArgs {
    id: String,
    since: Option<String>,
    timeout_ms: Option<u64>,
}

/// Wait for the session's next finished turn — after `since`, the transcript position
/// before the prompt was sent — including its `session-turn` hooks (the commit). Returns
/// early with `{ blocked: true }` when the agent stops to ask for permission or input.
async fn wait_for_turn(rc: &Arc<RunCtx>, a: WaitTurnArgs) -> OpResult {
    let t = rc.project_terminal(&a.id)?;
    if t.kind == "agent" {
        let (state, id) = (rc.state.clone(), a.id.clone());
        rc.on_main(async move { commands::ensure_attached(&state, &id).await })
            .await?;
    }
    let since = a.since.or_else(|| tail_uuid(&rc.state, &a.id));
    let deadline = a
        .timeout_ms
        .map(|ms| tokio::time::Instant::now() + Duration::from_millis(ms));
    let mut blocked_polls = 0u8;
    loop {
        let status = commands::agent_status_of(&rc.state, &a.id);
        let path = rc
            .state
            .store
            .lock()
            .terminal(&a.id)
            .and_then(|t| t.session_id.clone())
            .and_then(|sid| crate::projects::locate_session(&sid));
        if let Some(path) = &path {
            let tail = crate::transcript::tail_summary(path);
            if let Some(uuid) = tail.last_uuid.as_deref() {
                let finished = since.as_deref() != Some(uuid)
                    && tail.turn_complete()
                    && status != SessionStatus::Thinking
                    && rc.state.turns.lock().completed(&a.id) == Some(uuid);
                if finished {
                    return Ok(json!({
                        "turnUuid": uuid,
                        "text": final_text(path),
                        "status": status,
                    }));
                }
            }
        }
        if matches!(status, SessionStatus::BlockedPermission | SessionStatus::BlockedQuestion) {
            blocked_polls += 1;
            if blocked_polls >= 3 {
                let (state, id) = (rc.state.clone(), a.id.clone());
                let screen = rc.on_main(async move { screen(&state, &id).await }).await.unwrap_or_default();
                return Ok(json!({ "blocked": true, "status": status, "screen": screen }));
            }
        } else {
            blocked_polls = 0;
        }
        if deadline.is_some_and(|d| tokio::time::Instant::now() >= d) {
            return Err(OpError::msg("timed out waiting for the turn to finish"));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WaitStatusArgs {
    id: String,
    status: Vec<String>,
    timeout_ms: Option<u64>,
}

async fn wait_for_status(rc: &Arc<RunCtx>, a: WaitStatusArgs) -> OpResult {
    rc.project_terminal(&a.id)?;
    let deadline = a
        .timeout_ms
        .map(|ms| tokio::time::Instant::now() + Duration::from_millis(ms));
    loop {
        let status = json!(commands::agent_status_of(&rc.state, &a.id));
        if a.status.iter().any(|s| Some(s.as_str()) == status.as_str()) {
            return Ok(status);
        }
        if deadline.is_some_and(|d| tokio::time::Instant::now() >= d) {
            return Err(OpError::msg("timed out waiting for the status"));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentRunArgs {
    prompt: String,
    agent: Option<String>,
    title: Option<String>,
    key: Option<String>,
    prompt_handler: Option<String>,
}

/// A one-shot headless run in its own worktree, like a scheduled task; resolves with the
/// agent's final message once it finishes.
async fn agents_run(rc: &Arc<RunCtx>, a: AgentRunArgs) -> OpResult {
    let state = rc.state.clone();
    let agent_id = {
        let overrides = state.settings.lock().agent_paths.clone();
        let preferred = a.agent.clone().or_else(|| state.settings.lock().default_agent.clone());
        state.agents.lock().default_id(preferred.as_deref(), &overrides)
    }
    .ok_or_else(|| OpError::msg("no agent available"))?;
    let def = state
        .agents
        .lock()
        .get(&agent_id)
        .cloned()
        .ok_or_else(|| OpError::Msg(format!("unknown agent '{agent_id}'")))?;
    if def.argv.headless.is_none() {
        return Err(OpError::Msg(format!("{} can't run headless", def.name)));
    }

    let terminal_id = Uuid::new_v4().to_string();
    let directory = rc.project_dir.to_string_lossy().into_owned();
    {
        let mut store = state.store.lock();
        let p = store
            .project_mut(&rc.project_id)
            .ok_or_else(|| OpError::msg("no such project"))?;
        p.terminals.push(TerminalRec {
            id: terminal_id.clone(),
            title: a.title.unwrap_or_else(|| rc.workflow.clone()),
            kind: "agent".to_string(),
            agent: Some(agent_id),
            cwd: directory.clone(),
            session_id: None,
            group_id: None,
            parent_id: None,
            branch: None,
            base_branch: None,
            needs_attention: false,
            attention_reason: None,
            exec: None,
            workflow: Some(WorkflowTag {
                name: rc.workflow.clone(),
                key: a.key,
            }),
        });
    }
    commands::persist(&state);
    rc.claim(&terminal_id, a.prompt_handler);

    let mode = hooks::PromptMode::Workflow(prompter(Arc::downgrade(rc)));
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let tid = terminal_id.clone();
    let prompt = a.prompt;
    rc.on_main(async move {
        let (st, t, dir) = (state.clone(), tid.clone(), directory.clone());
        let run_dir = tokio::task::spawn_blocking(move || {
            let repo = crate::gitwt::repo_root(Path::new(&dir))?;
            let base = crate::gitwt::current_branch(&repo)?;
            commands::setup_session_worktree(&st, &t, &dir, &repo, base, &mode)
        })
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or(directory);
        let cwd = std::fs::canonicalize(&run_dir).unwrap_or_else(|_| PathBuf::from(&run_dir));
        let session_id = Uuid::new_v4().to_string();
        commands::bind_session(&state, &tid, &session_id);
        state.hub.emit("projects://changed", Vec::<String>::new());
        let rmux = commands::connect(&state).await?;
        crate::agents::headless::run(state.clone(), rmux, &def, tid, session_id, &cwd, prompt, move |o| {
            let _ = done_tx.send(o);
        })
        .await
    })
    .await?;

    let outcome = done_rx
        .await
        .map_err(|_| OpError::msg("the run ended without reporting an outcome"))?;
    let text = rc
        .state
        .store
        .lock()
        .terminal(&terminal_id)
        .and_then(|t| t.session_id.clone())
        .and_then(|sid| crate::projects::locate_session(&sid))
        .and_then(|p| final_text(&p));
    let (ok, error) = match outcome {
        crate::agents::headless::Outcome::Ok => (true, None),
        crate::agents::headless::Outcome::Failed(why) => (false, Some(why)),
    };
    Ok(json!({ "ok": ok, "error": error, "sessionId": terminal_id, "text": text }))
}

// ---------------------------------------------------------------------------
// exec / fetch / GitHub
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecArgs {
    cmd: String,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    input: Option<String>,
    timeout_ms: Option<u64>,
}

fn capped(bytes: Vec<u8>) -> String {
    let slice = &bytes[..bytes.len().min(EXEC_OUTPUT_CAP)];
    String::from_utf8_lossy(slice).into_owned()
}

async fn exec(rc: &Arc<RunCtx>, a: ExecArgs) -> OpResult {
    use tokio::io::AsyncWriteExt;
    let cwd = a
        .cwd
        .as_deref()
        .map(|c| rc.project_dir.join(c))
        .unwrap_or_else(|| rc.project_dir.clone());
    let mut cmd = tokio::process::Command::new(&a.cmd);
    cmd.args(&a.args)
        .current_dir(&cwd)
        .envs(&a.env)
        .stdin(if a.input.is_some() {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // A stopped run (or a timeout) drops the future, which kills the process.
        .kill_on_drop(true);
    let mut child = cmd
        .spawn()
        .map_err(|e| OpError::Msg(format!("couldn't run `{}`: {e}", a.cmd)))?;
    if let (Some(input), Some(mut stdin)) = (a.input, child.stdin.take()) {
        tokio::spawn(async move {
            let _ = stdin.write_all(input.as_bytes()).await;
        });
    }
    let timeout = Duration::from_millis(a.timeout_ms.unwrap_or(10 * 60 * 1000));
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(out)) => Ok(json!({
            "code": out.status.code(),
            "ok": out.status.success(),
            "stdout": capped(out.stdout),
            "stderr": capped(out.stderr),
        })),
        Ok(Err(e)) => Err(OpError::Msg(format!("`{}` failed: {e}", a.cmd))),
        Err(_) => Err(OpError::Msg(format!(
            "`{}` timed out after {}s",
            a.cmd,
            timeout.as_secs()
        ))),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchArgs {
    url: String,
    method: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    body: Option<String>,
    json: Option<Value>,
    timeout_ms: Option<u64>,
}

async fn fetch(rc: &Arc<RunCtx>, a: FetchArgs) -> OpResult {
    let method = reqwest::Method::from_bytes(a.method.as_deref().unwrap_or("GET").to_uppercase().as_bytes())
        .map_err(|_| OpError::msg("bad HTTP method"))?;
    let mut req = rc
        .http()
        .request(method, &a.url)
        .timeout(Duration::from_millis(a.timeout_ms.unwrap_or(60_000)));
    for (k, v) in &a.headers {
        req = req.header(k, v);
    }
    if let Some(j) = &a.json {
        req = req.json(j);
    } else if let Some(b) = a.body {
        req = req.body(b);
    }
    let resp = req.send().await.map_err(|e| OpError::Msg(e.to_string()))?;
    let status = resp.status();
    let headers: BTreeMap<String, String> = resp
        .headers()
        .iter()
        .filter_map(|(k, v)| Some((k.as_str().to_string(), v.to_str().ok()?.to_string())))
        .collect();
    let body = resp.text().await.map_err(|e| OpError::Msg(e.to_string()))?;
    Ok(json!({
        "status": status.as_u16(),
        "ok": status.is_success(),
        "headers": headers,
        "body": body,
    }))
}

fn github_token() -> Result<String, OpError> {
    crate::gitauth::token()
        .ok_or_else(|| OpError::msg("no GitHub token is saved — add one in spwn's Settings"))
}

/// GitHub's error message from a response body, else the body itself.
fn github_error(status: reqwest::StatusCode, body: &str) -> OpError {
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| body.chars().take(500).collect());
    OpError::Msg(format!("GitHub {}: {message}", status.as_u16()))
}

#[derive(Deserialize)]
struct GraphqlArgs {
    query: String,
    #[serde(default)]
    variables: Value,
}

async fn github_graphql(rc: &Arc<RunCtx>, a: GraphqlArgs) -> OpResult {
    let token = github_token()?;
    let resp = rc
        .http()
        .post(format!("{GITHUB_API}/graphql"))
        .bearer_auth(token)
        .json(&json!({ "query": a.query, "variables": a.variables }))
        .send()
        .await
        .map_err(|e| OpError::Msg(e.to_string()))?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| OpError::Msg(e.to_string()))?;
    if !status.is_success() {
        return Err(github_error(status, &body));
    }
    let v: Value = serde_json::from_str(&body).map_err(|e| OpError::Msg(e.to_string()))?;
    let errors: Vec<String> = v
        .get("errors")
        .and_then(Value::as_array)
        .map(|errs| {
            errs.iter()
                .filter_map(|e| e.get("message").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let data = v.get("data").cloned().unwrap_or(Value::Null);
    if !errors.is_empty() {
        if data.is_null() {
            return Err(OpError::Msg(format!("GitHub GraphQL: {}", errors.join("; "))));
        }
        rc.log("warn", format!("GitHub GraphQL: {}", errors.join("; ")));
    }
    Ok(data)
}

#[derive(Deserialize)]
struct RestArgs {
    method: String,
    path: String,
    body: Option<Value>,
}

async fn github_rest(rc: &Arc<RunCtx>, a: RestArgs) -> OpResult {
    let token = github_token()?;
    // The token only ever goes to GitHub's API.
    let url = if a.path.starts_with(&format!("{GITHUB_API}/")) {
        a.path.clone()
    } else if a.path.starts_with('/') {
        format!("{GITHUB_API}{}", a.path)
    } else {
        return Err(OpError::msg("the path must start with / (e.g. /repos/owner/repo/issues)"));
    };
    let method = reqwest::Method::from_bytes(a.method.to_uppercase().as_bytes())
        .map_err(|_| OpError::msg("bad HTTP method"))?;
    let mut req = rc
        .http()
        .request(method, url)
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(b) = &a.body {
        req = req.json(b);
    }
    let resp = req.send().await.map_err(|e| OpError::Msg(e.to_string()))?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| OpError::Msg(e.to_string()))?;
    if !status.is_success() {
        return Err(github_error(status, &body));
    }
    if body.trim().is_empty() {
        return Ok(Value::Null);
    }
    Ok(serde_json::from_str(&body).unwrap_or(Value::String(body)))
}
