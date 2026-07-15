//! Per-session docker-compose integration (opt-in, thin wrapper over `docker
//! compose`). Each Claude session works in its own git worktree; if that worktree
//! has a committed `spwn.yaml`, spwn can bring up the project's service + test
//! harness in an isolated compose stack for that session, so N sessions run
//! concurrently without collision.
//!
//! Design (mirrors `gitwt.rs`'s shell-out style):
//!   * Namespacing — every stack uses `COMPOSE_PROJECT_NAME=spwn-<short>` so
//!     containers/networks/volumes never collide across sessions.
//!   * Ephemeral ports — the user publishes container-only ports; Docker assigns a
//!     free host port; we surface it as a live URL via `docker compose port`.
//!   * Shared services (`scope: shared`) — heavy stateful services (DB/cache/queue)
//!     run ONCE under `spwn-shared-<repo>`, ref-counted so one deleted session never
//!     kills a DB others still use.
//!   * Image reuse — session build images are content-tagged by a hash of their
//!     DEPENDENCY inputs (Dockerfile + lockfiles), so a freshly-forked child reuses
//!     the parent's already-built image with no rebuild until deps diverge.
//!   * A generated per-session override (`.spwn/override.gen.yml`) carries all
//!     injections (pinned image tags, resource caps, shared-network wiring,
//!     per-session volumes) so the user's `docker-compose.yml` is never mutated.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// spwn.yaml config model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    #[default]
    Session,
    Shared,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ServiceCfg {
    #[serde(default)]
    pub scope: Scope,
    /// "service" (long-running; surfaces URLs) | "harness" (test watcher).
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub wait_for_healthy: bool,
    /// Container ports to publish ephemerally and surface as live URLs.
    #[serde(default)]
    pub ports: Vec<u16>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Lifecycle {
    /// "on-demand" (default) | "on-session-start" | "manual".
    #[serde(default = "default_up")]
    pub up: String,
    /// Idle threshold like "15m"/"30s" after which the stack is `stop`ped.
    #[serde(default)]
    pub idle_stop: Option<String>,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self { up: default_up(), idle_stop: None }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CacheMount {
    pub path: String,
    pub volume: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Volumes {
    /// "host" (default; rely on the worktree's COW-cloned copy) | "per-session"
    /// (mask with a per-session named volume + install inside the container).
    #[serde(default)]
    pub node_modules: Option<String>,
    /// Container path to mask when `node_modules: per-session` (default /app/node_modules).
    #[serde(default)]
    pub node_modules_path: Option<String>,
    /// Content-addressed caches shared (external volumes) across all sessions.
    #[serde(default)]
    pub caches: Vec<CacheMount>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Resources {
    pub memory: Option<String>,
    pub cpus: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ImageCfg {
    /// "lineage" to reuse the parent's content-tagged image (the default behavior).
    #[serde(default)]
    pub reuse: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpwnConfig {
    #[serde(default = "default_compose")]
    pub compose: String,
    #[serde(default)]
    pub services: BTreeMap<String, ServiceCfg>,
    #[serde(default)]
    pub volumes: Volumes,
    #[serde(default)]
    pub lifecycle: Lifecycle,
    #[serde(default)]
    pub resources: Option<Resources>,
    #[serde(default)]
    pub image: ImageCfg,
}

fn default_compose() -> String {
    "docker-compose.yml".to_string()
}
fn default_up() -> String {
    "on-demand".to_string()
}

/// Parse `worktree/spwn.yaml`. Returns `None` when the file is absent (the feature
/// is fully opt-in) or unparseable (logged; treated as absent so a session never
/// fails to open over a bad config).
pub fn read_config(worktree: &Path) -> Option<SpwnConfig> {
    let path = worktree.join("spwn.yaml");
    let text = std::fs::read_to_string(&path).ok()?;
    match serde_yaml::from_str::<SpwnConfig>(&text) {
        Ok(cfg) => Some(cfg),
        Err(e) => {
            eprintln!("spwn.yaml parse error ({}): {e}", path.display());
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Session context + naming
// ---------------------------------------------------------------------------

/// Everything a compose operation needs about the session it acts on.
pub struct SessionCtx {
    /// The session's terminal id (its stable key).
    pub terminal_id: String,
    /// A stable, sanitized slug for the user's repo (shared across its sessions),
    /// used for the shared-stack project name and content image tags.
    pub repo_slug: String,
    /// The session's worktree (holds spwn.yaml + the compose file).
    pub worktree: PathBuf,
    /// App data dir, for the shared-services ref-count file.
    pub data_dir: PathBuf,
}

/// First uuid segment — the same short id used for the `cm/<short>` branch.
pub fn short_id(terminal_id: &str) -> &str {
    terminal_id.split('-').next().unwrap_or(terminal_id)
}

/// Per-session compose project name.
pub fn project_name(terminal_id: &str) -> String {
    format!("spwn-{}", short_id(terminal_id))
}

/// Shared-services compose project name (one per repo).
fn shared_project(repo_slug: &str) -> String {
    format!("spwn-shared-{repo_slug}")
}

/// Sanitize an arbitrary name into a docker-safe slug ([a-z0-9-]).
pub fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "app".to_string() } else { s }
}

// ---------------------------------------------------------------------------
// Availability
// ---------------------------------------------------------------------------

/// Whether `docker compose` is usable (Docker installed + daemon reachable).
/// Not cached: the daemon can start/stop across the app's lifetime.
pub fn available() -> bool {
    Command::new("docker")
        .args(["compose", "version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Low-level `docker compose` invocation
// ---------------------------------------------------------------------------

/// Run `docker compose -p <project> [-f <file>…] <args>` in `worktree` with
/// `COMPOSE_PROJECT_NAME` set. Trimmed stdout on success, stderr on failure.
fn compose(
    worktree: &Path,
    project: &str,
    files: &[PathBuf],
    args: &[&str],
) -> Result<String, String> {
    let mut cmd = Command::new("docker");
    cmd.arg("compose").arg("-p").arg(project);
    for f in files {
        cmd.arg("-f").arg(f);
    }
    cmd.args(args);
    cmd.current_dir(worktree);
    cmd.env("COMPOSE_PROJECT_NAME", project);
    let out = cmd
        .output()
        .map_err(|e| format!("failed to run docker compose: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// The base compose file path (from config), resolved under the worktree.
fn base_file(ctx: &SessionCtx, cfg: &SpwnConfig) -> PathBuf {
    ctx.worktree.join(&cfg.compose)
}

/// The generated override path (`.spwn/override.gen.yml`).
fn override_file(ctx: &SessionCtx) -> PathBuf {
    ctx.worktree.join(".spwn").join("override.gen.yml")
}

/// Base + override (override included only when it exists on disk).
fn session_files(ctx: &SessionCtx, cfg: &SpwnConfig) -> Vec<PathBuf> {
    let mut files = vec![base_file(ctx, cfg)];
    let ov = override_file(ctx);
    if ov.exists() {
        files.push(ov);
    }
    files
}

// ---------------------------------------------------------------------------
// Compose config introspection (service list + which services build)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DcConfig {
    #[serde(default)]
    services: BTreeMap<String, DcService>,
}

#[derive(Debug, Deserialize)]
struct DcService {
    #[serde(default)]
    build: Option<Value>,
}

/// Resolve the compose project's services from the BASE file (before injections),
/// returning (all service names, service names that declare a `build:`). `None` if
/// `docker compose config` fails (we then fall back to plain `up` with no reuse).
fn resolved_services(ctx: &SessionCtx, cfg: &SpwnConfig) -> Option<(Vec<String>, HashSet<String>)> {
    let base = vec![base_file(ctx, cfg)];
    let json = compose(&ctx.worktree, &project_name(&ctx.terminal_id), &base, &["config", "--format", "json"]).ok()?;
    let parsed: DcConfig = serde_json::from_str(&json).ok()?;
    let all: Vec<String> = parsed.services.keys().cloned().collect();
    let build: HashSet<String> = parsed
        .services
        .iter()
        .filter(|(_, s)| s.build.is_some())
        .map(|(n, _)| n.clone())
        .collect();
    Some((all, build))
}

/// Names of services the user tagged `scope: shared`.
fn shared_service_names(cfg: &SpwnConfig) -> Vec<String> {
    cfg.services
        .iter()
        .filter(|(_, s)| s.scope == Scope::Shared)
        .map(|(n, _)| n.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Image content-tagging (lever #7: reuse the parent's image down the fork lineage)
// ---------------------------------------------------------------------------

/// Files whose contents define a build's DEPENDENCIES. Application source is
/// deliberately excluded — it's bind-mounted, so a code edit must NOT bust the tag.
const DEP_FILES: &[&str] = &[
    "Dockerfile",
    ".dockerignore",
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "Cargo.toml",
    "Cargo.lock",
    "poetry.lock",
    "requirements.txt",
    "go.sum",
];

/// A short, stable hash of the worktree's dependency inputs. Two worktrees with the
/// same deps (e.g. a fresh fork and its parent) produce the same hash.
fn deps_hash(worktree: &Path) -> String {
    let mut h = DefaultHasher::new();
    for f in DEP_FILES {
        if let Ok(bytes) = std::fs::read(worktree.join(f)) {
            f.hash(&mut h);
            bytes.hash(&mut h);
        }
    }
    format!("{:016x}", h.finish())
}

/// Content tag for a build service: `spwn-<repo>-<svc>:<deps-hash>`.
fn image_tag(ctx: &SessionCtx, service: &str) -> String {
    format!("spwn-{}-{}:{}", ctx.repo_slug, slug(service), deps_hash(&ctx.worktree))
}

/// True if an image with this tag already exists locally (built by a parent/sibling).
fn image_exists(tag: &str) -> bool {
    Command::new("docker")
        .args(["image", "inspect", tag])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Per-session override generation
// ---------------------------------------------------------------------------

/// Write `.spwn/override.gen.yml` carrying all spwn injections. Returns the path,
/// or `None` if there was nothing to inject.
fn write_override(
    ctx: &SessionCtx,
    cfg: &SpwnConfig,
    session_build: &HashSet<String>,
    session_names: &[String],
    has_shared: bool,
) -> Option<PathBuf> {
    let proj = project_name(&ctx.terminal_id);
    let nm_per_session = cfg.volumes.node_modules.as_deref() == Some("per-session");
    let nm_path = cfg
        .volumes
        .node_modules_path
        .clone()
        .unwrap_or_else(|| "/app/node_modules".to_string());

    let mut services = Map::new();
    for name in session_names {
        let mut svc = Map::new();

        // Pin build services to their content tag so forks reuse the parent's image.
        if session_build.contains(name) {
            svc.insert("image".into(), json!(image_tag(ctx, name)));
            svc.insert("pull_policy".into(), json!("never"));
        }

        // Per-session resource caps.
        if let Some(res) = &cfg.resources {
            let mut limits = Map::new();
            if let Some(m) = &res.memory {
                limits.insert("memory".into(), json!(m));
            }
            if let Some(c) = res.cpus {
                limits.insert("cpus".into(), json!(format!("{c}")));
            }
            if !limits.is_empty() {
                svc.insert("deploy".into(), json!({ "resources": { "limits": Value::Object(limits) } }));
            }
        }

        // Volumes: per-session node_modules mask + shared caches (merged by target).
        let mut vols: Vec<Value> = Vec::new();
        if nm_per_session {
            vols.push(json!(format!("{proj}-node_modules:{nm_path}")));
        }
        for c in &cfg.volumes.caches {
            vols.push(json!(format!("{}:{}", c.volume, c.path)));
        }
        if !vols.is_empty() {
            svc.insert("volumes".into(), Value::Array(vols));
        }

        // Wire session services onto the shared stack's network so they can reach
        // shared services (DB/cache) by service name.
        if has_shared {
            svc.insert("networks".into(), json!(["default", "shared"]));
        }

        if !svc.is_empty() {
            services.insert(name.clone(), Value::Object(svc));
        }
    }

    let mut root = Map::new();
    if !services.is_empty() {
        root.insert("services".into(), Value::Object(services));
    }

    // Top-level network + volume declarations.
    let mut networks = Map::new();
    if has_shared {
        networks.insert(
            "shared".into(),
            json!({ "external": true, "name": format!("{}_default", shared_project(&ctx.repo_slug)) }),
        );
    }
    if !networks.is_empty() {
        root.insert("networks".into(), Value::Object(networks));
    }

    let mut volumes = Map::new();
    if nm_per_session {
        // Per-session volume — compose scopes it to this project; `down -v` removes it.
        volumes.insert(format!("{proj}-node_modules"), Value::Null);
    }
    for c in &cfg.volumes.caches {
        // Shared caches are external (fixed name, created once) so all sessions share them.
        volumes.insert(c.volume.clone(), json!({ "external": true }));
    }
    if !volumes.is_empty() {
        root.insert("volumes".into(), Value::Object(volumes));
    }

    if root.is_empty() {
        return None;
    }

    let yaml = serde_yaml::to_string(&Value::Object(root)).ok()?;
    let dir = ctx.worktree.join(".spwn");
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let path = dir.join("override.gen.yml");
    std::fs::write(&path, yaml).ok()?;
    Some(path)
}

// ---------------------------------------------------------------------------
// Shared services ref-counting (lever #5)
// ---------------------------------------------------------------------------

/// Serializes read-modify-write on the shared ref-count file within this process.
static SHARED_LOCK: Mutex<()> = Mutex::new(());

fn shared_refs_path(data_dir: &Path) -> PathBuf {
    data_dir.join("compose-shared.json")
}

fn load_shared_refs(data_dir: &Path) -> BTreeMap<String, Vec<String>> {
    std::fs::read_to_string(shared_refs_path(data_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_shared_refs(data_dir: &Path, refs: &BTreeMap<String, Vec<String>>) {
    if let Ok(json) = serde_json::to_string_pretty(refs) {
        let _ = std::fs::write(shared_refs_path(data_dir), json);
    }
}

/// Bring the `scope: shared` services up once (idempotent) and add this session to
/// the ref-count. Pre-creates external cache volumes so they're shared across stacks.
fn ensure_shared(ctx: &SessionCtx, cfg: &SpwnConfig, shared_names: &[String]) -> Result<(), String> {
    if shared_names.is_empty() {
        return Ok(());
    }
    // External cache volumes must exist before any stack references them.
    for c in &cfg.volumes.caches {
        let _ = Command::new("docker").args(["volume", "create", &c.volume]).output();
    }

    let shared_proj = shared_project(&ctx.repo_slug);
    let base = vec![base_file(ctx, cfg)];
    let mut args = vec!["up", "-d"];
    for n in shared_names {
        args.push(n);
    }
    compose(&ctx.worktree, &shared_proj, &base, &args)?;

    let _guard = SHARED_LOCK.lock().unwrap();
    let mut refs = load_shared_refs(&ctx.data_dir);
    let set = refs.entry(shared_proj).or_default();
    if !set.contains(&ctx.terminal_id) {
        set.push(ctx.terminal_id.clone());
    }
    save_shared_refs(&ctx.data_dir, &refs);
    Ok(())
}

/// Drop this session's ref on the shared stack; `stop` the shared stack only when
/// no session still refs it (keeps volumes/data warm; never `down -v`).
fn release_shared(ctx: &SessionCtx, cfg: &SpwnConfig) {
    let shared_proj = shared_project(&ctx.repo_slug);
    let _guard = SHARED_LOCK.lock().unwrap();
    let mut refs = load_shared_refs(&ctx.data_dir);
    let now_empty = if let Some(set) = refs.get_mut(&shared_proj) {
        set.retain(|id| id != &ctx.terminal_id);
        set.is_empty()
    } else {
        false
    };
    if now_empty {
        refs.remove(&shared_proj);
        let base = vec![base_file(ctx, cfg)];
        let _ = compose(&ctx.worktree, &shared_proj, &base, &["stop"]);
    }
    save_shared_refs(&ctx.data_dir, &refs);
}

// ---------------------------------------------------------------------------
// Public lifecycle operations
// ---------------------------------------------------------------------------

/// Bring up this session's stack: ensure shared services, build/reuse content-tagged
/// images, write the override, and `up -d` the session-scope services.
pub fn up(ctx: &SessionCtx, cfg: &SpwnConfig) -> Result<(), String> {
    if !available() {
        return Err("Docker isn't available (is Docker Desktop running?)".to_string());
    }
    let proj = project_name(&ctx.terminal_id);
    let shared_names = shared_service_names(cfg);
    let has_shared = !shared_names.is_empty();

    ensure_shared(ctx, cfg, &shared_names)?;

    // Content-tag image reuse is on unless `image.reuse: off`.
    let reuse = cfg.image.reuse.as_deref() != Some("off");

    match resolved_services(ctx, cfg) {
        Some((all, build)) => {
            let shared_set: HashSet<&String> = shared_names.iter().collect();
            let session_names: Vec<String> =
                all.into_iter().filter(|n| !shared_set.contains(n)).collect();
            let session_build: HashSet<String> =
                build.iter().filter(|n| !shared_set.contains(n)).cloned().collect();

            // Only pin/reuse content-tagged images when reuse is enabled.
            let pinned = if reuse { session_build.clone() } else { HashSet::new() };
            write_override(ctx, cfg, &pinned, &session_names, has_shared);
            let files = session_files(ctx, cfg);

            // Any session service asking to gate readiness on its healthcheck.
            let want_wait = session_names.iter().any(|n| {
                cfg.services.get(n).map(|s| s.wait_for_healthy).unwrap_or(false)
            });

            if reuse {
                // Build only content tags that don't already exist (fork reuse).
                for svc in &session_build {
                    if !image_exists(&image_tag(ctx, svc)) {
                        compose(&ctx.worktree, &proj, &files, &["build", svc])?;
                    }
                }
                let mut args = vec!["up", "-d", "--no-build"];
                if want_wait {
                    args.push("--wait");
                }
                args.extend(session_names.iter().map(String::as_str));
                compose(&ctx.worktree, &proj, &files, &args)?;
            } else {
                let mut args = vec!["up", "-d", "--build"];
                if want_wait {
                    args.push("--wait");
                }
                args.extend(session_names.iter().map(String::as_str));
                compose(&ctx.worktree, &proj, &files, &args)?;
            }
        }
        None => {
            // Couldn't introspect the compose file — degrade to a plain build+up of
            // everything (no image reuse, no shared split).
            write_override(ctx, cfg, &HashSet::new(), &[], has_shared);
            let files = session_files(ctx, cfg);
            compose(&ctx.worktree, &proj, &files, &["up", "-d"])?;
        }
    }
    Ok(())
}

/// Tear down this session's stack (`down -v`) and release its shared-services ref.
/// Must run BEFORE the worktree is removed (the compose file lives inside it).
pub fn down(ctx: &SessionCtx, cfg: &SpwnConfig) {
    let proj = project_name(&ctx.terminal_id);
    let files = session_files(ctx, cfg);
    if let Err(e) = compose(&ctx.worktree, &proj, &files, &["down", "-v", "--remove-orphans"]) {
        // Fall back to a file-less down (compose still knows the project by label)
        // in case the worktree/compose file is already gone.
        eprintln!("compose down ({proj}) failed, retrying without files: {e}");
        let _ = compose(&ctx.worktree, &proj, &[], &["down", "-v", "--remove-orphans"]);
    }
    release_shared(ctx, cfg);
}

/// Idle-stop: free CPU/RAM while keeping volumes warm.
pub fn stop(ctx: &SessionCtx, cfg: &SpwnConfig) -> Result<(), String> {
    let proj = project_name(&ctx.terminal_id);
    compose(&ctx.worktree, &proj, &session_files(ctx, cfg), &["stop"]).map(|_| ())
}

/// Resume a stopped stack (near-instant; no rebuild).
pub fn start(ctx: &SessionCtx, cfg: &SpwnConfig) -> Result<(), String> {
    let proj = project_name(&ctx.terminal_id);
    compose(&ctx.worktree, &proj, &session_files(ctx, cfg), &["start"]).map(|_| ())
}

/// Recent logs for one service (last `tail` lines, no color codes, non-following).
pub fn logs(ctx: &SessionCtx, cfg: &SpwnConfig, service: &str, tail: u32) -> Result<String, String> {
    let proj = project_name(&ctx.terminal_id);
    compose(
        &ctx.worktree,
        &proj,
        &session_files(ctx, cfg),
        &["logs", "--no-color", "--tail", &tail.to_string(), service],
    )
}

// ---------------------------------------------------------------------------
// Status + live URLs
// ---------------------------------------------------------------------------

/// A running service surfaced to the UI: its role, state, and live URL (if any).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceState {
    pub name: String,
    pub role: Option<String>,
    pub scope: String,
    /// Compose container state, e.g. "running" | "exited" | "-" (not created).
    pub state: String,
    /// First surfaced live URL for this service, if it publishes a port.
    pub url: Option<String>,
}

/// Overall compose status for a session, for the Services panel.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ComposeStatus {
    pub available: bool,
    pub project: Option<String>,
    pub services: Vec<ServiceState>,
}

#[derive(Debug, Deserialize)]
struct PsRow {
    #[serde(rename = "Service", default)]
    service: String,
    #[serde(rename = "State", default)]
    state: String,
}

/// Parse `docker compose ps --format json` (NDJSON on newer Docker, a JSON array on
/// some versions) into service→state.
fn parse_ps(text: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if let Ok(rows) = serde_json::from_str::<Vec<PsRow>>(text) {
        for r in rows {
            map.insert(r.service, r.state);
        }
        return map;
    }
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        if let Ok(r) = serde_json::from_str::<PsRow>(line) {
            map.insert(r.service, r.state);
        }
    }
    map
}

/// The live host URL for a service's container port, via `docker compose port`.
fn service_url(ctx: &SessionCtx, cfg: &SpwnConfig, service: &str, port: u16) -> Option<String> {
    let proj = project_name(&ctx.terminal_id);
    let out = compose(
        &ctx.worktree,
        &proj,
        &session_files(ctx, cfg),
        &["port", service, &port.to_string()],
    )
    .ok()?;
    // Output like "0.0.0.0:49173" — take the last colon-separated field as the port.
    let host_port = out.rsplit(':').next()?.trim();
    if host_port.is_empty() || host_port.parse::<u16>().is_err() {
        return None;
    }
    Some(format!("http://localhost:{host_port}"))
}

/// Full status for the Services panel: each spwn-declared service with its role,
/// compose state, and (for `service`-role services) a live URL.
pub fn status(ctx: &SessionCtx, cfg: &SpwnConfig) -> ComposeStatus {
    if !available() {
        return ComposeStatus { available: false, ..Default::default() };
    }
    let proj = project_name(&ctx.terminal_id);
    let states = compose(&ctx.worktree, &proj, &session_files(ctx, cfg), &["ps", "-a", "--format", "json"])
        .map(|s| parse_ps(&s))
        .unwrap_or_default();

    let mut services = Vec::new();
    for (name, svc) in &cfg.services {
        let state = states.get(name).cloned().unwrap_or_else(|| "-".to_string());
        let url = if state == "running" {
            svc.ports.first().and_then(|p| service_url(ctx, cfg, name, *p))
        } else {
            None
        };
        services.push(ServiceState {
            name: name.clone(),
            role: svc.role.clone(),
            scope: match svc.scope {
                Scope::Shared => "shared".to_string(),
                Scope::Session => "session".to_string(),
            },
            state,
            url,
        });
    }
    ComposeStatus { available: true, project: Some(proj), services }
}

// ---------------------------------------------------------------------------
// Startup reconcile
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct LsRow {
    #[serde(rename = "Name", default)]
    name: String,
}

/// All compose project names starting with `spwn-`.
pub fn list_spwn_projects() -> Vec<String> {
    let out = Command::new("docker")
        .args(["compose", "ls", "-a", "--format", "json"])
        .output();
    let Ok(out) = out else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let rows: Vec<LsRow> = serde_json::from_str(&text).unwrap_or_default();
    rows.into_iter()
        .map(|r| r.name)
        .filter(|n| n.starts_with("spwn-"))
        .collect()
}

/// Reap orphaned per-session stacks: any `spwn-<short>` project whose `<short>` is no
/// longer among the live sessions' short ids is torn down. Shared stacks
/// (`spwn-shared-*`) are left to the ref-count and never reaped here.
pub fn reap_orphans(live_short_ids: &HashSet<String>) {
    if !available() {
        return;
    }
    for project in list_spwn_projects() {
        if project.starts_with("spwn-shared-") {
            continue;
        }
        let short = project.strip_prefix("spwn-").unwrap_or(&project);
        if !live_short_ids.contains(short) {
            let _ = Command::new("docker")
                .args(["compose", "-p", &project, "down", "-v", "--remove-orphans"])
                .output();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The documented example spwn.yaml must parse into the expected model — guards
    /// against serde field-name (snake/kebab) drift between the schema and the docs.
    #[test]
    fn parses_example_spwn_yaml() {
        let text = include_str!("../../examples/compose-integration/spwn.yaml");
        let cfg: SpwnConfig = serde_yaml::from_str(text).expect("example spwn.yaml parses");

        assert_eq!(cfg.compose, "docker-compose.yml");
        assert_eq!(cfg.services.get("db").unwrap().scope, Scope::Shared);

        let app = cfg.services.get("app").unwrap();
        assert_eq!(app.scope, Scope::Session);
        assert_eq!(app.role.as_deref(), Some("service"));
        assert!(app.wait_for_healthy);
        assert_eq!(app.ports, vec![3000]);

        assert_eq!(cfg.lifecycle.up, "on-demand");
        assert_eq!(cfg.lifecycle.idle_stop.as_deref(), Some("15m"));
        assert_eq!(cfg.volumes.node_modules.as_deref(), Some("host"));
        assert_eq!(cfg.volumes.caches.len(), 1);
        assert_eq!(cfg.volumes.caches[0].volume, "spwn-npm-cache");
        assert_eq!(cfg.resources.as_ref().unwrap().cpus, Some(1.5));
        assert_eq!(cfg.image.reuse.as_deref(), Some("lineage"));
    }

    #[test]
    fn project_name_uses_short_id() {
        assert_eq!(project_name("dc5be0b0-8496-4ba3-a3a3-94bca61071d3"), "spwn-dc5be0b0");
        assert_eq!(slug("My Repo!"), "my-repo");
    }

    #[test]
    fn parse_idle_units() {
        assert_eq!(parse_idle_str("30s"), Some(std::time::Duration::from_secs(30)));
        assert_eq!(parse_idle_str("15m"), Some(std::time::Duration::from_secs(900)));
        assert_eq!(parse_idle_str("2h"), Some(std::time::Duration::from_secs(7200)));
        assert_eq!(parse_idle_str("nope"), None);
    }

    // Mirror of commands::parse_idle so the duration grammar is unit-tested here.
    fn parse_idle_str(s: &str) -> Option<std::time::Duration> {
        let s = s.trim();
        let idx = s.find(|c: char| !c.is_ascii_digit())?;
        let (num, unit) = s.split_at(idx);
        let n: u64 = num.parse().ok()?;
        let secs = match unit {
            "s" => n,
            "m" => n * 60,
            "h" => n * 3600,
            _ => return None,
        };
        Some(std::time::Duration::from_secs(secs))
    }
}
