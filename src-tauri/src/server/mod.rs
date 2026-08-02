//! The web server: replaces the Tauri shell.
//!
//! `serve` boots the same backend the Tauri app did (store/settings load, default
//! hooks, fs watcher, scheduler) minus the tray/window, then serves an axum app:
//! a generic `/api/invoke/:command` HTTP surface over the old Tauri commands, one
//! multiplexed WebSocket at `/ws` fed by the [`EventHub`], and the embedded SPA.

pub mod fs;
pub mod hub;
pub mod routes;
pub mod ws;

use crate::state::AppState;
use crate::{checkpoints, hooks, projects, scheduler, settings, store};
use axum::routing::{get, post};
use axum::Router;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Options for `spwn serve`.
pub struct ServeOpts {
    pub host: IpAddr,
    pub port: u16,
    /// Skip auto-opening the browser.
    pub no_open: bool,
}

impl Default for ServeOpts {
    fn default() -> Self {
        Self {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 4317,
            no_open: false,
        }
    }
}

/// Boot the backend and run the web server until interrupted.
pub async fn serve(opts: ServeOpts) -> anyhow::Result<()> {
    let state = Arc::new(AppState::default());

    // Point the rmux SDK at a daemon binary it can launch: prefer one bundled next
    // to the executable, else a system rmux on PATH.
    if std::env::var_os("RMUX_SDK_DAEMON_BINARY").is_none() {
        let bundled = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("rmux")))
            .filter(|p| p.exists());
        if let Some(rmux) = bundled.or_else(crate::pty::find_rmux_bin) {
            std::env::set_var("RMUX_SDK_DAEMON_BINARY", rmux);
        }
    }

    // Load the persisted project store + settings from the app data dir.
    if let Some(data_dir) = checkpoints::default_app_data_dir() {
        let _ = std::fs::create_dir_all(&data_dir);
        let store_path = store::store_path(&data_dir);
        *state.store.lock() = store::ProjectStore::load(&store_path);
        *state.store_path.lock() = Some(store_path);

        let settings_path = settings::settings_path(&data_dir);
        *state.settings.lock() = settings::Settings::load(&settings_path);
        *state.settings_path.lock() = Some(settings_path);
    } else {
        eprintln!("warning: could not resolve the app data dir; state won't persist");
    }

    // Install spwn's built-in per-session behaviors as default global hooks.
    hooks::install_default_global_hooks();

    // Watch ~/.claude/projects so the transcript panel refreshes live.
    let root = projects::projects_root();
    match projects::start_watcher(state.hub.clone(), &root) {
        Ok(w) => *state.watcher.lock() = Some(w),
        Err(e) => eprintln!("failed to start projects watcher: {e}"),
    }

    // Start the per-project scheduled-task loop (the running server keeps it alive).
    scheduler::start_scheduler(state.clone());

    let app = router(state.clone());

    let addr = SocketAddr::new(opts.host, opts.port);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    let url = format!("http://{}:{}", opts.host, bound.port());
    println!("spwn serving at {url}");

    if !opts.no_open {
        if let Err(e) = open::that(&url) {
            eprintln!("couldn't open the browser automatically ({e}); visit {url}");
        }
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(state.clone()))
        .await?;
    Ok(())
}

/// Assemble the axum router: the invoke surface, the WebSocket, small JSON helpers,
/// and the embedded SPA fallback.
fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/invoke/:command", post(routes::invoke))
        .route("/api/version", get(routes::version))
        .route("/api/fs/list", get(fs::list))
        .route("/ws", get(ws::handler))
        .fallback(routes::static_handler)
        .with_state(state)
}

/// Wait for Ctrl-C / SIGTERM, then tear down Claude sidecars (the old
/// `RunEvent::Exit` behavior) so no orphaned node processes remain. rmux shell
/// sessions are intentionally left alive (they persist across restarts).
async fn shutdown_signal(state: Arc<AppState>) {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    state.quitting.store(true, Ordering::SeqCst);
    for (_, mut agent) in state.claude_agents.lock().drain() {
        agent.kill();
    }
    eprintln!("\nshutting down.");
}
