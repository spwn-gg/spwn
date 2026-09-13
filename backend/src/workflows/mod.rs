//! Project workflows: JavaScript (or TypeScript) files in a project's
//! `.spwn/workflows/` that orchestrate agents.
//!
//! A workflow is an ES module with a default-exported `main(spwn, inputs)` and an
//! optional `meta` export:
//!
//! ```js
//! export const meta = { name: "Board", keepAlive: true, inputs: { project: { default: "" } } };
//! export default async function main(spwn, inputs) { … }
//! ```
//!
//! spwn has no opinion about what a workflow does — personas, polling, which agent works
//! which ticket are all the script's business. spwn provides the `spwn` API (sessions,
//! headless runs, hooks, state, exec, fetch, GitHub) and runs the script: on demand from
//! the UI, or kept alive (restarted with backoff) when `meta.keepAlive` is set.
//!
//! Each run gets its own OS thread with its own QuickJS runtime (see [`runtime`]).
//! Anything that touches spwn's state is done on the server's runtime (see [`host`]).
//!
//! Files starting with `_`, `.d.ts` files and anything in a subdirectory (`lib/`) are
//! importable modules, not workflows.

pub mod host;
mod runtime;
mod ts;

#[cfg(test)]
mod tests;

use crate::hooks;
use crate::state::AppState;
use host::{Flag, RunCtx};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant, SystemTime};
use uuid::Uuid;

/// File extensions a workflow may have.
const EXTENSIONS: &[&str] = &["js", "mjs", "ts", "mts"];

/// Log lines kept per run.
const LOG_CAP: usize = 2000;

/// Finished runs remembered (per process) for the Workflows panel.
const FINISHED_RUNS_KEPT: usize = 50;

/// First delay before a keep-alive workflow is restarted; doubles up to
/// [`RESTART_BACKOFF_MAX`], and resets after a run that stayed up a while.
#[cfg(not(test))]
const RESTART_BACKOFF: Duration = Duration::from_secs(5);
#[cfg(test)]
const RESTART_BACKOFF: Duration = Duration::from_millis(50);
const RESTART_BACKOFF_MAX: Duration = Duration::from_secs(300);
const STABLE_AFTER: Duration = Duration::from_secs(600);

/// A workflow's `export const meta`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Inputs the Run form asks for: `{ key: { type?, default?, description?, required? } }`.
    #[serde(default)]
    pub inputs: serde_json::Map<String, Value>,
    /// Restart the workflow (with backoff) whenever it exits or throws, until stopped.
    #[serde(default)]
    pub keep_alive: bool,
}

/// One discovered workflow, for the panel.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowInfo {
    /// The file stem — what runs, autostart and session tags refer to it by.
    pub name: String,
    /// Path relative to the project dir.
    pub file: String,
    pub meta: Option<Meta>,
    /// Why `meta` couldn't be read (a syntax error, say).
    pub error: Option<String>,
    pub autostart: bool,
    /// The current (or most recent) run.
    pub run: Option<RunInfo>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowListing {
    /// Whether the user has allowed this project's workflows to run.
    pub enabled: bool,
    /// `.spwn/workflows`, relative to the project dir.
    pub dir: String,
    pub workflows: Vec<WorkflowInfo>,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Running,
    Stopping,
    /// A keep-alive workflow exited and is waiting out its backoff.
    Restarting,
    Finished,
    Failed,
    Stopped,
}

impl RunStatus {
    pub fn is_live(self) -> bool {
        matches!(self, Self::Running | Self::Stopping | Self::Restarting)
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RunInfo {
    pub id: String,
    pub project_id: String,
    pub workflow: String,
    pub status: RunStatus,
    /// `manual` | `autostart`.
    pub trigger: String,
    pub inputs: Value,
    /// Epoch ms.
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub error: Option<String>,
    pub restarts: u32,
    /// Sessions this run created or claimed.
    pub sessions: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    /// Epoch ms.
    pub at: i64,
    /// `debug` | `info` | `warn` | `error` | `hook`.
    pub level: String,
    pub msg: String,
}

/// A run of a workflow — across keep-alive restarts, which share its id and log.
pub struct Run {
    pub id: String,
    info: Mutex<RunInfo>,
    log: Mutex<VecDeque<LogLine>>,
    /// Set when the user (or disabling the project) stops the run.
    pub cancel: Flag,
}

impl Run {
    pub fn info(&self) -> RunInfo {
        self.info.lock().clone()
    }

    pub fn log(&self, state: &AppState, level: &str, msg: impl Into<String>) {
        let line = LogLine {
            at: now_ms(),
            level: level.to_string(),
            msg: msg.into(),
        };
        {
            let mut log = self.log.lock();
            if log.len() >= LOG_CAP {
                log.pop_front();
            }
            log.push_back(line.clone());
        }
        state.hub.emit(&format!("workflow://log/{}", self.id), &line);
    }

    pub fn add_session(&self, state: &AppState, terminal_id: &str) {
        let changed = {
            let mut info = self.info.lock();
            if info.sessions.iter().any(|s| s == terminal_id) {
                false
            } else {
                info.sessions.push(terminal_id.to_string());
                true
            }
        };
        if changed {
            self.publish(state);
        }
    }

    fn update(&self, state: &AppState, f: impl FnOnce(&mut RunInfo)) {
        f(&mut self.info.lock());
        self.publish(state);
    }

    fn publish(&self, state: &AppState) {
        state.hub.emit("workflow://runs", self.info());
    }
}

/// Runs, and which run owns which session.
#[derive(Default)]
pub struct Manager {
    runs: Mutex<Vec<Arc<Run>>>,
    /// terminal id → the live run context that created or claimed it. Decides who
    /// answers that session's hook prompts.
    owners: Mutex<HashMap<String, Weak<RunCtx>>>,
    /// The server's runtime: everything that touches sessions runs there.
    main: OnceLock<tokio::runtime::Handle>,
    /// path → (mtime, len, meta or error), so listing doesn't re-evaluate every file.
    meta_cache: Mutex<HashMap<PathBuf, (Option<SystemTime>, u64, Result<Meta, String>)>>,
}

impl Manager {
    /// Remember the server's runtime. Called once at boot, before anything starts.
    pub fn init(&self, handle: tokio::runtime::Handle) {
        let _ = self.main.set(handle);
    }

    fn main(&self) -> Result<tokio::runtime::Handle, String> {
        self.main
            .get()
            .cloned()
            .ok_or_else(|| "workflows aren't initialized".to_string())
    }

    pub(crate) fn claim(&self, terminal_id: &str, owner: &Arc<RunCtx>) {
        self.owners
            .lock()
            .insert(terminal_id.to_string(), Arc::downgrade(owner));
    }

    fn owner(&self, terminal_id: &str) -> Option<Arc<RunCtx>> {
        let mut owners = self.owners.lock();
        match owners.get(terminal_id).map(Weak::upgrade) {
            Some(Some(rc)) => Some(rc),
            Some(None) => {
                owners.remove(terminal_id);
                None
            }
            None => None,
        }
    }

    /// Drop a deleted session's ownership.
    pub fn forget_terminal(&self, terminal_id: &str) {
        self.owners.lock().remove(terminal_id);
    }

    fn find_run(&self, run_id: &str) -> Option<Arc<Run>> {
        self.runs.lock().iter().find(|r| r.id == run_id).cloned()
    }

    fn live_run(&self, project_id: &str, name: &str) -> Option<Arc<Run>> {
        self.runs
            .lock()
            .iter()
            .rev()
            .find(|r| {
                let i = r.info.lock();
                i.project_id == project_id && i.workflow == name && i.status.is_live()
            })
            .cloned()
    }

    fn remember(&self, run: Arc<Run>) {
        let mut runs = self.runs.lock();
        runs.push(run);
        // Forget the oldest finished runs; live runs are never dropped.
        let finished = runs.iter().filter(|r| !r.info.lock().status.is_live()).count();
        let mut excess = finished.saturating_sub(FINISHED_RUNS_KEPT);
        runs.retain(|r| {
            if excess > 0 && !r.info.lock().status.is_live() {
                excess -= 1;
                false
            } else {
                true
            }
        });
    }

    fn meta(&self, path: &Path, root: &Path) -> Result<Meta, String> {
        let fs_meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
        let (mtime, len) = (fs_meta.modified().ok(), fs_meta.len());
        if let Some((m, l, cached)) = self.meta_cache.lock().get(path) {
            if *m == mtime && *l == len {
                return cached.clone();
            }
        }
        let meta = runtime::read_meta(path, root);
        self.meta_cache
            .lock()
            .insert(path.to_path_buf(), (mtime, len, meta.clone()));
        meta
    }
}

/// Who answers a session's hook prompts: the workflow run that owns it, nobody (it
/// belongs to a workflow that isn't running), or the UI (it isn't a workflow's).
pub fn prompt_mode_for(state: &AppState, terminal_id: &str) -> hooks::PromptMode {
    if let Some(rc) = state.workflows.owner(terminal_id) {
        if !rc.is_stopping() {
            return hooks::PromptMode::Workflow(host::prompter(Arc::downgrade(&rc)));
        }
    }
    let tagged = state
        .store
        .lock()
        .terminal(terminal_id)
        .is_some_and(|t| t.workflow.is_some());
    if tagged {
        hooks::PromptMode::Decline
    } else {
        hooks::PromptMode::Ui
    }
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

pub fn workflows_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(".spwn").join("workflows")
}

/// The workflows in a `.spwn/workflows` dir, by name, sorted. Only top-level files:
/// subdirectories (`lib/`), `_`-prefixed files and `.d.ts` files are modules to import.
pub fn discover(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .filter_map(|p| {
            let file = p.file_name()?.to_str()?;
            if file.starts_with('_') || file.starts_with('.') || file.ends_with(".d.ts") {
                return None;
            }
            let ext = p.extension()?.to_str()?;
            if !EXTENSIONS.contains(&ext) {
                return None;
            }
            let stem = p.file_stem()?.to_str()?.to_string();
            Some((stem, p))
        })
        .collect();
    found.sort();
    // `board.js` and `board.ts` would be the same workflow; the first one wins.
    found.dedup_by(|a, b| a.0 == b.0);
    found
}

fn project(state: &AppState, project_id: &str) -> Result<crate::store::ProjectRec, String> {
    state
        .store
        .lock()
        .project(project_id)
        .cloned()
        .ok_or_else(|| "no such project".to_string())
}

/// The project's workflows with their meta, autostart flag and latest run.
pub fn list(state: &AppState, project_id: &str) -> Result<WorkflowListing, String> {
    let p = project(state, project_id)?;
    let project_dir = PathBuf::from(&p.directory);
    let dir = workflows_dir(&project_dir);
    let runs = state.workflows.runs.lock().clone();
    let workflows = discover(&dir)
        .into_iter()
        .map(|(name, path)| {
            let (meta, error) = match state.workflows.meta(&path, &dir) {
                Ok(m) => (Some(m), None),
                Err(e) => (None, Some(e)),
            };
            let run = runs
                .iter()
                .rev()
                .map(|r| r.info())
                .find(|i| i.project_id == project_id && i.workflow == name);
            WorkflowInfo {
                autostart: p.workflows.autostart.contains(&name),
                file: path
                    .strip_prefix(&project_dir)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned(),
                name,
                meta,
                error,
                run,
            }
        })
        .collect();
    Ok(WorkflowListing {
        enabled: p.workflows.enabled,
        dir: ".spwn/workflows".to_string(),
        workflows,
    })
}

/// The API's type definitions, written next to a project's workflows for editors.
const TYPES: &str = include_str!("spwn.d.ts");

fn template(name: &str, typescript: bool) -> String {
    let body = r#"  const session = await spwn.sessions.create({
    title: "__NAME__",
    prompt: "Summarize what this repository does in three sentences.",
  });
  const turn = await session.waitForTurn();
  if (turn.blocked) {
    spwn.warn("The session is waiting for input:\n" + turn.screen);
    return;
  }
  spwn.log(turn.text);
}
"#;
    let head = if typescript {
        r#"import type { Spwn, WorkflowMeta } from "./spwn";

export const meta: WorkflowMeta = {
  description: "Ask an agent about the repository",
};

export default async function main(spwn: Spwn, inputs: Record<string, unknown>) {
"#
    } else {
        r#"// @ts-check

/** @type {import("./spwn").WorkflowMeta} */
export const meta = {
  description: "Ask an agent about the repository",
};

/** @param {import("./spwn").Spwn} spwn */
export default async function main(spwn, inputs) {
"#
    };
    format!("{head}{}", body.replace("__NAME__", name))
}

/// Create `.spwn/workflows/<name>.js` (or `.ts`) from a starter template, and write the
/// API's `spwn.d.ts` beside it. Returns the new file's path, relative to the project.
pub fn scaffold(state: &AppState, project_id: &str, name: &str, typescript: bool) -> Result<String, String> {
    let name = name.trim();
    let valid = !name.is_empty()
        && !name.starts_with('_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !valid {
        return Err("name it with letters, digits, - and _ (not starting with _)".to_string());
    }
    let p = project(state, project_id)?;
    let project_dir = PathBuf::from(&p.directory);
    let dir = workflows_dir(&project_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if discover(&dir).iter().any(|(n, _)| n == name) {
        return Err(format!("a workflow named `{name}` already exists"));
    }
    let file = dir.join(format!("{name}.{}", if typescript { "ts" } else { "js" }));
    std::fs::write(&file, template(name, typescript)).map_err(|e| e.to_string())?;
    let types = dir.join("spwn.d.ts");
    if std::fs::read_to_string(&types).ok().as_deref() != Some(TYPES) {
        std::fs::write(&types, TYPES).map_err(|e| e.to_string())?;
    }
    Ok(file
        .strip_prefix(&project_dir)
        .unwrap_or(&file)
        .to_string_lossy()
        .into_owned())
}

pub fn runs(state: &AppState, project_id: &str) -> Vec<RunInfo> {
    state
        .workflows
        .runs
        .lock()
        .iter()
        .map(|r| r.info())
        .filter(|i| i.project_id == project_id)
        .collect()
}

pub fn log(state: &AppState, run_id: &str) -> Result<Vec<LogLine>, String> {
    let run = state
        .workflows
        .find_run(run_id)
        .ok_or_else(|| "no such run".to_string())?;
    let lines = run.log.lock().iter().cloned().collect();
    Ok(lines)
}

// ---------------------------------------------------------------------------
// Trust + autostart
// ---------------------------------------------------------------------------

pub fn set_enabled(state: &Arc<AppState>, project_id: &str, enabled: bool) -> Result<(), String> {
    {
        let mut store = state.store.lock();
        let p = store
            .project_mut(project_id)
            .ok_or_else(|| "no such project".to_string())?;
        p.workflows.enabled = enabled;
    }
    crate::commands::persist(state);
    if enabled {
        autostart_project(state, project_id);
    } else {
        for run in state.workflows.runs.lock().iter() {
            if run.info.lock().project_id == project_id {
                run.cancel.set();
            }
        }
    }
    Ok(())
}

pub fn set_autostart(
    state: &Arc<AppState>,
    project_id: &str,
    name: &str,
    autostart: bool,
) -> Result<(), String> {
    {
        let mut store = state.store.lock();
        let p = store
            .project_mut(project_id)
            .ok_or_else(|| "no such project".to_string())?;
        p.workflows.autostart.retain(|n| n != name);
        if autostart {
            p.workflows.autostart.push(name.to_string());
        }
    }
    crate::commands::persist(state);
    if autostart && state.workflows.live_run(project_id, name).is_none() {
        // Turning autostart on for an enabled project starts it now, rather than at
        // some future restart the user would have to trigger to see it work.
        if project(state, project_id)?.workflows.enabled {
            start(state, project_id, name, Value::Null, "autostart")?;
        }
    }
    Ok(())
}

/// Start every autostart workflow of every enabled project. Called once at boot.
pub fn autostart_all(state: &Arc<AppState>) {
    let ids: Vec<String> = state
        .store
        .lock()
        .projects
        .iter()
        .filter(|p| p.workflows.enabled && !p.workflows.autostart.is_empty())
        .map(|p| p.id.clone())
        .collect();
    for id in ids {
        autostart_project(state, &id);
    }
}

fn autostart_project(state: &Arc<AppState>, project_id: &str) {
    let names = state
        .store
        .lock()
        .project(project_id)
        .map(|p| p.workflows.autostart.clone())
        .unwrap_or_default();
    for name in names {
        if state.workflows.live_run(project_id, &name).is_some() {
            continue;
        }
        if let Err(e) = start(state, project_id, &name, Value::Null, "autostart") {
            eprintln!("workflow {name}: autostart failed: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Fill in `meta.inputs` defaults and check required inputs are present.
fn resolve_inputs(meta: &Meta, given: Value) -> Result<Value, String> {
    let mut inputs = match given {
        Value::Object(m) => m,
        Value::Null => serde_json::Map::new(),
        _ => return Err("inputs must be an object".to_string()),
    };
    for (key, spec) in &meta.inputs {
        let missing = inputs.get(key).is_none_or(|v| v.is_null() || v == "");
        if !missing {
            continue;
        }
        if let Some(default) = spec.get("default") {
            inputs.insert(key.clone(), default.clone());
        } else if spec.get("required").and_then(Value::as_bool) == Some(true) {
            return Err(format!("input `{key}` is required"));
        }
    }
    Ok(Value::Object(inputs))
}

/// Start a workflow. Refuses if the project hasn't enabled workflows or this workflow is
/// already running.
pub fn start(
    state: &Arc<AppState>,
    project_id: &str,
    name: &str,
    inputs: Value,
    trigger: &str,
) -> Result<RunInfo, String> {
    let p = project(state, project_id)?;
    if !p.workflows.enabled {
        return Err("workflows are not enabled for this project".to_string());
    }
    let project_dir = std::fs::canonicalize(&p.directory).unwrap_or_else(|_| PathBuf::from(&p.directory));
    let dir = workflows_dir(&project_dir);
    let (_, entry) = discover(&dir)
        .into_iter()
        .find(|(n, _)| n == name)
        .ok_or_else(|| format!("no workflow named `{name}` in .spwn/workflows"))?;
    if state.workflows.live_run(project_id, name).is_some() {
        return Err(format!("`{name}` is already running"));
    }
    let meta = state.workflows.meta(&entry, &dir)?;
    let inputs = resolve_inputs(&meta, inputs)?;
    let main = state.workflows.main()?;

    let run = Arc::new(Run {
        id: Uuid::new_v4().to_string(),
        info: Mutex::new(RunInfo {
            id: String::new(),
            project_id: project_id.to_string(),
            workflow: name.to_string(),
            status: RunStatus::Running,
            trigger: trigger.to_string(),
            inputs: inputs.clone(),
            started_at: now_ms(),
            ended_at: None,
            error: None,
            restarts: 0,
            sessions: Vec::new(),
        }),
        log: Mutex::new(VecDeque::new()),
        cancel: Flag::default(),
    });
    run.info.lock().id = run.id.clone();
    state.workflows.remember(run.clone());
    run.publish(state);

    let job = Job {
        state: state.clone(),
        run: run.clone(),
        project_id: project_id.to_string(),
        project_dir,
        dir,
        entry,
        name: name.to_string(),
        inputs,
        keep_alive: meta.keep_alive,
        main,
    };
    std::thread::Builder::new()
        .name(format!("workflow-{name}"))
        .spawn(move || job.supervise())
        .map_err(|e| format!("couldn't start a thread for the workflow: {e}"))?;
    Ok(run.info())
}

pub fn stop(state: &AppState, run_id: &str) -> Result<(), String> {
    let run = state
        .workflows
        .find_run(run_id)
        .ok_or_else(|| "no such run".to_string())?;
    if run.info().status.is_live() {
        run.update(state, |i| {
            if i.status == RunStatus::Running {
                i.status = RunStatus::Stopping;
            }
        });
        run.cancel.set();
    }
    Ok(())
}

/// Everything a run's thread needs.
struct Job {
    state: Arc<AppState>,
    run: Arc<Run>,
    project_id: String,
    project_dir: PathBuf,
    dir: PathBuf,
    entry: PathBuf,
    name: String,
    inputs: Value,
    keep_alive: bool,
    main: tokio::runtime::Handle,
}

impl Job {
    /// Run the workflow on this thread, restarting a keep-alive workflow until stopped.
    fn supervise(self) {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                self.finish(RunStatus::Failed, Some(format!("couldn't start a runtime: {e}")));
                return;
            }
        };
        self.fire_hook("workflow-started", None);
        // A crash in spwn's own code must still end the run, not leave it "running".
        let (status, error) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rt.block_on(self.attempts())))
                .unwrap_or_else(|_| {
                    (RunStatus::Failed, Some("spwn crashed running this workflow".to_string()))
                });
        self.finish(status, error.clone());
        let label = match status {
            RunStatus::Stopped => "stopped",
            RunStatus::Failed => "error",
            _ => "ok",
        };
        self.fire_hook("workflow-stopped", Some(label));
    }

    async fn attempts(&self) -> (RunStatus, Option<String>) {
        let mut backoff = RESTART_BACKOFF;
        loop {
            let began = Instant::now();
            let rc = Arc::new(RunCtx::new(
                self.state.clone(),
                self.run.clone(),
                self.project_id.clone(),
                self.project_dir.clone(),
                self.name.clone(),
                self.inputs.clone(),
                self.main.clone(),
            ));
            let outcome = runtime::run_script(rc.clone(), &self.entry, &self.dir).await;
            rc.wind_down();
            drop(rc);

            if self.run.cancel.is_set() {
                self.run.log(&self.state, "info", "stopped");
                return (RunStatus::Stopped, None);
            }
            match &outcome {
                Ok(()) if !self.keep_alive => return (RunStatus::Finished, None),
                Err(e) if !self.keep_alive => {
                    self.run.log(&self.state, "error", e.clone());
                    return (RunStatus::Failed, Some(e.clone()));
                }
                Ok(()) => self.run.log(&self.state, "info", "exited"),
                Err(e) => self.run.log(&self.state, "error", e.clone()),
            }

            if began.elapsed() >= STABLE_AFTER {
                backoff = RESTART_BACKOFF;
            }
            self.run.log(
                &self.state,
                "info",
                format!("restarting in {}s", backoff.as_secs_f32()),
            );
            self.run.update(&self.state, |i| {
                i.status = RunStatus::Restarting;
                i.error = outcome.err();
            });
            tokio::select! {
                _ = tokio::time::sleep(backoff) => {}
                _ = self.run.cancel.wait() => {
                    self.run.log(&self.state, "info", "stopped");
                    return (RunStatus::Stopped, None);
                }
            }
            backoff = (backoff * 2).min(RESTART_BACKOFF_MAX);
            self.run.update(&self.state, |i| {
                i.status = RunStatus::Running;
                i.restarts += 1;
            });
        }
    }

    fn finish(&self, status: RunStatus, error: Option<String>) {
        self.run.update(&self.state, |i| {
            i.status = status;
            i.ended_at = Some(now_ms());
            if error.is_some() {
                i.error = error;
            }
        });
    }

    /// Run the `workflow-started` / `workflow-stopped` hooks (both scopes, project dir).
    fn fire_hook(&self, event: &str, status: Option<&str>) {
        debug_assert!(hooks::WORKFLOW_EVENTS.contains(&event));
        let entries = hooks::discover_all(
            crate::commands::enabled_global_hooks_dir(&self.state).as_deref(),
            &self.project_dir,
            event,
        );
        if entries.is_empty() {
            return;
        }
        let mut extra_env = vec![
            ("SPWN_WORKFLOW".to_string(), self.name.clone()),
            ("SPWN_WORKFLOW_RUN_ID".to_string(), self.run.id.clone()),
        ];
        if let Some(s) = status {
            extra_env.push(("SPWN_WORKFLOW_STATUS".to_string(), s.to_string()));
        }
        let ctx = hooks::HookCtx {
            terminal_id: String::new(),
            project_dir: self.project_dir.to_string_lossy().into_owned(),
            worktree: self.project_dir.clone(),
            branch: None,
            base_branch: None,
            session_id: None,
            turn_uuid: None,
            exec: None,
            extra_env,
        };
        let runs = hooks::run_entries(
            &ctx,
            event,
            &entries,
            &mut |line| self.run.log(&self.state, "hook", line),
            // Nobody is there to answer a workflow event's hook.
            &mut |_| hooks::PROMPT_DECLINED.to_string(),
        );
        for r in runs.iter().filter(|r| !r.ok) {
            self.run.log(
                &self.state,
                "warn",
                format!("{event} hook {} failed (exit {:?})", r.script, r.exit_code),
            );
        }
    }
}
