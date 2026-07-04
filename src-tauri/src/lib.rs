mod checkpoints;
mod claude;
mod commands;
mod projects;
mod pty;
mod scheduler;
mod settings;
mod state;
mod store;
mod transcript;

use state::AppState;
use std::sync::atomic::Ordering;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;

/// Show and focus the main window (recreating nothing — it's hidden, not closed).
fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
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

            // Menu-bar tray: keeps the app alive when the window is closed so the
            // scheduler keeps ticking. Left-click or "Show" reopens the window;
            // "Quit" is the only real exit (sets the quitting flag first).
            let show = MenuItem::with_id(app, "show", "Show spwn", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit spwn", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            // macOS has no "window icon", so default_window_icon() is None here —
            // decode the bundled PNG for the menu-bar icon instead.
            let tray_icon = match app.default_window_icon() {
                Some(icon) => icon.clone(),
                None => tauri::image::Image::from_bytes(include_bytes!("../icons/128x128.png"))?,
            };
            let _tray = TrayIconBuilder::with_id("main")
                .icon(tray_icon)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "quit" => {
                        app.state::<AppState>().quitting.store(true, Ordering::SeqCst);
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;

            // Closing the window hides it (background/tray) instead of quitting,
            // unless a real quit is in progress.
            if let Some(window) = app.get_webview_window("main") {
                let w = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        if !w.app_handle().state::<AppState>().quitting.load(Ordering::SeqCst) {
                            api.prevent_close();
                            let _ = w.hide();
                        }
                    }
                });
            }

            // Start the per-project scheduled-task loop.
            scheduler::start_scheduler(app.handle().clone());
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
            commands::add_scheduled_task,
            commands::update_scheduled_task,
            commands::set_scheduled_task_enabled,
            commands::remove_scheduled_task,
            commands::run_scheduled_task_now,
            commands::clear_terminal_attention,
            commands::open_terminal,
            commands::close_terminal,
            commands::delete_terminal,
            commands::set_terminal_session,
            commands::claude_send,
            commands::claude_permission,
            commands::claude_set_mode,
            commands::claude_interrupt,
            commands::claude_answer,
            commands::claude_rewind,
            commands::claude_rewind_restore,
            commands::checkpoint_project,
            commands::restore_checkpoint,
            commands::list_checkpoints,
            commands::write_to_pty,
            commands::resize_pty,
            commands::read_transcript,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| match event {
            // Safety net: if an exit is requested (e.g. Cmd-Q) but we're not really
            // quitting, stay alive in the background so the scheduler keeps running.
            tauri::RunEvent::ExitRequested { api, .. } => {
                if !app_handle.state::<AppState>().quitting.load(Ordering::SeqCst) {
                    api.prevent_exit();
                }
            }
            // Don't leave orphaned sidecar node processes when the app quits.
            tauri::RunEvent::Exit => {
                let state = app_handle.state::<AppState>();
                for (_, mut agent) in state.claude_agents.lock().drain() {
                    agent.kill();
                }
            }
            _ => {}
        });
}
