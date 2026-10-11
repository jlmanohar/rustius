use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State};
use tracing::info;

use rustius_core::connection::AuthMethod;
use rustius_rdp::{FreeRdpConfig, FreeRdpSession};

use crate::state::AppState;

/// Establish a native FreeRDP session embedded into the application window.
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
    let conn = state
        .config_manager
        .resolve_connection(&conn)
        .map_err(|e| format!("Failed to resolve credentials: {e}"))?;

    let host = conn.host;
    let port = conn.port;

    let (username, password) = match &conn.auth {
        AuthMethod::Password { username, password, .. } => {
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
    let tab_id_clone = tab_id.clone();
    let app_clone = app_handle.clone();
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

/// Update position and dimensions of the embedded FreeRDP child window.
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

/// Toggle visibility of the embedded FreeRDP child window when switching tabs.
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

/// Terminate the active FreeRDP session for the specified tab.
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

/// Alias for terminating an RDP session for a given tab.
#[tauri::command]
pub async fn rdp_disconnect(
    tab_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    freerdp_disconnect(tab_id, state).await
}
