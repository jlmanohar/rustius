pub mod commands;
pub mod state;

use commands::config::{
    add_connection, add_credential, add_folder, delete_credential, get_config, import_sessions,
    import_sessions_content, remove_node, save_app_settings, update_credential,
};
use commands::rdp::{
    freerdp_connect, freerdp_disconnect, freerdp_set_docked, freerdp_set_visibility,
    freerdp_update_bounds, rdp_disconnect,
};
use commands::sftp::{
    local_get_home_dir, local_list_dir, local_mkdir, local_remove, local_rename, sftp_connect,
    sftp_disconnect, sftp_download, sftp_download_bytes, sftp_download_to_downloads, sftp_list_dir,
    sftp_mkdir, sftp_remove, sftp_rename, sftp_upload, sftp_upload_bytes,
};
use commands::ssh::{ssh_connect, ssh_disconnect, ssh_resize, ssh_write};
use commands::terminal::{pty_close, pty_resize, pty_spawn, pty_write};
use commands::window::{window_close, window_is_maximized, window_minimize, window_toggle_maximize};
use state::AppState;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,rustius=debug")),
        )
        .init();

    let app_state = match AppState::new() {
        Ok(state) => state,
        Err(err) => {
            eprintln!("Failed to initialize app state: {err}");
            panic!("Cannot start Rustius without valid state");
        }
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)) {
                let _ = window.emit("window-moved", ());
                let state = window.state::<AppState>();
                let sessions: Vec<_> = {
                    if let Ok(guard) = state.freerdp_sessions.try_read() {
                        guard.values().cloned().collect()
                    } else {
                        Vec::new()
                    }
                };
                for session in sessions {
                    session.sync_screen_position();
                }
            } else if matches!(event, tauri::WindowEvent::Destroyed) {
                let state = window.state::<AppState>();
                let freerdp = state.freerdp_sessions.clone();
                let pty = state.local_terminals.clone();
                if let Ok(mut guard) = freerdp.try_write() {
                    for (_, session) in guard.drain() {
                        session.disconnect();
                    }
                };
                if let Ok(mut guard) = pty.try_write() {
                    for (_, session) in guard.drain() {
                        session.close();
                    }
                };
            }
        })
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_app_settings,
            add_connection,
            add_folder,
            remove_node,
            add_credential,
            update_credential,
            delete_credential,
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
            sftp_download_bytes,
            local_get_home_dir,
            local_list_dir,
            local_mkdir,
            local_remove,
            local_rename,
            rdp_disconnect,
            freerdp_connect,
            freerdp_disconnect,
            freerdp_set_docked,
            freerdp_update_bounds,
            freerdp_set_visibility,
            window_minimize,
            window_toggle_maximize,
            window_close,
            window_is_maximized
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
