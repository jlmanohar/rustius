use crate::state::AppState;
use rustius_ssh::client::SshSession;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;
use tracing::{debug, error, info};
use uuid::Uuid;

#[tauri::command]
pub async fn ssh_connect(
    tab_id: String,
    connection_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Starting SSH session for tab: {}, conn: {}", tab_id, connection_id);

    let uuid = Uuid::parse_str(&connection_id).map_err(|e| e.to_string())?;
    let connection = state
        .config_manager
        .get_connection(uuid)
        .await
        .map_err(|e| e.to_string())?;

    let (output_tx, mut output_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    let session = SshSession::connect(&connection, output_tx)
        .await
        .map_err(|e| e.to_string())?;
    let session = Arc::new(session);

    {
        let mut sessions = state.ssh_sessions.write().await;
        sessions.insert(tab_id.clone(), session);
    }

    let event_name = format!("terminal-data-{}", tab_id);
    tokio::spawn(async move {
        while let Some(bytes) = output_rx.recv().await {
            if let Err(e) = app.emit(&event_name, bytes) {
                error!("Failed to emit SSH output: {:?}", e);
                break;
            }
        }
        debug!("SSH output loop exited for tab: {}", tab_id);
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
