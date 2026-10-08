use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;
use tracing::{error, info};

use rustius_core::connection::AuthMethod;
use rustius_rdp::client::{RdpFrame, RdpSession};

use crate::state::AppState;

/// Establish a Remote Desktop Protocol (RDP) session for a specific tab.
///
/// Connects to the remote desktop server, begins receiving video frames,
/// and streams them to the frontend via `rdp-frame-{tab_id}` events.
#[tauri::command]
pub async fn rdp_connect(
    app_handle: AppHandle,
    tab_id: String,
    connection_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!(
        "Starting RDP connection for tab: {} with conn: {}",
        tab_id, connection_id
    );

    // Retrieve connection from config manager
    let uuid = rustius_core::connection::parse_uuid_or_fallback(&connection_id);
    let conn = state
        .config_manager
        .get_connection(uuid)
        .await
        .map_err(|e| format!("Connection not found (ID: {connection_id}): {e}"))?;

    let host = conn.host;
    let port = conn.port;

    let (username, password) = match &conn.auth {
        AuthMethod::Password { username, password } => {
            (username.clone(), password.clone().unwrap_or_default())
        }
        AuthMethod::KeyFile { username, .. } => (username.clone(), String::new()),
        AuthMethod::Agent { username } => (username.clone(), String::new()),
    };

    let (width, height) = if let Some(rdp_cfg) = &conn.rdp_config {
        (rdp_cfg.width, rdp_cfg.height)
    } else {
        (1280, 800)
    };

    let (frame_tx, mut frame_rx) = mpsc::unbounded_channel::<RdpFrame>();

    let session = RdpSession::connect(&host, port, &username, &password, width, height, frame_tx)
        .await
        .map_err(|e| e.to_string())?;

    let session = Arc::new(session);

    // Store in AppState, closing any previous session for this tab
    let old_session = {
        let mut rdp_map = state.rdp_sessions.write().await;
        rdp_map.insert(tab_id.clone(), session.clone())
    };
    if let Some(old) = old_session {
        old.disconnect();
    }

    let frame_event = format!("rdp-frame-{}", tab_id);
    let exit_event = format!("rdp-exit-{}", tab_id);
    let tab_id_clone = tab_id.clone();
    let app_clone = app_handle.clone();

    // Stream frames to frontend
    tokio::spawn(async move {
        while let Some(frame) = frame_rx.recv().await {
            if let Err(e) = app_clone.emit(&frame_event, frame) {
                error!("Failed to emit RDP frame event: {:?}", e);
                break;
            }
        }
        info!("RDP frame forwarding exited for tab: {}", tab_id_clone);
        let _ = app_clone.emit(&exit_event, 0u32);
    });

    Ok(())
}

/// Terminate the active RDP session for the specified tab.
#[tauri::command]
pub async fn rdp_disconnect(tab_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let session = {
        let mut rdp_map = state.rdp_sessions.write().await;
        rdp_map.remove(&tab_id)
    };

    if let Some(sess) = session {
        sess.disconnect();
        info!("Closed RDP session for tab: {}", tab_id);
    }

    Ok(())
}

/// Forward keyboard scan codes and press states to the remote RDP session.
#[tauri::command]
pub async fn rdp_key_event(
    tab_id: String,
    key_code: u16,
    pressed: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let session = {
        let rdp_map = state.rdp_sessions.read().await;
        rdp_map.get(&tab_id).cloned()
    };

    if let Some(sess) = session {
        sess.send_key_event(key_code, pressed)
            .map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err(format!("RDP session not found for tab: {}", tab_id))
    }
}

/// Forward mouse coordinates, button clicks, and scroll wheel events to the remote RDP session.
#[tauri::command]
pub async fn rdp_mouse_event(
    tab_id: String,
    x: u32,
    y: u32,
    buttons: u8,
    wheel: i8,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let session = {
        let rdp_map = state.rdp_sessions.read().await;
        rdp_map.get(&tab_id).cloned()
    };

    if let Some(sess) = session {
        sess.send_mouse_event(x, y, buttons, wheel)
            .map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err(format!("RDP session not found for tab: {}", tab_id))
    }
}
