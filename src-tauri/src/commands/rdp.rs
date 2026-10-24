use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tracing::info;

use rustius_core::connection::AuthMethod;
use rustius_rdp::{IronRdpConfig, IronRdpEvent, IronRdpSession, MouseButton};

use crate::state::AppState;

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
    on_frame: tauri::ipc::Channel<tauri::ipc::Response>,
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

    let app_settings = state.config_manager.get_config().await.app_settings;

    let scale = scale_factor.filter(|&s| s >= 100).or_else(|| {
        conn.rdp_config
            .as_ref()
            .and_then(|r| r.scale_factor)
            .filter(|&s| s >= 100)
    }).or_else(|| {
        Some(app_settings.default_rdp_scale_factor).filter(|&s| s >= 100)
    });

    let egfx = conn
        .rdp_config
        .as_ref()
        .and_then(|r| r.egfx)
        .or(Some(app_settings.default_rdp_egfx));
    let clipboard_sharing = conn
        .rdp_config
        .as_ref()
        .map(|r| r.clipboard_sharing)
        .or(Some(app_settings.default_rdp_clipboard_sharing));

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
        clipboard_sharing,
        known_hosts_path: Some(state.rdp_known_hosts_path()),
    };

    let app_clone = app_handle.clone();
    let tab_id_clone = tab_id.clone();
    let on_frame_clone = on_frame.clone();

    let session = IronRdpSession::connect(rdp_config, move |event| match event {
        IronRdpEvent::Connected => {
            let _ = app_clone.emit(&format!("rdp-connected-{}", tab_id_clone), ());
        }
        IronRdpEvent::BinaryFrameBatch(bytes) => {
            let _ = on_frame_clone.send(tauri::ipc::Response::new(bytes));
        }
        IronRdpEvent::PointerDefault => {
            let _ = app_clone.emit(&format!("rdp-pointer-default-{}", tab_id_clone), ());
        }
        IronRdpEvent::PointerHidden => {
            let _ = app_clone.emit(&format!("rdp-pointer-hidden-{}", tab_id_clone), ());
        }
        IronRdpEvent::PointerPosition { x, y } => {
            let _ = app_clone.emit(&format!("rdp-pointer-position-{}", tab_id_clone), (x, y));
        }
        IronRdpEvent::PointerBitmap(bitmap) => {
            let _ = app_clone.emit(&format!("rdp-pointer-bitmap-{}", tab_id_clone), bitmap);
        }
        IronRdpEvent::Terminated(diag) => {
            let _ = app_clone.emit(&format!("rdp-exit-{}", tab_id_clone), diag);
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
    x: Option<u16>,
    y: Option<u16>,
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
        let coords = match (x, y) {
            (Some(x), Some(y)) => Some((x, y)),
            _ => None,
        };
        s.mouse_button(mouse_btn, pressed, coords);
    }
    Ok(())
}

/// Release all held keys and mouse buttons (e.g. on window blur or visibility loss).
#[tauri::command]
pub async fn ironrdp_release_all(
    tab_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let sess = {
        let map = state.ironrdp_sessions.read().await;
        map.get(&tab_id).cloned()
    };
    if let Some(s) = sess {
        s.release_all();
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
        s.resize(width, height, scale_factor.unwrap_or(100))?;
        Ok(())
    } else {
        Err(format!("No active IronRDP session found for tab {}", tab_id))
    }
}

/// Terminate the active IronRDP session for the specified tab.
#[tauri::command]
pub async fn ironrdp_disconnect(tab_id: String, state: State<'_, AppState>) -> Result<(), String> {
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

/// Terminate an RDP session for a given tab.
#[tauri::command]
pub async fn rdp_disconnect(tab_id: String, state: State<'_, AppState>) -> Result<(), String> {
    ironrdp_disconnect(tab_id, state).await
}
