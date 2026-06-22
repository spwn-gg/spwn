mod claude;
mod commands;
mod projects;
mod pty;
mod settings;
mod state;
mod store;
mod transcript;

use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            // Stash the app handle so background helpers (e.g. persist) can emit.
            *app.state::<AppState>().app.lock() = Some(app.handle().clone());

            // Point the rmux SDK at a daemon binary it can launch. Prefer the
            // bundled sidecar (next to the app binary in Contents/MacOS), falling
            // back to a system rmux for `tauri dev`. A Finder-launched app doesn't
            // inherit the shell PATH, so we always resolve an absolute path.
            if std::env::var_os("RMUX_SDK_DAEMON_BINARY").is_none() {
                let bundled = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.join("rmux")))
                    .filter(|p| p.exists());
                if let Some(rmux) = bundled.or_else(pty::find_rmux_bin) {
                    std::env::set_var("RMUX_SDK_DAEMON_BINARY", rmux);
                }
            }

            // Load the persisted project store and settings.
            if let Ok(data_dir) = app.path().app_data_dir() {
                let state = app.state::<AppState>();

                let store_path = store::store_path(&data_dir);
                *state.store.lock() = store::ProjectStore::load(&store_path);
                *state.store_path.lock() = Some(store_path);

                let settings_path = settings::settings_path(&data_dir);
                *state.settings.lock() = settings::Settings::load(&settings_path);
                *state.settings_path.lock() = Some(settings_path);
            }

            // Watch ~/.claude/projects so the transcript panel refreshes live.
            let root = projects::projects_root();
            match projects::start_watcher(app.handle().clone(), &root) {
                Ok(w) => *app.state::<AppState>().watcher.lock() = Some(w),
                Err(e) => eprintln!("failed to start projects watcher: {e}"),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::find_claude,
            commands::get_settings,
            commands::set_settings,
            commands::list_projects,
            commands::create_project,
            commands::delete_project,
            commands::open_in_vscode,
            commands::add_context_block,
            commands::add_context_file,
            commands::remove_context_block,
            commands::update_context_block,
            commands::reorder_context,
            commands::clear_context,
            commands::open_terminal,
            commands::close_terminal,
            commands::delete_terminal,
            commands::set_terminal_session,
            commands::claude_send,
            commands::claude_permission,
            commands::claude_set_mode,
            commands::claude_interrupt,
            commands::claude_answer,
            commands::write_to_pty,
            commands::resize_pty,
            commands::read_transcript,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Don't leave orphaned sidecar node processes when the app quits.
            if let tauri::RunEvent::Exit = event {
                let state = app_handle.state::<AppState>();
                for (_, mut agent) in state.claude_agents.lock().drain() {
                    agent.kill();
                }
            }
        });
}
