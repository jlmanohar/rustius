use crate::state::AppState;
use rustius_ssh::client::SshSession;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

#[tauri::command]
pub async fn ssh_connect(
    tab_id: String,
    connection_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Starting SSH session for tab: {}, conn: {}", tab_id, connection_id);

    let uuid = rustius_core::connection::parse_uuid_or_fallback(&connection_id);
    let connection = state
        .config_manager
        .get_connection(uuid)
        .await
        .map_err(|e| format!("Connection not found (ID: {connection_id}): {e}"))?;

    let (output_tx, mut output_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (exit_tx, mut exit_rx) = mpsc::channel::<u32>(1);

    let session = SshSession::connect(&connection, output_tx, exit_tx)
        .await
        .map_err(|e| e.to_string())?;
    let session = Arc::new(session);

    {
        let mut sessions = state.ssh_sessions.write().await;
        if let Some(old) = sessions.remove(&tab_id) {
            info!("Disconnecting previous SSH session for tab: {}", tab_id);
            let _ = old.disconnect().await;
        }
        sessions.insert(tab_id.clone(), session);
    }

    let data_event = format!("terminal-data-{}", tab_id);
    let exit_event = format!("terminal-exit-{}", tab_id);
    let app_handle = app.clone();
    let tab_id_clone = tab_id.clone();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(bytes) = output_rx.recv() => {
                    if let Err(e) = app_handle.emit(&data_event, bytes) {
                        error!("Failed to emit SSH output: {:?}", e);
                        break;
                    }
                }
                Some(code) = exit_rx.recv() => {
                    // Drain any remaining buffered output bytes from the SSH session
                    while let Ok(bytes) = output_rx.try_recv() {
                        let _ = app_handle.emit(&data_event, bytes);
                    }
                    info!("SSH session exited with code: {} for tab: {}", code, tab_id_clone);
                    let _ = app_handle.emit(&exit_event, code);
                    break;
                }
                else => {
                    info!("SSH session closed for tab: {}", tab_id_clone);
                    let _ = app_handle.emit(&exit_event, 0u32);
                    break;
                }
            }
        }
        debug!("SSH output loop exited for tab: {}", tab_id_clone);
    });

    Ok(())
}

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
        s.write(&data).await.map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err(format!("SSH session not found for tab: {}", tab_id))
    }
}

#[tauri::command]
pub async fn ssh_resize(
    tab_id: String,
    cols: u32,
    rows: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
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

#[tauri::command]
pub async fn ssh_disconnect(tab_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let session = {
        let mut sessions = state.ssh_sessions.write().await;
        sessions.remove(&tab_id)
    };

    if let Some(s) = session {
        let _ = s.disconnect().await;
    }
    info!("SSH session closed for tab: {}", tab_id);
    Ok(())
}
