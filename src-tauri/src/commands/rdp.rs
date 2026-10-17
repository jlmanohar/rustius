use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State};
use tracing::info;

use rustius_core::connection::AuthMethod;
use rustius_rdp::{FreeRdpConfig, FreeRdpSession, IronRdpConfig, IronRdpEvent, IronRdpSession, MouseButton};

use crate::state::AppState;



/// Establish a native FreeRDP session embedded into the application window.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn freerdp_connect(
    app_handle: AppHandle,
    tab_id: String,
    connection_id: String,
    credentials_override: Option<rustius_core::connection::CredentialOverride>,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    desktop_width: Option<u32>,
    desktop_height: Option<u32>,
    scale_desktop: Option<u32>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!(
        "Starting FreeRDP native session for tab: {} with conn: {}",
        tab_id, connection_id
    );

    let uuid = rustius_core::connection::parse_uuid_or_fallback(&connection_id);
    let conn = state
        .config_manager
        .get_connection(uuid)
        .await
        .map_err(|e| format!("Connection not found (ID: {connection_id}): {e}"))?;
    let mut conn = state
        .config_manager
        .resolve_connection(&conn)
        .await
        .map_err(|e| format!("Failed to resolve credentials: {e}"))?;

    conn.apply_override(credentials_override);

    let host = conn.host;
    let port = conn.port;

    let (mut username, password) = match &conn.auth {
        AuthMethod::Password {
            username, password, ..
        } => (username.clone(), password.clone()),
        AuthMethod::KeyFile { username, .. } => (username.clone(), None),
        AuthMethod::Agent { username } => (username.clone(), None),
    };

    let mut domain = conn
        .rdp_config
        .as_ref()
        .and_then(|r| r.domain.as_ref())
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty());

    // Support standard Windows down-level logon format "DOMAIN\username"
    if username.contains('\\') {
        let parts: Vec<&str> = username.splitn(2, '\\').collect();
        if parts.len() == 2 {
            if domain.is_none() && !parts[0].trim().is_empty() {
                domain = Some(parts[0].trim().to_string());
            }
            username = parts[1].trim().to_string();
        }
    }

    info!(
        "FreeRDP credentials prepared - host: {}:{}, user: {}, domain: {:?}",
        host, port, username, domain
    );

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
        desktop_width,
        desktop_height,
        scale_desktop: scale_desktop
            .filter(|&s| s >= 100)
            .or_else(|| conn.rdp_config.as_ref().and_then(|r| r.scale_factor).filter(|&s| s >= 100)),
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
                    let code = status.code().unwrap_or(0);
                    let diag = session_clone.get_exit_diagnostic(code);
                    if code == 0 {
                        info!(
                            "FreeRDP session ended normally for tab: {}",
                            tab_id_clone
                        );
                    } else {
                        tracing::error!(
                            target: "rustius_rdp",
                            "FreeRDP exited for tab {}: [{}] {} (code: {}, raw: {:?})",
                            tab_id_clone,
                            diag.title,
                            diag.message,
                            code,
                            diag.raw_error
                        );
                    }
                    let _ = app_clone.emit(&exit_event, &diag);
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
        s.set_visibility(visible).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Terminate the active FreeRDP session for the specified tab.
#[tauri::command]
pub async fn freerdp_disconnect(tab_id: String, state: State<'_, AppState>) -> Result<(), String> {
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

/// Toggle between embedded docked mode and detached native window mode.
#[tauri::command]
pub async fn freerdp_set_docked(
    tab_id: String,
    docked: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let sess = {
        let map = state.freerdp_sessions.read().await;
        map.get(&tab_id).cloned()
    };
    if let Some(s) = sess {
        s.set_docked(docked).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Establish a cross-platform IronRDP session with software decode and canvas/WebGL streaming.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn ironrdp_connect(
    app_handle: AppHandle,
    tab_id: String,
    connection_id: String,
    credentials_override: Option<rustius_core::connection::CredentialOverride>,
    width: u16,
    height: u16,
    scale_factor: Option<u32>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!(
        "Starting IronRDP session for tab: {} with conn: {}",
        tab_id, connection_id
    );

    let uuid = rustius_core::connection::parse_uuid_or_fallback(&connection_id);
    let conn = state
        .config_manager
        .get_connection(uuid)
        .await
        .map_err(|e| format!("Connection not found (ID: {connection_id}): {e}"))?;
    let mut conn = state
        .config_manager
        .resolve_connection(&conn)
        .await
        .map_err(|e| format!("Failed to resolve credentials: {e}"))?;

    conn.apply_override(credentials_override);

    let host = conn.host;
    let port = conn.port;

    let (mut username, password) = match &conn.auth {
        AuthMethod::Password {
            username, password, ..
        } => (username.clone(), password.clone()),
        AuthMethod::KeyFile { username, .. } => (username.clone(), None),
        AuthMethod::Agent { username } => (username.clone(), None),
    };

    let mut domain = conn
        .rdp_config
        .as_ref()
        .and_then(|r| r.domain.as_ref())
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty());

    if username.contains('\\') {
        let parts: Vec<&str> = username.splitn(2, '\\').collect();
        if parts.len() == 2 {
            if domain.is_none() && !parts[0].trim().is_empty() {
                domain = Some(parts[0].trim().to_string());
            }
            username = parts[1].trim().to_string();
        }
    }

    let scale = scale_factor
        .filter(|&s| s >= 100)
        .or_else(|| conn.rdp_config.as_ref().and_then(|r| r.scale_factor).filter(|&s| s >= 100));

    let egfx = conn.rdp_config.as_ref().and_then(|r| r.egfx);

    let rdp_config = IronRdpConfig {
        host,
        port,
        username,
        password,
        domain,
        width,
        height,
        scale_factor: scale,
        egfx,
    };

    let app_clone = app_handle.clone();
    let tab_id_clone = tab_id.clone();

    let session = IronRdpSession::connect(rdp_config, move |event| {
        match event {
            IronRdpEvent::Connected => {
                let _ = app_clone.emit(&format!("rdp-connected-{}", tab_id_clone), ());
            }
            IronRdpEvent::Frame(frame) => {
                let _ = app_clone.emit(&format!("rdp-frame-{}", tab_id_clone), frame);
            }
            IronRdpEvent::Terminated(diag) => {
                let _ = app_clone.emit(&format!("rdp-exit-{}", tab_id_clone), diag);
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    let session = Arc::new(session);

    let old_session = {
        let mut map = state.ironrdp_sessions.write().await;
        map.insert(tab_id.clone(), session)
    };
    if let Some(old) = old_session {
        old.disconnect();
    }

    Ok(())
}

/// Send mouse position to the active IronRDP session.
#[tauri::command]
pub async fn ironrdp_mouse_move(
    tab_id: String,
    x: u16,
    y: u16,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let sess = {
        let map = state.ironrdp_sessions.read().await;
        map.get(&tab_id).cloned()
    };
    if let Some(s) = sess {
        s.mouse_move(x, y);
    }
    Ok(())
}

/// Send mouse button press/release to the active IronRDP session.
#[tauri::command]
pub async fn ironrdp_mouse_button(
    tab_id: String,
    button: u8,
    pressed: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mouse_btn = match button {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        3 => MouseButton::X1,
        4 => MouseButton::X2,
        _ => MouseButton::Left,
    };

    let sess = {
        let map = state.ironrdp_sessions.read().await;
        map.get(&tab_id).cloned()
    };
    if let Some(s) = sess {
        s.mouse_button(mouse_btn, pressed);
    }
    Ok(())
}

/// Send mouse wheel scrolling to the active IronRDP session.
#[tauri::command]
pub async fn ironrdp_mouse_wheel(
    tab_id: String,
    is_vertical: bool,
    rotation_units: i16,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let sess = {
        let map = state.ironrdp_sessions.read().await;
        map.get(&tab_id).cloned()
    };
    if let Some(s) = sess {
        s.mouse_wheel(is_vertical, rotation_units);
    }
    Ok(())
}

/// Forward keyboard event (using web KeyboardEvent.code) to the active IronRDP session.
#[tauri::command]
pub async fn ironrdp_key_event(
    tab_id: String,
    code: String,
    pressed: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let sess = {
        let map = state.ironrdp_sessions.read().await;
        map.get(&tab_id).cloned()
    };
    if let Some(s) = sess {
        s.key_event(&code, pressed);
    }
    Ok(())
}

/// Dynamically resize the desktop in the active IronRDP session.
#[tauri::command]
pub async fn ironrdp_resize(
    tab_id: String,
    width: u16,
    height: u16,
    scale_factor: Option<u32>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let sess = {
        let map = state.ironrdp_sessions.read().await;
        map.get(&tab_id).cloned()
    };
    if let Some(s) = sess {
        s.resize(width, height, scale_factor.unwrap_or(100));
    }
    Ok(())
}

/// Terminate the active IronRDP session for the specified tab.
#[tauri::command]
pub async fn ironrdp_disconnect(
    tab_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let sess = {
        let mut map = state.ironrdp_sessions.write().await;
        map.remove(&tab_id)
    };
    if let Some(s) = sess {
        s.disconnect();
        info!("Closed IronRDP session for tab: {}", tab_id);
    }
    Ok(())
}

/// Terminate an RDP session for a given tab (disconnects both IronRDP and FreeRDP if active).
#[tauri::command]
pub async fn rdp_disconnect(tab_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let _ = ironrdp_disconnect(tab_id.clone(), state.clone()).await;
    let _ = freerdp_disconnect(tab_id, state).await;
    Ok(())
}

