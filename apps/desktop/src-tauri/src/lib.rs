use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::mpsc;
use tracing::{error, info};

mod commands;

use commands::*;

pub struct AppState {
    pub config: Mutex<syncmind_core::Config>,
    pub store: Arc<syncmind_storage::VectorStore>,
    pub embedder: Arc<dyn syncmind_rag_engine::embedder::Embedder>,
    pub watcher: Mutex<Option<syncmind_file_watcher::FileWatcher>>,
    pub indexing_handle: Mutex<Option<tauri::async_runtime::JoinHandle<anyhow::Result<()>>>>,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            search_knowledge,
            get_config,
            update_config,
            get_indexing_status,
            trigger_reindex,
            open_file,
            reveal_in_finder,
            is_auto_launch_enabled,
            set_auto_launch,
        ])
        .setup(|app| {
            // Core runtime initialization
            let config = syncmind_core::Config::load()
                .context("Failed to load SyncMind config")
                .map_err(|e| {
                    error!(error = %e, "config load failed");
                    e.to_string()
                })?;

            let db_path = syncmind_core::db_path()
                .context("Failed to resolve DB path")
                .map_err(|e| e.to_string())?;
            if let Some(parent) = db_path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create DB directory at {}", parent.display()))
                    .map_err(|e| e.to_string())?;
            }

            let store = syncmind_storage::VectorStore::new(&db_path, config.embedding_dim)
                .context("Failed to open VectorStore")
                .map_err(|e| e.to_string())?;
            let store = Arc::new(store);

            let embedder = tauri::async_runtime::block_on(async {
                syncmind_rag_engine::embedder::AutoEmbedder::new(&config)
                    .await
                    .context("Failed to create embedder")
            })
            .map_err(|e| {
                error!(error = %e, "embedder creation failed");
                e.to_string()
            })?;
            let embedder: Arc<dyn syncmind_rag_engine::embedder::Embedder> = Arc::new(embedder);

            // Index all registered files on startup.
            let extractor = syncmind_rag_engine::extractor::CompositeExtractor::new();
            for path in &config.registered_files {
                let chunker = syncmind_indexing::chunker_for_path(path, config.chunk_size, config.chunk_overlap);
                if let Err(e) = tauri::async_runtime::block_on(async {
                    syncmind_indexing::index_file(path, &extractor, chunker.as_ref(), embedder.as_ref(), &store).await
                }) {
                    error!(path = %path.display(), error = %e, "startup indexing failed");
                }
            }

            // Start file watcher with 1-second debounce.
            let (file_tx, file_rx) = mpsc::channel::<Vec<std::path::PathBuf>>(256);
            let watcher = syncmind_file_watcher::FileWatcher::new(
                config.registered_files.clone(),
                Duration::from_secs(1),
                file_tx,
            )
            .map_err(|e| {
                error!(error = %e, "file watcher creation failed");
                e.to_string()
            })?;

            // Spawn background indexing pipeline.
            let indexing_handle = tauri::async_runtime::spawn(syncmind_indexing::run_indexing_pipeline(
                config.clone(),
                Arc::clone(&store),
                Arc::clone(&embedder),
                file_rx,
            ));

            app.manage(AppState {
                config: Mutex::new(config),
                store,
                embedder,
                watcher: Mutex::new(Some(watcher)),
                indexing_handle: Mutex::new(Some(indexing_handle)),
            });

            // Window setup
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
            let _window_clone3 = window.clone();

            // Hide window on blur.
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::Focused(false) = event {
                    let _ = window_clone.hide();
                }
            });

            // Hide window on Esc key via injected JS that calls the Tauri window hide API.
            let window_for_esc = window.clone();
            let _ = window_for_esc.eval(r#"
                document.addEventListener('keydown', function(e) {
                    if (e.key === 'Escape') {
                        if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) {
                            window.__TAURI_INTERNALS__.invoke('plugin:window|hide');
                        }
                    }
                });
            "#);

            // System tray
            let open_palette_i = MenuItem::with_id(app, "open_palette", "Open Palette", true, None::<&str>)?;
            let settings_i = MenuItem::with_id(app, "settings", "Settings...", true, None::<&str>)?;
            let indexing_status_i = MenuItem::with_id(app, "indexing_status", "Indexing Status", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_palette_i, &settings_i, &indexing_status_i, &quit_i])?;

            TrayIconBuilder::new()
                .menu(&menu)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "open_palette" => {
                        let _ = window_clone2.show();
                        let _ = window_clone2.set_focus();
                    }
                    "settings" => {
                        let _ = window_clone2.show();
                        let _ = window_clone2.set_focus();
                        let _ = window_clone2.emit("navigate", "settings");
                    }
                    "indexing_status" => {
                        let _ = window_clone2.show();
                        let _ = window_clone2.set_focus();
                        let _ = window_clone2.emit("navigate", "indexing");
                    }
                    _ => {}
                })
                .build(app)?;

            // Global shortcut: Cmd+Shift+Space (toggle)
            let window_for_shortcut = window.clone();
            app.handle().plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_shortcuts(["Cmd+Shift+Space"])?
                    .with_handler(move |_app, shortcut, _event| {
                        if shortcut.matches(
                            tauri_plugin_global_shortcut::Modifiers::SUPER
                                | tauri_plugin_global_shortcut::Modifiers::SHIFT,
                            tauri_plugin_global_shortcut::Code::Space,
                        ) {
                            if let Ok(true) = window_for_shortcut.is_visible() {
                                let _ = window_for_shortcut.hide();
                            } else {
                                let _ = window_for_shortcut.show();
                                let _ = window_for_shortcut.set_focus();
                            }
                        }
                    })
                    .build(),
            )?;

            Ok(())
        })
        .on_window_event(|_window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Hide instead of close so the tray keeps the app alive.
                api.prevent_close();
                let _ = _window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                info!("Exit requested, performing graceful shutdown");

                if let Some(state) = _app_handle.try_state::<AppState>() {
                    // Stop file watcher.
                    if let Ok(mut watcher_guard) = state.watcher.lock() {
                        *watcher_guard = None;
                    }

                    // Abort indexing pipeline.
                    if let Ok(mut handle_guard) = state.indexing_handle.lock() {
                        if let Some(handle) = handle_guard.take() {
                            handle.abort();
                        }
                    }

                    // We rely on process exit to drop managed state (VectorStore, Embedder).
                }

                api.prevent_exit();
            }
        });
}
