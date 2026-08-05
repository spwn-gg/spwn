//! Shell hooks: one script per session lifecycle event that spwn runs with useful
//! environment variables. This replaces the old opinionated per-session
//! docker-compose integration with a generic, unix-y mechanism — spwn just runs the
//! script; it has no opinion about what it does (and the script is free to
//! orchestrate other files/code). spwn's own built-in per-session behaviors
//! (worktree create/remove, heavy-dir seeding, per-turn commit + checkpoint) ship as
//! *default global hooks* (see [`install_default_global_hooks`]), so they're editable
//! and overridable instead of hardcoded.
//!
//! Discovery is layered across two [`Scope`]s, global first then repo, and each scope
//! resolves to a root — `~/.spwn/hooks` (global, every session everywhere) and
//! `<worktree>/.spwn/hooks` (repo, committed with a repo). Within a root an event runs,
//! in order: a bare `<event>.sh` (if present), then every runnable file in `<event>.d/`
//! sorted by filename — so numeric prefixes (`10-`, `20-`) order independent steps and
//! users can drop their own `NN-*.sh` alongside spwn's defaults without editing them.
//! A file runs directly when it's executable (honoring its shebang); otherwise via `sh`.
//!
//! Events (see [`EVENTS`]): `session-created`, `session-ready`, `session-turn`,
//! `session-deleted`. Most hooks run with the worktree as their cwd, EXCEPT the
//! *global* `session-created` / `session-deleted` scripts, which run in the *project
//! dir* — the global `session-created` script creates the worktree (which doesn't
//! exist yet), and the global `session-deleted` script removes it.
//!
//! Injected environment (a hook reads these): `SPWN_EVENT`, `SPWN_TERMINAL_ID`,
//! `SPWN_PROJECT_DIR`, `SPWN_WORKTREE`, `SPWN_BRANCH`, `SPWN_BASE_BRANCH`,
//! `SPWN_SESSION_ID`, `SPWN_TURN_UUID`, `SPWN_EXEC` (all but the first four only when
//! known).
//!
//! Callback: a hook reports values back to spwn by printing a sentinel line
//! `::spwn:set:: key=value` (one key per line — values may contain spaces). spwn
//! parses these out of the stream (they never appear in captured output) — the global
//! `session-created` script uses `worktree=`/`branch=`/`base=` to tell spwn which
//! worktree it made.
//!
//! The other recognized `session-created` keys describe an *environment* the hook
//! stood up — a container, a VM, a remote host — that the session's processes should
//! run inside:
//!
//!   * `exec` — a command prefix spwn prepends to every interactive pane's argv, e.g.
//!     `docker exec -it -w <worktree> <container>`. For a TUI agent this MUST allocate
//!     a tty, or nothing renders and the agent's `detect` rules never match.
//!   * `execHeadless` — the same for scheduled runs, which parse line-delimited JSON
//!     and therefore must NOT get a tty. Absent → scheduled runs stay on the host.
//!   * `execBin` — the agent binary inside the environment (default: the agent
//!     definition's bare `binary.name`, resolved by the environment's own PATH).
//!   * `execShell` — the shell for shell panes inside it (default `/bin/sh`).
//!
//! spwn never parses the prefix beyond splitting it into argv, and has no idea whether
//! it names Docker or anything else — the same reason worktree creation lives in a
//! script rather than here.
//!
//! Style mirrors `gitwt.rs`/the old `compose.rs`: thin `std::process::Command`
//! shell-outs, best-effort, never fail a session.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The lifecycle events spwn fires hooks for (also the discoverable file stems).
pub const EVENTS: &[&str] = &[
    "session-created",
    "session-ready",
    "session-turn",
    "session-deleted",
];

/// Prefix of a callback line a hook prints to report a value back to spwn:
/// `::spwn:set:: key=value` (one key per line). Parsed out of the stream, so these
/// lines never appear in the captured/streamed output.
const SET_SENTINEL: &str = "::spwn:set::";

/// Which layer a discovered hook belongs to. Global hooks (in `~/.spwn/hooks`) apply
/// to every session; repo hooks (in `<worktree>/.spwn/hooks`) ship with a repo. When
/// both exist for an event, global runs first, then repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Global,
    Repo,
}

/// The shared, cross-session global hooks dir: `~/.spwn/hooks`. None if the home dir
/// can't be resolved.
pub fn global_hooks_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().join(".spwn").join("hooks"))
}

/// Cap on captured hook output kept per run (tail), so a chatty script can't bloat
/// the store / the panel.
const OUTPUT_CAP: usize = 8192;

/// Env var pointing hooks at the per-run prompt socket. A hook raises a UI prompt by
/// running the `spwn prompt` helper, which connects here — the hook's own stdin/stdout
/// are never touched, so there's no bare-`read` footgun.
const PROMPT_SOCK_ENV: &str = "SPWN_PROMPT_SOCK";

/// Env var giving hooks the path to the spwn binary, so `"$SPWN_BIN" prompt …` works
/// even when spwn isn't on `PATH`.
const PROMPT_BIN_ENV: &str = "SPWN_BIN";

/// Sentinel the runner's `on_prompt` callback returns when a prompt can't be answered
/// by a human (headless run, timeout, or the window went away). Mapped to a `declined`
/// socket response so the helper exits non-zero and the script can branch.
pub const PROMPT_DECLINED: &str = "__SPWN_DECLINED__";

/// Monotonic counter making each run's socket filename unique within this process.
static SOCK_SEQ: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Context + result model
// ---------------------------------------------------------------------------

/// Everything a hook run needs about the session it acts on.
pub struct HookCtx {
    pub terminal_id: String,
    pub project_dir: String,
    /// The session's worktree — where most hooks run (the global `session-created`
    /// script runs in `project_dir` instead, since it *creates* this path).
    pub worktree: PathBuf,
    pub branch: Option<String>,
    pub base_branch: Option<String>,
    pub session_id: Option<String>,
    /// The assistant turn id, set only when firing `session-turn`.
    pub turn_uuid: Option<String>,
    /// The command prefix reaching this session's environment, if a hook stood one up
    /// (`::spwn:set:: exec=…`). Surfaced as `SPWN_EXEC` so a later hook can run
    /// commands *inside* that environment, or tear it down on delete.
    pub exec: Option<String>,
}

/// The result of running an event's hook script.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRun {
    pub event: String,
    /// Which layer this run's script came from.
    pub scope: Scope,
    /// Hook file basename (e.g. `session-created.sh`).
    pub script: String,
    /// Process exit code, or None if the script couldn't be launched / was signalled.
    pub exit_code: Option<i32>,
    pub ok: bool,
    /// Combined stdout+stderr tail.
    pub output: String,
    /// Epoch seconds when the run finished.
    pub at: u64,
    /// Values the hook reported via `::spwn:set:: key=value` lines. Not sent to the UI.
    #[serde(skip)]
    pub reported: BTreeMap<String, String>,
}

/// One discovered hook script (a scope+file) plus its most recent run, for the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookScriptInfo {
    pub scope: Scope,
    /// The hook file basename (e.g. `session-created.sh`).
    pub script: String,
    pub last_run: Option<HookRun>,
}

/// The discovered hook scripts (0..2, global first) for one event (for the UI).
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HookEventInfo {
    pub event: String,
    pub scripts: Vec<HookScriptInfo>,
}

/// Overall hooks status for a session, for the Hooks panel.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HooksStatus {
    /// Whether this session has a worktree to discover hooks in.
    pub available: bool,
    pub events: Vec<HookEventInfo>,
    /// The event whose hook is executing right now (if any), so a freshly-opened
    /// panel shows the running state without waiting for the next stream event.
    pub running: Option<String>,
}

/// One selectable option in a hook UI prompt (mirrors the frontend `QuestionSpec`
/// option shape so the existing picker renders it unchanged).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookPromptOption {
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// A multiple-choice prompt a hook raised for the user, parsed from a `SPWN_PROMPT`
/// stdout line. Deserialized from the script's JSON; serialized to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookPromptRequest {
    pub question: String,
    #[serde(default)]
    pub header: Option<String>,
    #[serde(default)]
    pub multi_select: bool,
    pub options: Vec<HookPromptOption>,
}

/// The reply spwn sends back over the prompt socket: either the user's chosen label(s)
/// or a decline (no UI / timed out).
#[cfg(unix)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PromptResponse {
    #[serde(default)]
    answer: Option<String>,
    #[serde(default)]
    declined: bool,
}

/// A message from the hook's IO threads to the runner's main loop: either an output
/// line, or a prompt request paired with a sender the loop replies on. Keeping prompts
/// on the main loop lets `on_prompt` stay a plain `&mut dyn FnMut` (no `Send` needed).
enum HookMsg {
    Line(String),
    #[cfg(unix)]
    Prompt(HookPromptRequest, mpsc::Sender<String>),
}

// ---------------------------------------------------------------------------
// Prompt socket (server side, in the app) + `spwn prompt` client (in hooks)
// ---------------------------------------------------------------------------

/// Serve one prompt connection: read the request line, hand it to the runner loop, wait
/// for the answer, and write the response back. Runs on the socket-accept thread.
#[cfg(unix)]
fn handle_prompt_conn(mut stream: UnixStream, tx: &mpsc::Sender<HookMsg>) {
    let _ = stream.set_nonblocking(false);
    let Ok(read_half) = stream.try_clone() else { return };
    let mut line = String::new();
    if BufReader::new(read_half).read_line(&mut line).is_err() {
        return;
    }
    let Ok(req) = serde_json::from_str::<HookPromptRequest>(line.trim()) else {
        let _ = writeln!(stream, "{{\"declined\":true}}");
        return;
    };
    if req.options.is_empty() {
        let _ = writeln!(stream, "{{\"declined\":true}}");
        return;
    }
    let (ans_tx, ans_rx) = mpsc::channel::<String>();
    if tx.send(HookMsg::Prompt(req, ans_tx)).is_err() {
        return;
    }
    let answer = ans_rx.recv().unwrap_or_else(|_| PROMPT_DECLINED.to_string());
    let resp = if answer == PROMPT_DECLINED {
        PromptResponse { answer: None, declined: true }
    } else {
        PromptResponse { answer: Some(answer), declined: false }
    };
    if let Ok(json) = serde_json::to_string(&resp) {
        let _ = writeln!(stream, "{json}");
    }
}

/// Accept prompt connections until `stop` is set (the hook has exited). Non-blocking so
/// a hook that never prompts doesn't leave this thread wedged in `accept`.
#[cfg(unix)]
fn serve_prompt_socket(listener: UnixListener, stop: Arc<AtomicBool>, tx: mpsc::Sender<HookMsg>) {
    let _ = listener.set_nonblocking(true);
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => handle_prompt_conn(stream, &tx),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => break,
        }
    }
}

/// Outcome of the `spwn prompt` client: process exit code plus the single line to print
/// to stdout (the chosen label(s)), kept separate so the entry point owns the actual
/// printing and this stays unit-testable.
pub struct PromptCliOutcome {
    pub code: i32,
    pub stdout: Option<String>,
}

/// Parse `spwn prompt` args into a request. Grammar:
/// `[--multi] [--header H] "Question" [option ...]`. With no options, defaults to a
/// Yes/No confirm. Returns None (usage error) when no question is given.
#[cfg(unix)]
fn parse_prompt_args(args: &[String]) -> Option<HookPromptRequest> {
    let mut multi_select = false;
    let mut header: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--multi" => multi_select = true,
            "--header" => {
                i += 1;
                header = args.get(i).cloned();
            }
            s if s.starts_with("--header=") => {
                header = Some(s["--header=".len()..].to_string());
            }
            _ => positional.push(args[i].clone()),
        }
        i += 1;
    }
    if positional.is_empty() {
        return None;
    }
    let question = positional.remove(0);
    let labels = if positional.is_empty() {
        vec!["Yes".to_string(), "No".to_string()]
    } else {
        positional
    };
    let options = labels
        .into_iter()
        .map(|label| HookPromptOption { label, description: None })
        .collect();
    Some(HookPromptRequest { question, header, multi_select, options })
}

/// Client for `spwn prompt …`, invoked by hooks. Connects to the run's prompt socket,
/// sends the request, and reports the answer. Exit codes: 0 = answered (label on
/// stdout), 2 = declined (no UI / timed out), 3 = usage error / not inside a hook.
#[cfg(unix)]
pub fn run_prompt_cli(args: &[String]) -> PromptCliOutcome {
    let usage = || {
        eprintln!(
            "usage: spwn prompt [--multi] [--header TEXT] \"Question?\" [option ...]\n\
             (no options → a Yes/No confirm). Prints the chosen label; exits 2 if declined."
        );
        PromptCliOutcome { code: 3, stdout: None }
    };
    let Some(req) = parse_prompt_args(args) else {
        return usage();
    };
    let sock = match std::env::var(PROMPT_SOCK_ENV) {
        Ok(s) if !s.is_empty() => s,
        _ => {
            eprintln!("spwn prompt: not running inside a spwn hook ({PROMPT_SOCK_ENV} unset)");
            return PromptCliOutcome { code: 3, stdout: None };
        }
    };
    let mut stream = match UnixStream::connect(&sock) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("spwn prompt: can't reach spwn ({e})");
            return PromptCliOutcome { code: 3, stdout: None };
        }
    };
    let Ok(line) = serde_json::to_string(&req) else {
        return PromptCliOutcome { code: 3, stdout: None };
    };
    if writeln!(stream, "{line}").is_err() {
        return PromptCliOutcome { code: 3, stdout: None };
    }
    let mut resp = String::new();
    if BufReader::new(&stream).read_line(&mut resp).is_err() {
        return PromptCliOutcome { code: 2, stdout: None };
    }
    match serde_json::from_str::<PromptResponse>(resp.trim()) {
        Ok(PromptResponse { answer: Some(a), declined: false }) => {
            PromptCliOutcome { code: 0, stdout: Some(a) }
        }
        // Declined, or a malformed/empty reply → treat as declined.
        _ => PromptCliOutcome { code: 2, stdout: None },
    }
}

/// Non-unix stub: prompts require the unix-socket transport, so decline.
#[cfg(not(unix))]
pub fn run_prompt_cli(_args: &[String]) -> PromptCliOutcome {
    eprintln!("spwn prompt: hook prompts are only supported on unix");
    PromptCliOutcome { code: 3, stdout: None }
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// The hooks root dir for a scope: `<global_dir>` for global, `<worktree>/.spwn/hooks`
/// for repo. None for global when the home dir couldn't be resolved.
fn scope_base(global_dir: Option<&Path>, worktree: &Path, scope: Scope) -> Option<PathBuf> {
    match scope {
        Scope::Global => global_dir.map(Path::to_path_buf),
        Scope::Repo => Some(worktree.join(".spwn").join("hooks")),
    }
}

/// The bare single-file hook for an event under a scope root: `<base>/<event>.sh`.
fn bare_hook_file(base: &Path, event: &str) -> PathBuf {
    base.join(format!("{event}.sh"))
}

/// The directory of per-event scripts under a scope root: `<base>/<event>.d`.
fn hook_dir(base: &Path, event: &str) -> PathBuf {
    base.join(format!("{event}.d"))
}

/// Whether `p` is a regular file with an execute bit (on unix; any regular file
/// elsewhere).
fn is_executable(p: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(p) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Whether a file inside an `<event>.d` directory should be run: a non-hidden regular
/// file that is either executable or ends in `.sh` (so a stray `README`/`notes.txt`
/// is ignored, but `20-setup.py` with a shebang still runs).
fn is_runnable_hook(p: &Path) -> bool {
    let Some(name) = p.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    if name.starts_with('.') || !p.is_file() {
        return false;
    }
    name.ends_with(".sh") || is_executable(p)
}

/// The hook scripts for one scope+event, in run order: a bare `<event>.sh` first (if
/// present), then every runnable file in `<event>.d/` sorted by filename (so numeric
/// prefixes like `10-`, `20-` order them). Empty means "no hook for this scope" — the
/// feature is fully opt-in.
pub fn discover_scope(
    global_dir: Option<&Path>,
    worktree: &Path,
    event: &str,
    scope: Scope,
) -> Vec<PathBuf> {
    let Some(base) = scope_base(global_dir, worktree, scope) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let bare = bare_hook_file(&base, event);
    if bare.is_file() {
        out.push(bare);
    }
    if let Ok(rd) = std::fs::read_dir(hook_dir(&base, event)) {
        let mut entries: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| is_runnable_hook(p))
            .collect();
        entries.sort();
        out.extend(entries);
    }
    out
}

/// All discovered scripts for an event, ordered global-first then repo, each scope's
/// scripts in run order — the layering contract (global defaults run first; repo hooks
/// add to / override them).
pub fn discover_all(
    global_dir: Option<&Path>,
    worktree: &Path,
    event: &str,
) -> Vec<(Scope, PathBuf)> {
    let mut out = Vec::new();
    for scope in [Scope::Global, Scope::Repo] {
        for p in discover_scope(global_dir, worktree, event, scope) {
            out.push((scope, p));
        }
    }
    out
}

/// The working directory a hook runs in. Everything runs in the session worktree,
/// EXCEPT the global `session-created` / `session-deleted` scripts, which run in the
/// project dir — the former creates the worktree (it doesn't exist yet), the latter
/// removes it.
fn run_dir_for<'a>(ctx: &'a HookCtx, event: &str, scope: Scope) -> &'a Path {
    match (event, scope) {
        ("session-created", Scope::Global) | ("session-deleted", Scope::Global) => {
            Path::new(&ctx.project_dir)
        }
        _ => ctx.worktree.as_path(),
    }
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Keep at most `cap` bytes from the end of `s`, on a char boundary, prefixing `…`
/// when truncated.
fn tail(s: &str, cap: usize) -> String {
    let t = s.trim_end();
    if t.len() <= cap {
        return t.to_string();
    }
    let mut start = t.len() - cap;
    while start < t.len() && !t.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &t[start..])
}

/// Run the event's hook script, capturing its outcome. Executes the file directly
/// when it's executable (honoring its shebang), otherwise via `sh`. Never panics.
///
/// Streams the script's output **live**: each stdout/stderr line is handed to
/// `on_line` as it's produced (so the UI can show progress while a hook runs), and
/// the same lines are accumulated into the returned [`HookRun`]'s captured tail.
///
/// The hook can raise a blocking multiple-choice prompt by running the `spwn prompt`
/// helper, which connects to a per-run unix socket. Each request is delivered to
/// `on_prompt` on this thread, which **blocks** until it returns the user's answer (the
/// caller enforces a timeout / headless fallback so the runner can't hang); the answer
/// is written back over the socket. The hook's own stdin is `/dev/null`, so there's no
/// bare-`read` hazard, and prompts never appear in the captured output.
fn run_one(
    ctx: &HookCtx,
    event: &str,
    scope: Scope,
    script: &Path,
    run_dir: &Path,
    on_line: &mut dyn FnMut(&str),
    on_prompt: &mut dyn FnMut(HookPromptRequest) -> String,
) -> HookRun {
    let name = script
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut cmd = if is_executable(script) {
        Command::new(script)
    } else {
        let mut c = Command::new("sh");
        c.arg(script);
        c
    };
    cmd.current_dir(run_dir)
        .env("SPWN_EVENT", event)
        .env("SPWN_TERMINAL_ID", &ctx.terminal_id)
        .env("SPWN_PROJECT_DIR", &ctx.project_dir)
        .env("SPWN_WORKTREE", ctx.worktree.to_string_lossy().as_ref())
        // No stdin: a hook prompts via the `spwn prompt` helper (a socket), never by
        // reading stdin — so a stray `read` gets EOF instead of hanging.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(b) = &ctx.branch {
        cmd.env("SPWN_BRANCH", b);
    }
    if let Some(b) = &ctx.base_branch {
        cmd.env("SPWN_BASE_BRANCH", b);
    }
    if let Some(s) = &ctx.session_id {
        cmd.env("SPWN_SESSION_ID", s);
    }
    if let Some(u) = &ctx.turn_uuid {
        cmd.env("SPWN_TURN_UUID", u);
    }
    if let Some(x) = &ctx.exec {
        cmd.env("SPWN_EXEC", x);
    }
    // So `"$SPWN_BIN" prompt …` works even when spwn isn't on PATH.
    if let Ok(exe) = std::env::current_exe() {
        cmd.env(PROMPT_BIN_ENV, exe);
    }

    // Per-run prompt socket (unix only). The hook's `spwn prompt` helper connects here.
    // Bind failure is non-fatal: the hook still runs; a prompt just errors client-side.
    #[cfg(unix)]
    let sock_path = {
        let seq = SOCK_SEQ.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("spwn-hook-{}-{}.sock", std::process::id(), seq))
    };
    #[cfg(unix)]
    let listener = {
        let _ = std::fs::remove_file(&sock_path);
        match UnixListener::bind(&sock_path) {
            Ok(l) => {
                cmd.env(PROMPT_SOCK_ENV, &sock_path);
                Some(l)
            }
            Err(_) => None,
        }
    };

    let failed = |msg: String| HookRun {
        event: event.to_string(),
        scope,
        script: name.clone(),
        exit_code: None,
        ok: false,
        output: msg,
        at: now_secs(),
        reported: BTreeMap::new(),
    };

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            #[cfg(unix)]
            let _ = std::fs::remove_file(&sock_path);
            return failed(format!("failed to run hook: {e}"));
        }
    };

    // Unified message channel: output lines (from the two pipe readers) and prompt
    // requests (from the socket thread) both land here and are handled in arrival order
    // on this thread — which keeps `on_line`/`on_prompt` free of `Send` bounds. Draining
    // stdout/stderr on their own threads means a chatty hook can't deadlock on a full
    // pipe buffer, even while we block on a prompt.
    let (tx, rx) = mpsc::channel::<HookMsg>();
    let mut readers = Vec::new();
    for pipe in [
        child.stdout.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
        child.stderr.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let tx = tx.clone();
        readers.push(thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                if tx.send(HookMsg::Line(line)).is_err() {
                    break;
                }
            }
        }));
    }

    // Flipped once the child exits, so the socket thread stops polling `accept` and drops
    // its sender (which, with the readers done, closes `rx`).
    let stop = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    let sock_thread = listener.map(|l| {
        let stop = stop.clone();
        let tx = tx.clone();
        thread::spawn(move || serve_prompt_socket(l, stop, tx))
    });

    // Waiter thread owns the child: it reports the exit status and trips `stop`. (The
    // reader threads end on their own when the pipes close at child exit.)
    let (exit_tx, exit_rx) = mpsc::channel::<(Option<i32>, bool)>();
    let waiter = {
        let stop = stop.clone();
        thread::spawn(move || {
            let status = match child.wait() {
                Ok(s) => (s.code(), s.success()),
                Err(_) => (None, false),
            };
            let _ = exit_tx.send(status);
            stop.store(true, Ordering::SeqCst);
        })
    };

    drop(tx); // now only the reader/socket threads hold senders; `rx` ends when they do

    let mut combined = String::new();
    let mut reported: BTreeMap<String, String> = BTreeMap::new();
    for msg in rx {
        match msg {
            HookMsg::Line(line) => {
                // A `::spwn:set:: key=value` line is a callback to spwn, not output:
                // record it and keep it out of the streamed/captured text.
                if let Some(rest) = line.trim_start().strip_prefix(SET_SENTINEL) {
                    if let Some((k, v)) = rest.trim().split_once('=') {
                        let k = k.trim();
                        if !k.is_empty() {
                            reported.insert(k.to_string(), v.trim().to_string());
                        }
                    }
                    continue;
                }
                on_line(&line);
                combined.push_str(&line);
                combined.push('\n');
            }
            // A prompt blocks here until the user (or the timeout) answers; the reply
            // goes back to the socket thread, which writes it to the waiting helper.
            #[cfg(unix)]
            HookMsg::Prompt(req, reply) => {
                let _ = reply.send(on_prompt(req));
            }
        }
    }

    for r in readers {
        let _ = r.join();
    }
    #[cfg(unix)]
    if let Some(t) = sock_thread {
        let _ = t.join();
    }
    let _ = waiter.join();
    #[cfg(unix)]
    let _ = std::fs::remove_file(&sock_path);

    let (exit_code, ok) = exit_rx.recv().unwrap_or((None, false));
    HookRun {
        event: event.to_string(),
        scope,
        script: name,
        exit_code,
        ok,
        output: tail(&combined, OUTPUT_CAP),
        at: now_secs(),
        reported,
    }
}

/// Run a set of already-discovered `(scope, script)` entries for an event in order,
/// each in its proper working directory, returning one [`HookRun`] per entry. Output
/// lines stream to `on_line` live.
pub fn run_entries(
    ctx: &HookCtx,
    event: &str,
    entries: &[(Scope, PathBuf)],
    on_line: &mut dyn FnMut(&str),
    on_prompt: &mut dyn FnMut(HookPromptRequest) -> String,
) -> Vec<HookRun> {
    entries
        .iter()
        .map(|(scope, script)| {
            let run_dir = run_dir_for(ctx, event, *scope);
            run_one(ctx, event, *scope, script, run_dir, on_line, on_prompt)
        })
        .collect()
}

/// Discover + run every scope's hook for an event (global first, then repo). Returns a
/// run per script that existed (empty when none). Each output line streams live.
/// (Commands fire hooks per-scope via `run_entries`; this all-scopes convenience is
/// used by the tests and kept as part of the module's public API.)
#[allow(dead_code)]
pub fn run_event_sync(
    ctx: &HookCtx,
    event: &str,
    global_dir: Option<&Path>,
    on_line: &mut dyn FnMut(&str),
    on_prompt: &mut dyn FnMut(HookPromptRequest) -> String,
) -> Vec<HookRun> {
    let entries = discover_all(global_dir, &ctx.worktree, event);
    run_entries(ctx, event, &entries, on_line, on_prompt)
}

// ---------------------------------------------------------------------------
// Status (for the Hooks panel)
// ---------------------------------------------------------------------------

/// The discovered scripts (per scope) + the caller's recorded last run for each event.
pub fn status(
    global_dir: Option<&Path>,
    worktree: &Path,
    last_runs: &BTreeMap<String, Vec<HookRun>>,
) -> HooksStatus {
    let events = EVENTS
        .iter()
        .map(|&event| {
            let runs = last_runs.get(event);
            let scripts = discover_all(global_dir, worktree, event)
                .into_iter()
                .map(|(scope, p)| {
                    let script = p
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let last_run = runs.and_then(|rs| {
                        rs.iter()
                            .find(|r| r.scope == scope && r.script == script)
                            .cloned()
                    });
                    HookScriptInfo { scope, script, last_run }
                })
                .collect();
            HookEventInfo { event: event.to_string(), scripts }
        })
        .collect();
    HooksStatus { available: true, events, running: None }
}

// ---------------------------------------------------------------------------
// Default global hooks (spwn's built-in per-session behaviors, as editable scripts)
// ---------------------------------------------------------------------------

/// spwn's built-in per-session behaviors ship as default *global* hook scripts under
/// per-event `<event>.d/` directories, so users can drop their own `NN-*.sh` alongside
/// (composing, not editing) while spwn keeps ownership of these numbered files and can
/// update them on version bumps. Each is `(subdir, filename, body)`.
const DEFAULT_HOOKS: &[(&str, &str, &str)] = &[
    (
        "session-created.d",
        "10-worktree.sh",
        include_str!("../assets/hooks/session-created.d/10-worktree.sh"),
    ),
    (
        "session-deleted.d",
        "90-worktree.sh",
        include_str!("../assets/hooks/session-deleted.d/90-worktree.sh"),
    ),
    (
        "session-turn.d",
        "10-commit.sh",
        include_str!("../assets/hooks/session-turn.d/10-commit.sh"),
    ),
    (
        "session-turn.d",
        "20-checkpoint.sh",
        include_str!("../assets/hooks/session-turn.d/20-checkpoint.sh"),
    ),
];

fn write_default_hook(path: &Path, body: &str) {
    if std::fs::write(path, body).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
        }
    }
}

/// Install spwn's default global hook scripts into `~/.spwn/hooks/<event>.d/`. Behavior
/// is keyed off a `.spwn-version` marker so spwn owns these numbered files without
/// clobbering user-added ones or resurrecting intentionally-deleted ones:
///   - Fresh install (no marker): create every default.
///   - Version change: refresh only the defaults that still exist on disk (update
///     spwn's own scripts in place), leaving user-deleted ones deleted.
///   - Same version: nothing to do.
/// Users add their own `NN-*.sh` files in the same dirs; those are never touched.
/// Best-effort — any IO failure is ignored. Called once on startup.
pub fn install_default_global_hooks() {
    if let Some(dir) = global_hooks_dir() {
        install_defaults_into(&dir, env!("CARGO_PKG_VERSION"));
    }
}

/// Core of [`install_default_global_hooks`], parameterized on the hooks dir + version
/// so it's testable without touching the real `~/.spwn/hooks`.
fn install_defaults_into(dir: &Path, current: &str) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let marker = dir.join(".spwn-version");
    let prev = std::fs::read_to_string(&marker)
        .ok()
        .map(|s| s.trim().to_string());
    // refresh_existing: on a version change, overwrite spwn's own files that are still
    // present (update them) but don't recreate ones the user deleted. On a fresh install
    // (no marker), create everything.
    let refresh_existing = match prev.as_deref() {
        Some(v) if v == current => return, // already installed for this version
        Some(_) => true,                   // version change
        None => false,                     // fresh install
    };
    for (subdir, name, body) in DEFAULT_HOOKS {
        let d = dir.join(subdir);
        if std::fs::create_dir_all(&d).is_err() {
            continue;
        }
        let path = d.join(name);
        let exists = path.exists();
        // Fresh: write if missing. Version change: write only if it still exists.
        if (refresh_existing && exists) || (!refresh_existing && !exists) {
            write_default_hook(&path, body);
        }
    }
    let _ = std::fs::write(&marker, current);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, body: &str, exec: bool) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
        #[cfg(unix)]
        if exec {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let _ = exec;
    }

    // The bare single-file hook paths, for tests.
    fn hook_file(wt: &Path, event: &str) -> PathBuf {
        wt.join(".spwn").join("hooks").join(format!("{event}.sh"))
    }
    fn global_hook_file(dir: &Path, event: &str) -> PathBuf {
        dir.join(format!("{event}.sh"))
    }

    fn ctx(wt: &Path) -> HookCtx {
        HookCtx {
            terminal_id: "abc123".into(),
            project_dir: wt.to_string_lossy().into_owned(),
            worktree: wt.to_path_buf(),
            branch: Some("spwn/abc123".into()),
            base_branch: None,
            session_id: None,
            turn_uuid: None,
            exec: None,
        }
    }

    #[test]
    fn runs_single_event_file_with_env() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path();
        write(
            &hook_file(wt, "session-created"),
            "#!/bin/sh\necho \"id=$SPWN_TERMINAL_ID event=$SPWN_EVENT branch=$SPWN_BRANCH\"\n",
            true,
        );
        let runs =
            run_event_sync(&ctx(wt), "session-created", None, &mut |_| {}, &mut |_| String::new());
        assert_eq!(runs.len(), 1, "one repo-scope hook present");
        let run = &runs[0];
        assert_eq!(run.scope, Scope::Repo);
        assert_eq!(run.script, "session-created.sh");
        assert!(run.ok);
        assert!(run.output.contains("id=abc123"));
        assert!(run.output.contains("event=session-created"));
        assert!(run.output.contains("branch=spwn/abc123"));
    }

    #[test]
    fn missing_hook_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(discover_scope(None, dir.path(), "session-deleted", Scope::Repo).is_empty());
        assert!(run_event_sync(&ctx(dir.path()), "session-deleted", None, &mut |_| {}, &mut |_| String::new()).is_empty());
    }

    #[test]
    fn install_defaults_fresh_upgrade_and_respects_deletion() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let worktree_hook = root.join("session-created.d").join("10-worktree.sh");
        let checkpoint_hook = root.join("session-turn.d").join("20-checkpoint.sh");

        // Fresh install: all defaults created, discoverable, and a user file is untouched.
        install_defaults_into(root, "1.0.0");
        assert!(worktree_hook.is_file());
        assert!(checkpoint_hook.is_file());
        let user_hook = root.join("session-created.d").join("50-user.sh");
        write(&user_hook, "#!/bin/sh\necho mine\n", true);

        // The user deletes one spwn default and re-runs the SAME version: not resurrected.
        std::fs::remove_file(&checkpoint_hook).unwrap();
        install_defaults_into(root, "1.0.0");
        assert!(!checkpoint_hook.exists(), "same version must not recreate a deleted default");

        // Version bump: refreshes defaults that still exist (worktree) but does NOT
        // resurrect the deleted one, and never touches the user's file.
        std::fs::write(&worktree_hook, "# edited by spwn-owner test\n").unwrap();
        install_defaults_into(root, "2.0.0");
        assert!(worktree_hook.is_file());
        assert!(
            std::fs::read_to_string(&worktree_hook).unwrap().contains("git worktree add"),
            "upgrade should refresh spwn's own file in place"
        );
        assert!(!checkpoint_hook.exists(), "upgrade must not resurrect a deleted default");
        assert!(user_hook.is_file(), "user-added scripts are never touched");
    }

    #[test]
    fn discovers_bare_then_sorted_dir_scripts() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path();
        let base = wt.join(".spwn").join("hooks");
        // A bare file plus a `.d` dir with two ordered scripts and one ignored non-hook.
        write(&base.join("session-turn.sh"), "#!/bin/sh\necho bare\n", true);
        write(&base.join("session-turn.d").join("20-b.sh"), "#!/bin/sh\necho b\n", true);
        write(&base.join("session-turn.d").join("10-a.sh"), "#!/bin/sh\necho a\n", true);
        write(&base.join("session-turn.d").join("notes.txt"), "not a hook\n", false);
        let found = discover_scope(None, wt, "session-turn", Scope::Repo);
        let names: Vec<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        // Bare file first, then dir scripts sorted by name; notes.txt excluded.
        assert_eq!(names, vec!["session-turn.sh", "10-a.sh", "20-b.sh"]);
    }

    #[test]
    fn global_runs_before_repo() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path().join("wt");
        let global = dir.path().join("global");
        std::fs::create_dir_all(&wt).unwrap();
        // Both scopes define session-ready (which runs in the worktree for both).
        write(&global_hook_file(&global, "session-ready"), "#!/bin/sh\necho G\n", true);
        write(&hook_file(&wt, "session-ready"), "#!/bin/sh\necho R\n", true);
        let mut c = ctx(&wt);
        c.project_dir = wt.to_string_lossy().into_owned();
        let runs = run_event_sync(&c, "session-ready", Some(global.as_path()), &mut |_| {}, &mut |_| String::new());
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].scope, Scope::Global);
        assert_eq!(runs[1].scope, Scope::Repo);
    }

    #[test]
    fn reports_sentinel_values_out_of_band() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path();
        write(
            &hook_file(wt, "session-created"),
            "#!/bin/sh\necho hello\necho '::spwn:set:: worktree=/tmp/some path/wt'\necho '::spwn:set:: branch=spwn/x'\n",
            true,
        );
        let runs = run_event_sync(&ctx(wt), "session-created", None, &mut |_| {}, &mut |_| String::new());
        let run = &runs[0];
        assert!(run.ok);
        // Sentinel lines are parsed, not echoed into the captured output.
        assert!(run.output.contains("hello"));
        assert!(!run.output.contains("::spwn:set::"));
        assert_eq!(run.reported.get("worktree").map(String::as_str), Some("/tmp/some path/wt"));
        assert_eq!(run.reported.get("branch").map(String::as_str), Some("spwn/x"));
    }

    #[test]
    fn non_executable_file_runs_via_sh() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path();
        // No execute bit — spwn falls back to `sh <file>`.
        write(&hook_file(wt, "session-ready"), "echo hi\n", false);
        let runs = run_event_sync(&ctx(wt), "session-ready", None, &mut |_| {}, &mut |_| String::new());
        let run = &runs[0];
        assert!(run.ok, "output: {}", run.output);
        assert!(run.output.contains("hi"));
    }

    #[test]
    fn streams_each_output_line_live() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path();
        write(
            &hook_file(wt, "session-created"),
            "#!/bin/sh\necho one\necho two >&2\necho three\n",
            true,
        );
        let mut lines: Vec<String> = Vec::new();
        let runs = run_event_sync(
            &ctx(wt),
            "session-created",
            None,
            &mut |l| lines.push(l.to_string()),
            &mut |_| String::new(),
        );
        let run = &runs[0];
        assert!(run.ok);
        // Every line reached the streamer (order across stdout/stderr is not asserted).
        assert!(lines.iter().any(|l| l == "one"));
        assert!(lines.iter().any(|l| l == "two"));
        assert!(lines.iter().any(|l| l == "three"));
        // …and the captured tail still holds them.
        assert!(run.output.contains("one") && run.output.contains("three"));
    }

    #[cfg(unix)]
    #[test]
    fn parse_prompt_args_builds_request() {
        // No options → a Yes/No confirm.
        let r = parse_prompt_args(&["Seed?".to_string()]).unwrap();
        assert_eq!(r.question, "Seed?");
        assert_eq!(r.options.len(), 2);
        assert!(!r.multi_select);
        // Flags + explicit options.
        let r = parse_prompt_args(&[
            "--multi".to_string(),
            "--header".to_string(),
            "setup".to_string(),
            "Pick a color".to_string(),
            "Red".to_string(),
            "Blue".to_string(),
        ])
        .unwrap();
        assert!(r.multi_select);
        assert_eq!(r.header.as_deref(), Some("setup"));
        assert_eq!(
            r.options.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(),
            vec!["Red", "Blue"]
        );
        // No question → usage error.
        assert!(parse_prompt_args(&[]).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn prompt_socket_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("p.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel::<HookMsg>();
        let server = {
            let stop = stop.clone();
            thread::spawn(move || serve_prompt_socket(listener, stop, tx))
        };

        // Client: connect, send a request, read the reply.
        let client = thread::spawn(move || {
            let mut s = UnixStream::connect(&sock).unwrap();
            writeln!(s, "{{\"question\":\"Seed?\",\"options\":[{{\"label\":\"Yes\"}}]}}").unwrap();
            let mut resp = String::new();
            BufReader::new(&s).read_line(&mut resp).unwrap();
            resp
        });

        // Runner side: receive the prompt and answer "Yes".
        match rx.recv().unwrap() {
            HookMsg::Prompt(req, reply) => {
                assert_eq!(req.question, "Seed?");
                reply.send("Yes".to_string()).unwrap();
            }
            _ => panic!("expected a prompt"),
        }

        let resp = client.join().unwrap();
        assert!(resp.contains("\"answer\":\"Yes\""), "resp: {resp}");

        stop.store(true, Ordering::SeqCst);
        let _ = server.join();
    }

    #[cfg(unix)]
    #[test]
    fn prompt_cli_round_trip() {
        // A one-shot fake spwn: accept one connection, read the request, send `reply`.
        fn responder(sock: PathBuf, reply: &'static str) -> thread::JoinHandle<()> {
            let listener = UnixListener::bind(&sock).unwrap();
            thread::spawn(move || {
                if let Ok((mut stream, _)) = listener.accept() {
                    let rc = stream.try_clone().unwrap();
                    let mut line = String::new();
                    let _ = BufReader::new(rc).read_line(&mut line);
                    let _ = writeln!(stream, "{reply}");
                }
            })
        }

        let dir = tempfile::tempdir().unwrap();

        // Answered → label on stdout, exit 0.
        let sock = dir.path().join("a.sock");
        let h = responder(sock.clone(), "{\"answer\":\"Blue\"}");
        std::env::set_var(PROMPT_SOCK_ENV, &sock);
        let out = run_prompt_cli(&["Pick".to_string(), "Red".to_string(), "Blue".to_string()]);
        let _ = h.join();
        assert_eq!(out.code, 0);
        assert_eq!(out.stdout.as_deref(), Some("Blue"));

        // Declined → exit 2, no stdout.
        let sock = dir.path().join("b.sock");
        let h = responder(sock.clone(), "{\"declined\":true}");
        std::env::set_var(PROMPT_SOCK_ENV, &sock);
        let out = run_prompt_cli(&["Pick".to_string()]);
        let _ = h.join();
        assert_eq!(out.code, 2);
        assert!(out.stdout.is_none());

        // Not inside a hook → usage/connection error, exit 3.
        std::env::remove_var(PROMPT_SOCK_ENV);
        assert_eq!(run_prompt_cli(&["Pick".to_string()]).code, 3);
    }

    #[test]
    fn nonzero_exit_is_recorded_as_failure() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path();
        write(&hook_file(wt, "session-deleted"), "#!/bin/sh\nexit 3\n", true);
        let runs = run_event_sync(&ctx(wt), "session-deleted", None, &mut |_| {}, &mut |_| String::new());
        let run = &runs[0];
        assert!(!run.ok);
        assert_eq!(run.exit_code, Some(3));
    }
}
