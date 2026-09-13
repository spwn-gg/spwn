//! The embedded JavaScript runtime a workflow runs in.
//!
//! One QuickJS runtime per run attempt, on the run's own thread. The script reaches
//! spwn through two host functions — `__host(op, json)` (async) and `__hostSync(op,
//! json)` — which the prelude (`prelude.js`) wraps into the `spwn` API. Keeping the
//! boundary to JSON strings keeps the API itself in one readable JS file.

use super::host::{self, RunCtx};
use super::Meta;
use rquickjs::function::Async;
use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::promise::MaybePromise;
use rquickjs::{
    AsyncContext, AsyncRuntime, CatchResultExt, Context, Ctx, Error as JsError, Exception,
    Function, Module, Object, Runtime, Value,
};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const PRELUDE: &str = include_str!("prelude.js");

/// Just enough for a module's top level to evaluate while its `meta` is read: logging
/// and timers are inert, and there is no `spwn` API.
const META_PRELUDE: &str = r#"
globalThis.console = { log() {}, info() {}, debug() {}, warn() {}, error() {} };
globalThis.setTimeout = globalThis.setInterval = () => 0;
globalThis.clearTimeout = globalThis.clearInterval = () => {};
"#;

/// How long a module's top level may run while its meta is read.
const META_TIMEOUT: Duration = Duration::from_secs(2);

/// How long a finished attempt's leftover promises get to settle before it's torn down.
const WIND_DOWN: Duration = Duration::from_secs(3);

// ---------------------------------------------------------------------------
// Modules: relative imports inside `.spwn/workflows`, `.ts` stripped of types
// ---------------------------------------------------------------------------

/// Lexically normalize a path (`a/./b/../c` → `a/c`) without touching the disk.
pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn with_extension(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    let s = path.to_string_lossy();
    super::EXTENSIONS
        .iter()
        .map(|ext| PathBuf::from(format!("{s}.{ext}")))
        .chain(super::EXTENSIONS.iter().map(|ext| path.join(format!("index.{ext}"))))
        .find(|p| p.is_file())
}

/// Resolve an import to a module file inside `root`. Only relative specifiers (and the
/// absolute path of the entry itself) are allowed: there is no package resolution, and
/// a workflow can't import files from outside its directory.
pub(crate) fn resolve_module(root: &Path, base: &str, name: &str) -> Result<String, String> {
    let candidate = if Path::new(name).is_absolute() {
        PathBuf::from(name)
    } else if name.starts_with("./") || name.starts_with("../") {
        Path::new(base).parent().unwrap_or(root).join(name)
    } else {
        return Err(format!(
            "only relative imports (\"./…\") inside .spwn/workflows are supported, not \"{name}\""
        ));
    };
    let wanted = normalize(&candidate);
    let file = with_extension(&wanted)
        .ok_or_else(|| format!("no such module: {}", wanted.display()))?;
    let file = std::fs::canonicalize(&file).unwrap_or(file);
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    if !file.starts_with(&root) {
        return Err(format!("{} is outside .spwn/workflows", file.display()));
    }
    Ok(file.to_string_lossy().into_owned())
}

fn load_source(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    match path.extension().and_then(|e| e.to_str()) {
        Some("ts" | "mts") => super::ts::strip_types(&text, path),
        _ => Ok(text),
    }
}

struct WorkflowResolver {
    root: PathBuf,
}

impl Resolver for WorkflowResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        resolve_module(&self.root, base, name)
            .map_err(|msg| JsError::new_resolving_message(base, name, msg))
    }
}

struct WorkflowLoader;

impl Loader for WorkflowLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js>> {
        let source =
            load_source(Path::new(name)).map_err(|msg| JsError::new_loading_message(name, msg))?;
        Module::declare(ctx.clone(), name, source)
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A JS error as text: the message, then the stack when there is one.
fn describe<'js>(ctx: &Ctx<'js>, err: JsError) -> String {
    match Err::<(), _>(err).catch(ctx) {
        Err(rquickjs::CaughtError::Exception(e)) => exception_text(&e),
        Err(other) => other.to_string().trim_end().to_string(),
        Ok(()) => String::new(),
    }
}

fn exception_text(e: &Exception<'_>) -> String {
    let message = e.message().unwrap_or_else(|| "error".to_string());
    match e.stack().map(|s| s.trim_end().to_string()).filter(|s| !s.is_empty()) {
        Some(stack) => format!("{message}\n{stack}"),
        None => message,
    }
}

fn value_text(v: &Value<'_>) -> String {
    if let Some(e) = v.as_object().and_then(|o| Exception::from_object(o.clone())) {
        return exception_text(&e);
    }
    if let Some(s) = v.as_string().and_then(|s| s.to_string().ok()) {
        return s;
    }
    v.ctx()
        .json_stringify(v.clone())
        .ok()
        .flatten()
        .and_then(|s| s.to_string().ok())
        .unwrap_or_else(|| format!("{v:?}"))
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

/// Run one attempt of a workflow to completion: `Ok` when `main` returned, `Err` with
/// the error text when it (or loading it) threw.
pub(crate) async fn run_script(rc: Arc<RunCtx>, entry: &Path, root: &Path) -> Result<(), String> {
    let rt = AsyncRuntime::new().map_err(|e| e.to_string())?;
    rt.set_loader(WorkflowResolver { root: root.to_path_buf() }, WorkflowLoader)
        .await;
    // Stopping a run must also stop JS that never yields (`while (true) {}`); host calls
    // are cancelled separately, in `host`.
    let flags = rc.stop_flags();
    rt.set_interrupt_handler(Some(Box::new(move || flags.any_set())))
        .await;
    let tracker_rc = rc.clone();
    rt.set_host_promise_rejection_tracker(Some(Box::new(
        move |_ctx: Ctx<'_>, _promise: Value<'_>, reason: Value<'_>, handled: bool| {
            if !handled && !tracker_rc.is_stopping() {
                tracker_rc.log("error", format!("unhandled promise rejection: {}", value_text(&reason)));
            }
        },
    )))
    .await;
    let ctx = AsyncContext::full(&rt).await.map_err(|e| e.to_string())?;

    let entry = entry.to_string_lossy().into_owned();
    let run_rc = rc.clone();
    let outcome = ctx
        .async_with(async move |ctx| drive(ctx, run_rc, entry).await)
        .await;

    // Let what's left settle: host calls still pending resolve as "stopped" once the
    // attempt is marked done, which ends loops like the event pump.
    rc.end_attempt();
    if tokio::time::timeout(WIND_DOWN, rt.idle()).await.is_err() {
        // Something refuses to settle. Freeing a QuickJS runtime that still has live
        // objects aborts the process, so leak this attempt's runtime instead.
        rc.log("warn", "the workflow didn't settle after finishing; abandoning its runtime");
        std::mem::forget(ctx);
        std::mem::forget(rt);
    }
    outcome
}

async fn drive<'js>(ctx: Ctx<'js>, rc: Arc<RunCtx>, entry: String) -> Result<(), String> {
    install_host(&ctx, rc.clone()).map_err(|e| describe(&ctx, e))?;
    ctx.eval::<(), _>(PRELUDE)
        .map_err(|e| format!("spwn prelude failed: {}", describe(&ctx, e)))?;

    let promise = Module::import(&ctx, entry.as_str()).map_err(|e| describe(&ctx, e))?;
    let namespace: Object = promise
        .into_future::<Object>()
        .await
        .map_err(|e| describe(&ctx, e))?;
    let main = namespace
        .get::<_, Value>("default")
        .ok()
        .and_then(|v| v.as_function().cloned())
        .ok_or_else(|| "the workflow has no `export default` function".to_string())?;

    let take: Function = ctx
        .globals()
        .get("__takeSpwnApi")
        .map_err(|e| describe(&ctx, e))?;
    let api: Value = take.call(()).map_err(|e| describe(&ctx, e))?;
    let inputs = ctx
        .json_parse(rc.inputs.to_string())
        .map_err(|e| describe(&ctx, e))?;

    let returned: Value = main.call((api, inputs)).map_err(|e| describe(&ctx, e))?;
    MaybePromise::from_value(returned)
        .into_future::<Value>()
        .await
        .map_err(|e| describe(&ctx, e))?;
    Ok(())
}

fn install_host<'js>(ctx: &Ctx<'js>, rc: Arc<RunCtx>) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    let sync_rc = rc.clone();
    globals.set(
        "__hostSync",
        Function::new(ctx.clone(), move |op: String, args: String| -> String {
            host::dispatch_sync(&sync_rc, &op, &args)
        })?,
    )?;
    globals.set(
        "__host",
        Function::new(
            ctx.clone(),
            Async(move |op: String, args: String| {
                let rc = rc.clone();
                async move { host::dispatch(rc, op, args).await }
            }),
        )?,
    )?;
    Ok(())
}

/// Read a workflow's `export const meta` by evaluating the module with no `spwn` API.
/// Also checks there is a default-exported function, so a broken workflow shows its
/// error in the list rather than only when run.
pub(crate) fn read_meta(entry: &Path, root: &Path) -> Result<Meta, String> {
    let rt = Runtime::new().map_err(|e| e.to_string())?;
    rt.set_loader(WorkflowResolver { root: root.to_path_buf() }, WorkflowLoader);
    let deadline = Instant::now() + META_TIMEOUT;
    rt.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
    let ctx = Context::full(&rt).map_err(|e| e.to_string())?;
    let entry = entry.to_string_lossy().into_owned();
    ctx.with(|ctx| {
        ctx.eval::<(), _>(META_PRELUDE)
            .map_err(|e| describe(&ctx, e))?;
        let namespace: Object = Module::import(&ctx, entry.as_str())
            .and_then(|p| p.finish::<Object>())
            .map_err(|e| match e {
                JsError::WouldBlock => {
                    "the workflow's top level awaits something; do that inside main".to_string()
                }
                e => describe(&ctx, e),
            })?;
        let has_main = namespace
            .get::<_, Value>("default")
            .is_ok_and(|v| v.is_function());
        if !has_main {
            return Err("the workflow has no `export default` function".to_string());
        }
        let meta: Value = namespace.get("meta").map_err(|e| describe(&ctx, e))?;
        if meta.is_undefined() || meta.is_null() {
            return Ok(Meta::default());
        }
        let json = ctx
            .json_stringify(meta)
            .map_err(|e| describe(&ctx, e))?
            .and_then(|s| s.to_string().ok())
            .unwrap_or_else(|| "{}".to_string());
        serde_json::from_str::<Meta>(&json).map_err(|e| format!("invalid `meta`: {e}"))
    })
}
