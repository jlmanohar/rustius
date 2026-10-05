pub mod commands;
pub mod state;

use commands::config::{add_connection, add_folder, get_config, remove_node, save_app_settings};
use commands::sftp::{
    local_get_home_dir, local_list_dir, local_mkdir, local_remove, local_rename,
    sftp_connect, sftp_disconnect, sftp_download, sftp_list_dir, sftp_mkdir, sftp_remove,
    sftp_rename, sftp_upload,
};
use commands::ssh::{ssh_connect, ssh_disconnect, ssh_resize, ssh_write};
use commands::terminal::{pty_close, pty_resize, pty_spawn, pty_write};
use state::AppState;

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
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_app_settings,
            add_connection,
            add_folder,
            remove_node,
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
            sftp_download,
            local_get_home_dir,
            local_list_dir,
            local_mkdir,
            local_remove,
            local_rename
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
