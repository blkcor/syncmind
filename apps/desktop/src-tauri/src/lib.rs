use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{include_image, Emitter, Listener, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::ShortcutState;
use tokio::sync::mpsc;
use tracing::{error, info};

mod commands;

use commands::*;

/// Maximum number of indexing errors retained in `IndexingState`.
const MAX_RECENT_ERRORS: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexingErrorEntry {
    pub file_path: PathBuf,
    pub message: String,
    pub timestamp: i64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct IndexingState {
    /// Unix seconds of the most recent successful per-file index.
    pub last_updated: Option<i64>,
    /// Bounded ring of recent failures (newest at back).
    pub recent_errors: VecDeque<IndexingErrorEntry>,
}

impl IndexingState {
    /// Record a successful file index.
    pub fn record_success(&mut self, timestamp: i64) {
        self.last_updated = Some(timestamp);
        // A successful pass for a file evicts any prior error for the same path.
        // Errors for *other* files remain so the tray stays in "error" state
        // until every previously-failing file recovers.
    }

    /// Record an indexing failure for `path`. Trims the ring to `MAX_RECENT_ERRORS`.
    pub fn record_error(&mut self, path: PathBuf, message: String, timestamp: i64) {
        self.recent_errors.push_back(IndexingErrorEntry {
            file_path: path,
            message,
            timestamp,
        });
        while self.recent_errors.len() > MAX_RECENT_ERRORS {
            self.recent_errors.pop_front();
        }
    }

    /// Remove any stored error for `path` (called on successful re-index).
    pub fn clear_error_for(&mut self, path: &std::path::Path) {
        self.recent_errors.retain(|e| e.file_path != path);
    }

    pub fn is_healthy(&self) -> bool {
        self.recent_errors.is_empty()
    }
}

pub struct AppState {
    pub config: Mutex<syncmind_core::Config>,
    pub store: Arc<syncmind_storage::VectorStore>,
    pub embedder: Arc<dyn syncmind_rag_engine::embedder::Embedder>,
    pub watcher: Mutex<Option<syncmind_file_watcher::FileWatcher>>,
    pub indexing_handle: Mutex<Option<tauri::async_runtime::JoinHandle<anyhow::Result<()>>>>,
    pub indexing: Arc<Mutex<IndexingState>>,
    /// Shared callback used by every indexing path (startup, watcher pipeline,
    /// manual re-index command) so all three update `indexing` consistently.
    pub on_index_result: syncmind_indexing::IndexResultCallback,
    /// When true, the auto-hide on Focused(false) is suppressed so modal
    /// dialogs (file picker, etc.) don't get dismissed on macOS.
    pub dialog_open: Mutex<bool>,
}

/// Set the app as an accessory (menu-bar-only) app on macOS.
/// Hides the Dock icon and removes the app from Cmd+Tab.
/// This works at runtime so it also applies in `cargo tauri dev`.
#[cfg(target_os = "macos")]
fn set_activation_policy_accessory() {
    use objc::runtime::Object;
    use objc::{msg_send, sel, sel_impl};
    unsafe {
        let cls = objc::runtime::Class::get("NSApplication").unwrap();
        let app: *mut Object = msg_send![cls, sharedApplication];
        // NSApplicationActivationPolicyAccessory = 1
        let _: () = msg_send![app, setActivationPolicy: 1i32];
    }
}

#[cfg(not(target_os = "macos"))]
fn set_activation_policy_accessory() {}

/// Activate the app on macOS so the palette window can steal focus.
/// Required for LSUIElement (accessory) apps which are not auto-activated.
#[cfg(target_os = "macos")]
fn activate_app() {
    use objc::runtime::{Object, YES};
    use objc::{msg_send, sel, sel_impl};
    unsafe {
        let cls = objc::runtime::Class::get("NSApplication").unwrap();
        let app: *mut Object = msg_send![cls, sharedApplication];
        let _: () = msg_send![app, activateIgnoringOtherApps: YES];
    }
}

#[cfg(not(target_os = "macos"))]
fn activate_app() {}

/// Show and focus a window, ensuring the app is activated on macOS.
fn show_and_focus(window: &tauri::WebviewWindow) {
    activate_app();
    let _ = window.show();
    let _ = window.set_focus();
}

/// Swap the tray icon between healthy (template-rendered) and error (full-color)
/// variants. The error variant disables template rendering so the red accent
/// is preserved instead of being collapsed to monochrome by the OS.
fn apply_tray_health(tray: &TrayIcon, healthy: bool) {
    let icon = if healthy {
        include_image!("icons/tray.png")
    } else {
        include_image!("icons/tray-error.png")
    };
    let _ = tray.set_icon(Some(icon));
    let _ = tray.set_icon_as_template(healthy);
}

pub fn run() {
    tauri::Builder::default()
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
            set_dialog_open,
        ])
        .setup(|app| {
            // Hide from Dock / App Switcher on macOS.
            set_activation_policy_accessory();

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

            // Shared indexing status — updated from every indexing path
            // (startup loop, background pipeline, manual re-index).
            let indexing_state: Arc<Mutex<IndexingState>> = Arc::new(Mutex::new(IndexingState::default()));

            // Build the result callback once: updates the shared state and
            // emits `indexing-status-changed` so the frontend + tray react.
            // `AppHandle` is `Clone` and cheap to capture.
            let app_handle_for_cb = app.handle().clone();
            let indexing_for_cb = Arc::clone(&indexing_state);
            let on_index_result: syncmind_indexing::IndexResultCallback = Arc::new(
                move |path: &std::path::Path, result: Result<(), &anyhow::Error>| {
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let snapshot = {
                        let mut guard = match indexing_for_cb.lock() {
                            Ok(g) => g,
                            Err(poison) => poison.into_inner(),
                        };
                        match result {
                            Ok(()) => {
                                guard.record_success(timestamp);
                                guard.clear_error_for(path);
                            }
                            Err(e) => {
                                guard.record_error(path.to_path_buf(), e.to_string(), timestamp);
                            }
                        }
                        guard.clone()
                    };
                    let _ = app_handle_for_cb.emit("indexing-status-changed", &snapshot);
                },
            );

            // Index all registered files on startup (using the same callback).
            let extractor = syncmind_rag_engine::extractor::CompositeExtractor::new();
            for path in &config.registered_files {
                let chunker = syncmind_indexing::chunker_for_path(path, config.chunk_size, config.chunk_overlap);
                let result = tauri::async_runtime::block_on(async {
                    syncmind_indexing::index_file(path, &extractor, chunker.as_ref(), embedder.as_ref(), &store).await
                });
                if let Err(e) = &result {
                    error!(path = %path.display(), error = %e, "startup indexing failed");
                }
                on_index_result(path, result.as_ref().map(|_| ()));
            }

            // Start file watcher with 1-second debounce.
            let (file_tx, file_rx) = mpsc::channel::<Vec<std::path::PathBuf>>(256);
            let watcher = tauri::async_runtime::block_on(async {
                syncmind_file_watcher::FileWatcher::new(
                    config.registered_files.clone(),
                    Duration::from_secs(1),
                    file_tx,
                )
            })
            .map_err(|e| {
                error!(error = %e, "file watcher creation failed");
                e.to_string()
            })?;

            // Spawn background indexing pipeline with the same callback.
            let indexing_handle = tauri::async_runtime::spawn(syncmind_indexing::run_indexing_pipeline(
                config.clone(),
                Arc::clone(&store),
                Arc::clone(&embedder),
                file_rx,
                Some(Arc::clone(&on_index_result)),
            ));

            app.manage(AppState {
                config: Mutex::new(config),
                store,
                embedder,
                watcher: Mutex::new(Some(watcher)),
                indexing_handle: Mutex::new(Some(indexing_handle)),
                indexing: Arc::clone(&indexing_state),
                on_index_result: Arc::clone(&on_index_result),
                dialog_open: Mutex::new(false),
            });

            // First-run detection: show window on first launch
            let app_data_dir = app.handle().path().app_data_dir().map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&app_data_dir).map_err(|e| e.to_string())?;
            let first_run_marker = app_data_dir.join("first_run_done");
            let is_first_run = !first_run_marker.exists();

            // Window setup
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("SyncMind")
                .inner_size(860.0, 540.0)
                .resizable(false)
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .center()
                .focused(true)
                .visible(is_first_run)
                .build()?;

            if is_first_run {
                let _ = std::fs::File::create(&first_run_marker);
            }

            // System tray (menu bar)
            let open_palette_item =
                MenuItemBuilder::with_id("open_palette", "Open Palette").build(app)?;
            let indexing_status_item =
                MenuItemBuilder::with_id("indexing_status", "Indexing Status").build(app)?;
            let settings_item =
                MenuItemBuilder::with_id("settings", "Settings").build(app)?;
            let quit_item =
                MenuItemBuilder::with_id("quit", "Quit SyncMind").build(app)?;
            let tray_menu = MenuBuilder::new(app)
                .item(&open_palette_item)
                .item(&indexing_status_item)
                .item(&settings_item)
                .separator()
                .item(&quit_item)
                .build()?;

            let tray = TrayIconBuilder::with_id("main-tray")
                .icon(include_image!("icons/tray.png"))
                .icon_as_template(true)
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open_palette" => {
                        if let Some(window) = app.get_webview_window("main") {
                            show_and_focus(&window);
                        }
                    }
                    "indexing_status" => {
                        if let Some(window) = app.get_webview_window("main") {
                            show_and_focus(&window);
                            let _ = window.emit("tray-navigate", "indexing");
                        }
                    }
                    "settings" => {
                        if let Some(window) = app.get_webview_window("main") {
                            show_and_focus(&window);
                            let _ = window.emit("tray-navigate", "settings");
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            if let Ok(true) = window.is_visible() {
                                let _ = window.hide();
                            } else {
                                show_and_focus(&window);
                            }
                        }
                    }
                })
                .build(app)?;

            // Subscribe to indexing-status-changed and swap the tray icon
            // when health flips. The listener runs on Tauri's event thread.
            let tray_for_listener = tray.clone();
            let indexing_for_listener = Arc::clone(&indexing_state);
            app.listen("indexing-status-changed", move |_event| {
                let healthy = indexing_for_listener
                    .lock()
                    .map(|s| s.is_healthy())
                    .unwrap_or(true);
                apply_tray_health(&tray_for_listener, healthy);
            });

            // Global shortcut: Cmd+Shift+Space (toggle)
            let window_for_shortcut = window.clone();
            app.handle().plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_shortcuts(["Cmd+Shift+Space"])?
                    .with_handler(move |_app, shortcut, event| {
                        if event.state != ShortcutState::Pressed {
                            return;
                        }
                        if shortcut.matches(
                            tauri_plugin_global_shortcut::Modifiers::SUPER
                                | tauri_plugin_global_shortcut::Modifiers::SHIFT,
                            tauri_plugin_global_shortcut::Code::Space,
                        ) {
                            if let Ok(true) = window_for_shortcut.is_visible() {
                                let _ = window_for_shortcut.hide();
                            } else {
                                show_and_focus(&window_for_shortcut);
                            }
                        }
                    })
                    .build(),
            )?;

            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                // Hide instead of close; global shortcut re-opens the palette.
                api.prevent_close();
                let _ = window.hide();
            }
            tauri::WindowEvent::Focused(false) => {
                // Auto-hide when the palette loses focus (click outside,
                // Cmd+Tab to another app, switch Spaces). Esc still works
                // as a fallback via the frontend keydown listener.
                // Suppressed while a modal dialog is open so the file picker
                // isn't dismissed on macOS (hiding the parent window dismisses
                // child dialogs).
                if window.label() == "main" {
                    let skip_hide = if let Some(state) = window.app_handle().try_state::<AppState>() {
                        *state.dialog_open.lock().unwrap_or_else(|poison| poison.into_inner())
                    } else {
                        false
                    };
                    if !skip_hide {
                        let _ = window.hide();
                    }
                }
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
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

                // Intentionally NOT calling prevent_exit() so the process exits cleanly.
                // Window close requests are already handled by on_window_event (hide, not close).
            }
        });
}
