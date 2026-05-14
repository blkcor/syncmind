use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{WebviewUrl, WebviewWindowBuilder};

mod commands;

use commands::*;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            config: Mutex::new(syncmind_core::Config::default()),
        })
        .invoke_handler(tauri::generate_handler![
            search_knowledge,
            get_config,
            update_config,
            get_indexing_status,
            trigger_reindex,
            open_file,
            reveal_in_finder,
        ])
        .setup(|app| {
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("SyncMind")
                .inner_size(860.0, 540.0)
                .resizable(false)
                .decorations(false)
                .center()
                .focused(true)
                .visible(false)
                .build()?;

            let window_clone = window.clone();
            let window_clone2 = window.clone();

            // Hide window on blur
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::Focused(false) = event {
                    let _ = window_clone.hide();
                }
            });

            // System tray
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

            TrayIconBuilder::new()
                .menu(&menu)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        let _ = window_clone2.show();
                        let _ = window_clone2.set_focus();
                    }
                    _ => {}
                })
                .build(app)?;

            // Global shortcut: Cmd+Shift+Space
            let window_for_shortcut = window.clone();
            app.handle().plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_shortcuts(["Cmd+Shift+Space"])?
                    .with_handler(move |_app, shortcut, _event| {
                        if shortcut.matches(
                            tauri_plugin_global_shortcut::Modifiers::SUPER | tauri_plugin_global_shortcut::Modifiers::SHIFT,
                            tauri_plugin_global_shortcut::Code::Space,
                        ) {
                            let _ = window_for_shortcut.show();
                            let _ = window_for_shortcut.set_focus();
                        }
                    })
                    .build(),
            )?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

pub struct AppState {
    config: Mutex<syncmind_core::Config>,
}
