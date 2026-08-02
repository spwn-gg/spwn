mod checkpoints;
mod claude;
mod commands;
mod gitwt;
mod hooks;
mod projects;
mod pty;
mod scheduler;
mod server;
mod settings;
mod state;
mod store;
mod transcript;

pub use server::{serve, ServeOpts};

/// CLI entry for the `spwn prompt …` helper that hooks invoke to raise a UI prompt.
/// Returns `(exit code, optional stdout line)`; `main` owns the actual print + exit so
/// the client logic stays unit-testable. Called before the server boots when spwn is
/// run as `spwn prompt …`.
pub fn run_prompt_cli(args: &[String]) -> (i32, Option<String>) {
    let out = hooks::run_prompt_cli(args);
    (out.code, out.stdout)
}

/// CLI entry for the `spwn checkpoint <turn_uuid>` helper the default `session-turn`
/// hook invokes to snapshot the working tree. Reads `SPWN_SESSION_ID` and
/// `SPWN_WORKTREE` from the environment (set by the hook runner). No-ops (exit 0) when
/// there's no session id yet. Runs as a short-lived subprocess — no server / AppState.
pub fn run_checkpoint_cli(args: &[String]) -> i32 {
    let Some(turn_uuid) = args.first() else {
        eprintln!("spwn checkpoint: missing turn uuid");
        return 2;
    };
    // No bound session yet → nothing to snapshot (not an error).
    let session_id = match std::env::var("SPWN_SESSION_ID") {
        Ok(s) if !s.is_empty() => s,
        _ => return 0,
    };
    let worktree = match std::env::var("SPWN_WORKTREE") {
        Ok(w) if !w.is_empty() => w,
        _ => return 0,
    };
    let Some(app_data) = checkpoints::default_app_data_dir() else {
        eprintln!("spwn checkpoint: could not resolve the app data dir");
        return 1;
    };
    match checkpoints::capture(
        &app_data,
        std::path::Path::new(&worktree),
        &session_id,
        turn_uuid,
        "turn",
    ) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("spwn checkpoint: {e}");
            1
        }
    }
}
