pub mod commands;
pub mod state;

use commands::config::{
    add_connection, add_credential, add_folder, clear_recovery_notice, delete_credential,
    get_config, get_connection_password, get_credential_password, import_sessions,
    import_sessions_content, move_node, remove_node, save_app_settings, update_credential,
    vault_change_backend, vault_get_status, vault_lock, vault_set_master_password, vault_unlock,
};
use commands::rdp::{
    ironrdp_connect, ironrdp_disconnect, ironrdp_key_event, ironrdp_mouse_button,
    ironrdp_mouse_move, ironrdp_mouse_wheel, ironrdp_release_all, ironrdp_resize, rdp_disconnect,
};

use commands::sftp::{
    local_get_home_dir, local_list_dir, local_mkdir, local_remove, local_rename, sftp_connect,
    sftp_disconnect, sftp_download, sftp_download_to_downloads, sftp_list_dir, sftp_mkdir,
    sftp_remove, sftp_rename, sftp_upload, sftp_upload_bytes,
};
use commands::ssh::{ssh_connect, ssh_disconnect, ssh_resize, ssh_write};
use commands::terminal::{pty_close, pty_resize, pty_spawn, pty_write};
use commands::window::{
    window_close, window_is_maximized, window_minimize, window_toggle_maximize,
};
use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize tracing to both disk log file and stdout
    let log_dir = std::env::temp_dir().join("rustius");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("rustius.log"));

    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new("info,rustius=debug,ironrdp=debug,ironrdp_cliprdr=debug")
    });

    if let Ok(file) = log_file {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::sync::Mutex::new(file))
            .with_ansi(false)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }

    let (data_dir, is_portable) = state::resolve_data_dir();
    let app_state = match AppState::new() {
        Ok(state) => state,
        Err(err) => {
            tracing::error!("Failed to initialize primary app state: {err}. Attempting fallback...");
            let fallback_dir = std::env::temp_dir().join("rustius-recovery");
            let _ = std::fs::create_dir_all(&fallback_dir);
            let storage = Box::new(rustius_core::storage::PlaintextStorage);
            match rustius_core::config::ConfigManager::new_with_portable(fallback_dir.clone(), is_portable, storage) {
                Ok(cm) => {
                    tracing::warn!("Initialized Rustius in safe-mode recovery state.");
                    let notice = format!(
                        "Warning: Failed to load configuration from {:?}: {err}. Rustius started in temporary recovery mode.",
                        data_dir
                    );
                    tauri::async_runtime::block_on(async {
                        cm.set_corruption_notice(notice).await;
                    });
                    AppState {
                        config_manager: std::sync::Arc::new(cm),
                        local_terminals: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
                        ssh_sessions: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
                        sftp_sessions: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
                        ironrdp_sessions: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
                        data_dir: fallback_dir,
                        is_portable,
                    }
                }
                Err(fatal) => {
                    tracing::error!("Fatal: cannot initialize fallback state: {fatal}");
                    eprintln!("Fatal error starting Rustius: {fatal}");
                    return;
                }
            }
        }
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(Some(monitor)) = window.current_monitor() {
                    let size = monitor.size();
                    let scale = monitor.scale_factor();
                    if scale > 0.0 {
                        let logical_w = size.width as f64 / scale;
                        let logical_h = size.height as f64 / scale;

                        // Dynamically scale window to ~88% width and ~85% height of the user's active display
                        let target_w = (logical_w * 0.88).clamp(840.0, 1600.0);
                        let target_h = (logical_h * 0.85).clamp(500.0, 1000.0);

                        let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
                            target_w, target_h,
                        )));
                        let _ = window.center();
                    }
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                let state = window.state::<AppState>();
                let ironrdp = state.ironrdp_sessions.clone();
                let pty = state.local_terminals.clone();
                let ssh = state.ssh_sessions.clone();
                let sftp = state.sftp_sessions.clone();
                // Spawn async cleanup task to properly acquire locks and drain all session maps
                tauri::async_runtime::spawn(async move {
                    // Drain IronRDP sessions
                    {
                        let mut guard = ironrdp.write().await;
                        for (_, session) in guard.drain() {
                            session.disconnect();
                        }
                    }
                    // Drain local PTY sessions
                    {
                        let mut guard = pty.write().await;
                        for (_, session) in guard.drain() {
                            session.close();
                        }
                    }
                    // Drain SSH sessions
                    {
                        let mut guard = ssh.write().await;
                        for (_, session) in guard.drain() {
                            let _ = session.disconnect().await;
                        }
                    }
                    // Drain SFTP sessions
                    {
                        let mut guard = sftp.write().await;
                        for (_, session) in guard.drain() {
                            let _ = session.disconnect().await;
                        }
                    }
                    tracing::info!("All sessions cleaned up on window destroy");
                });
            }
        })
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_config,
            clear_recovery_notice,
            save_app_settings,
            add_connection,
            add_folder,
            remove_node,
            move_node,
            add_credential,
            update_credential,
            delete_credential,
            get_connection_password,
            get_credential_password,
            vault_get_status,
            vault_unlock,
            vault_lock,
            vault_set_master_password,
            vault_change_backend,
            import_sessions,
            import_sessions_content,
            pty_spawn,
            pty_write,
            pty_resize,
            pty_close,
            ssh_connect,
            ssh_write,
            ssh_resize,
            ssh_disconnect,
            sftp_connect,
            sftp_disconnect,
            sftp_list_dir,
            sftp_mkdir,
            sftp_remove,
            sftp_rename,
            sftp_upload,
            sftp_upload_bytes,
            sftp_download,
            sftp_download_to_downloads,
            local_get_home_dir,
            local_list_dir,
            local_mkdir,
            local_remove,
            local_rename,
            rdp_disconnect,
            ironrdp_connect,
            ironrdp_disconnect,
            ironrdp_mouse_move,
            ironrdp_mouse_button,
            ironrdp_mouse_wheel,
            ironrdp_key_event,
            ironrdp_release_all,
            ironrdp_resize,
            window_minimize,
            window_toggle_maximize,
            window_close,
            window_is_maximized
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
