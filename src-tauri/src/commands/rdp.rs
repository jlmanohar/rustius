use crate::state::AppState;
use rustius_core::connection::AuthMethod;
use rustius_rdp::client::{RdpFrame, RdpSession};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;
use tracing::{error, info};

#[tauri::command]
pub async fn rdp_connect(
    app_handle: AppHandle,
    tab_id: String,
    connection_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Starting RDP connection for tab: {} with conn: {}", tab_id, connection_id);

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

use rustius_rdp::freerdp::{FreeRdpConfig, FreeRdpSession};
use tauri::Manager;

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn freerdp_connect(
    app_handle: AppHandle,
    tab_id: String,
    connection_id: String,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Starting FreeRDP native session for tab: {} with conn: {}", tab_id, connection_id);

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
            (username.clone(), password.clone())
        }
        AuthMethod::KeyFile { username, .. } => (username.clone(), None),
        AuthMethod::Agent { username } => (username.clone(), None),
    };

    let domain: Option<String> = None;

    #[cfg(windows)]
    let parent_window = {
        let window = app_handle
            .get_webview_window("main")
            .ok_or("Main application window not found")?;
        window.hwnd().map_err(|e| e.to_string())?.0 as isize
    };

    #[cfg(not(windows))]
    let parent_window = 0isize;

    let config = FreeRdpConfig {
        host,
        port,
        username,
        password,
        domain,
        parent_window,
        x,
        y,
        width,
        height,
    };

    let session = FreeRdpSession::connect(config)
        .await
        .map_err(|e| e.to_string())?;

    let session = Arc::new(session);

    let old_session = {
        let mut map = state.freerdp_sessions.write().await;
        map.insert(tab_id.clone(), session.clone())
    };
    if let Some(old) = old_session {
        old.disconnect();
    }

    let exit_event = format!("rdp-exit-{}", tab_id);
    let app_clone = app_handle.clone();
    let tab_id_clone = tab_id.clone();
    let session_clone = session.clone();

    tokio::spawn(async move {
        while session_clone.is_alive() {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            match session_clone.try_wait_child() {
                Ok(Some(status)) => {
                    info!("FreeRDP session exited with status {:?} for tab: {}", status, tab_id_clone);
                    let code = status.code().unwrap_or(0);
                    let _ = app_clone.emit(&exit_event, code);
                    break;
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("Error checking FreeRDP child status: {:?}", e);
                    break;
                }
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn freerdp_update_bounds(
    tab_id: String,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let sess = {
        let map = state.freerdp_sessions.read().await;
        map.get(&tab_id).cloned()
    };
    if let Some(s) = sess {
        s.update_bounds(x, y, width, height)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn freerdp_set_visibility(
    tab_id: String,
    visible: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let sess = {
        let map = state.freerdp_sessions.read().await;
        map.get(&tab_id).cloned()
    };
    if let Some(s) = sess {
        s.set_visibility(visible)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn freerdp_disconnect(
    tab_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let sess = {
        let mut map = state.freerdp_sessions.write().await;
        map.remove(&tab_id)
    };
    if let Some(s) = sess {
        s.disconnect();
        info!("Closed FreeRDP native session for tab: {}", tab_id);
    }
    Ok(())
}

