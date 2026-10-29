use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;
use tracing::{debug, info};

use rustius_ssh::client::SshSession;

use crate::state::AppState;

/// Initialize a new SSH terminal session for a specific tab.
///
/// 1. Retrieves connection configuration and credentials by UUID.
/// 2. Spawns an SSH client session with pseudo-terminal allocation (`xterm-256color`).
/// 3. Replaces any prior active session for the same `tab_id` cleanly.
/// 4. Spawns an asynchronous background worker task that consumes incoming remote
///    pty output bytes and emits them to the frontend via `terminal-data-{tab_id}` events.
/// 5. Drains any residual output upon receiving session exit status before signaling
///    `terminal-exit-{tab_id}` to avoid cutting off last-second output (e.g., prompt or exit message).
#[tauri::command]
pub async fn ssh_connect(
    tab_id: String,
    connection_id: String,
    credentials_override: Option<rustius_core::connection::CredentialOverride>,
    on_data: tauri::ipc::Channel<tauri::ipc::Response>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!(
        "Starting SSH session for tab: {}, conn: {}",
        tab_id, connection_id
    );

    let uuid = rustius_core::connection::parse_uuid_or_fallback(&connection_id);
    let connection = state
        .config_manager
        .get_connection(uuid)
        .await
        .map_err(|e| format!("Connection not found (ID: {connection_id}): {e}"))?;
    let mut connection = state
        .config_manager
        .resolve_connection(&connection)
        .await
        .map_err(|e| format!("Failed to resolve credentials: {e}"))?;

    connection.apply_override(credentials_override);

    let (output_tx, mut output_rx) = mpsc::channel::<Vec<u8>>(128);
    let (exit_tx, mut exit_rx) = mpsc::channel::<u32>(1);

    let known_hosts_path = state.known_hosts_path();
    let session = SshSession::connect(&connection, known_hosts_path, output_tx, exit_tx)
        .await
        .map_err(|e| e.to_string())?;
    let session = Arc::new(session);

    let old_session = {
        let mut sessions = state.ssh_sessions.write().await;
        let old = sessions.remove(&tab_id);
        sessions.insert(tab_id.clone(), session.clone());
        old
    };
    if let Some(old) = old_session {
        info!("Disconnecting previous SSH session for tab: {}", tab_id);
        let _ = old.disconnect().await;
    }

    let exit_event = format!("terminal-exit-{}", tab_id);
    let app_handle = app.clone();
    let tab_id_clone = tab_id.clone();
    let sessions_map = state.ssh_sessions.clone();
    let sftp_sessions_map = state.sftp_sessions.clone();
    let session_for_cleanup = session.clone();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(bytes) = output_rx.recv() => {
                    let _ = on_data.send(tauri::ipc::Response::new(bytes));
                }
                Some(code) = exit_rx.recv() => {
                    // Drain any remaining buffered output bytes from the SSH session
                    while let Ok(bytes) = output_rx.try_recv() {
                        let _ = on_data.send(tauri::ipc::Response::new(bytes));
                    }
                    info!("SSH session exited with code: {} for tab: {}", code, tab_id_clone);
                    let _ = app_handle.emit(&exit_event, code);
                    break;
                }
                else => {
                    info!("SSH session closed for tab: {}", tab_id_clone);
                    let _ = app_handle.emit(&exit_event, 255u32);
                    break;
                }
            }
        }
        // Evict session from AppState and disconnect underlying transport cleanly
        let ssh_was_current = {
            let mut sessions = sessions_map.write().await;
            if let Some(s) = sessions.get(&tab_id_clone) {
                if Arc::ptr_eq(s, &session_for_cleanup) {
                    let removed = sessions.remove(&tab_id_clone);
                    drop(sessions); // Release lock before disconnect
                    if let Some(r) = removed {
                        let _ = r.disconnect().await;
                    }
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        // Evict and disconnect any associated SFTP session for this tab only if this SSH session was still the active session
        if ssh_was_current {
            let sftp = {
                let mut sftp_sessions = sftp_sessions_map.write().await;
                sftp_sessions.remove(&tab_id_clone)
            };
            if let Some(sftp) = sftp {
                let _ = sftp.disconnect().await;
            }
        }
        debug!("SSH output loop exited for tab: {}", tab_id_clone);
    });

    Ok(())
}

/// Send keystrokes and raw user input bytes into the remote SSH session channel.
///
/// Minimizes lock hold time by cloning the session [`Arc`] out of the session map
/// before awaiting asynchronous write operations.
#[tauri::command]
pub async fn ssh_write(
    tab_id: String,
    data: Vec<u8>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let session = {
        let sessions = state.ssh_sessions.read().await;
        sessions.get(&tab_id).cloned()
    };

    if let Some(s) = session {
        if let Err(e) = s.write(&data).await {
            // Write failed because connection or channel dropped midway. Evict from state.
            let mut sessions = state.ssh_sessions.write().await;
            sessions.remove(&tab_id);
            return Err(e.to_string());
        }
        Ok(())
    } else {
        Err(format!("SSH session not found for tab: {}", tab_id))
    }
}

/// Request terminal window resize on the remote SSH pseudo-terminal (PTY).
///
/// Sends an SSH channel window change request with updated column and row dimensions
/// so remote applications (vim, htop, bash) adapt layout accordingly.
#[tauri::command]
pub async fn ssh_resize(
    tab_id: String,
    cols: u32,
    rows: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if cols < 10 || rows < 2 {
        return Ok(());
    }

    let session = {
        let sessions = state.ssh_sessions.read().await;
        sessions.get(&tab_id).cloned()
    };

    if let Some(s) = session {
        s.resize(cols, rows).await.map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err(format!("SSH session not found for tab: {}", tab_id))
    }
}

/// Gracefully terminate and remove the SSH session for the given tab ID.
#[tauri::command]
pub async fn ssh_disconnect(tab_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let session = {
        let mut sessions = state.ssh_sessions.write().await;
        sessions.remove(&tab_id)
    };

    if let Some(s) = session {
        let _ = s.disconnect().await;
    }

    // Also clean up any associated SFTP session for this tab
    let sftp_session = {
        let mut sftp_sessions = state.sftp_sessions.write().await;
        sftp_sessions.remove(&tab_id)
    };

    if let Some(s) = sftp_session {
        let _ = s.disconnect().await;
    }

    info!("SSH session closed for tab: {}", tab_id);
    Ok(())
}
