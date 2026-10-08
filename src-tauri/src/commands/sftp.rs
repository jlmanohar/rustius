use crate::state::AppState;
use rustius_ssh::sftp::{FileEntry, SftpClient};
use std::path::Path;
use std::sync::Arc;
use tauri::State;
use tracing::info;

#[tauri::command]
pub async fn sftp_connect(
    tab_id: String,
    connection_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    info!("Starting SFTP session for tab: {}, conn: {}", tab_id, connection_id);

    let uuid = rustius_core::connection::parse_uuid_or_fallback(&connection_id);
    let connection = state
        .config_manager
        .get_connection(uuid)
        .await
        .map_err(|e| format!("Connection not found (ID: {connection_id}): {e}"))?;

    let client = SftpClient::connect(&connection)
        .await
        .map_err(|e| format!("Failed to connect SFTP: {e}"))?;

    // Determine initial remote directory (default home or root)
    let initial_path = client
        .canonicalize(".")
        .await
        .unwrap_or_else(|_| "/".to_string());

    let client = Arc::new(client);
    {
        let mut sessions = state.sftp_sessions.write().await;
        sessions.insert(tab_id, client);
    }

    Ok(initial_path)
}

#[tauri::command]
pub async fn sftp_disconnect(
    tab_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut sessions = state.sftp_sessions.write().await;
    sessions.remove(&tab_id);
    Ok(())
}

#[tauri::command]
pub async fn sftp_list_dir(
    tab_id: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<FileEntry>, String> {
    let client = {
        let sessions = state.sftp_sessions.read().await;
        sessions.get(&tab_id).cloned()
    }
    .ok_or_else(|| format!("SFTP session not active for tab: {tab_id}"))?;

    client
        .list_dir(&path)
        .await
        .map_err(|e| format!("Failed to list remote directory '{path}': {e}"))
}

#[tauri::command]
pub async fn sftp_mkdir(
    tab_id: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let client = {
        let sessions = state.sftp_sessions.read().await;
        sessions.get(&tab_id).cloned()
    }
    .ok_or_else(|| format!("SFTP session not active for tab: {tab_id}"))?;

    client
        .mkdir(&path)
        .await
        .map_err(|e| format!("Failed to create remote directory '{path}': {e}"))
}

#[tauri::command]
pub async fn sftp_remove(
    tab_id: String,
    path: String,
    is_dir: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let client = {
        let sessions = state.sftp_sessions.read().await;
        sessions.get(&tab_id).cloned()
    }
    .ok_or_else(|| format!("SFTP session not active for tab: {tab_id}"))?;

    if is_dir {
        client.remove_dir(&path).await
    } else {
        client.remove_file(&path).await
    }
    .map_err(|e| format!("Failed to delete remote item '{path}': {e}"))
}

#[tauri::command]
pub async fn sftp_rename(
    tab_id: String,
    from: String,
    to: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let client = {
        let sessions = state.sftp_sessions.read().await;
        sessions.get(&tab_id).cloned()
    }
    .ok_or_else(|| format!("SFTP session not active for tab: {tab_id}"))?;

    client
        .rename(&from, &to)
        .await
        .map_err(|e| format!("Failed to rename remote item '{from}' to '{to}': {e}"))
}

#[tauri::command]
pub async fn sftp_upload(
    tab_id: String,
    local_path: String,
    remote_path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let client = {
        let sessions = state.sftp_sessions.read().await;
        sessions.get(&tab_id).cloned()
    }
    .ok_or_else(|| format!("SFTP session not active for tab: {tab_id}"))?;

    client
        .upload(&local_path, &remote_path)
        .await
        .map_err(|e| format!("Failed to upload '{local_path}' to '{remote_path}': {e}"))
}

#[tauri::command]
pub async fn sftp_download(
    tab_id: String,
    remote_path: String,
    local_path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let client = {
        let sessions = state.sftp_sessions.read().await;
        sessions.get(&tab_id).cloned()
    }
    .ok_or_else(|| format!("SFTP session not active for tab: {tab_id}"))?;

    client
        .download(&remote_path, &local_path)
        .await
        .map_err(|e| format!("Failed to download '{remote_path}' to '{local_path}': {e}"))
}

#[tauri::command]
pub async fn sftp_download_to_downloads(
    tab_id: String,
    remote_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let client = {
        let sessions = state.sftp_sessions.read().await;
        sessions.get(&tab_id).cloned()
    }
    .ok_or_else(|| format!("SFTP session not active for tab: {tab_id}"))?;

    let file_name = Path::new(&remote_path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "downloaded_file".to_string());

    let downloads_dir = if let Ok(profile) = std::env::var("USERPROFILE") {
        std::path::PathBuf::from(profile).join("Downloads")
    } else if let Ok(home) = std::env::var("HOME") {
        std::path::PathBuf::from(home).join("Downloads")
    } else {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")).join("Downloads")
    };

    if !downloads_dir.exists() {
        let _ = tokio::fs::create_dir_all(&downloads_dir).await;
    }

    let local_path = downloads_dir.join(&file_name).to_string_lossy().to_string();

    client
        .download(&remote_path, &local_path)
        .await
        .map_err(|e| format!("Failed to download '{remote_path}' to '{local_path}': {e}"))?;

    Ok(local_path)
}

#[tauri::command]
pub async fn sftp_download_bytes(
    tab_id: String,
    remote_path: String,
    state: State<'_, AppState>,
) -> Result<Vec<u8>, String> {
    let client = {
        let sessions = state.sftp_sessions.read().await;
        sessions.get(&tab_id).cloned()
    }
    .ok_or_else(|| format!("SFTP session not active for tab: {tab_id}"))?;

    client
        .download_bytes(&remote_path)
        .await
        .map_err(|e| format!("Failed to download bytes from '{remote_path}': {e}"))
}

#[tauri::command]
pub async fn sftp_upload_bytes(
    tab_id: String,
    remote_path: String,
    data: Vec<u8>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let client = {
        let sessions = state.sftp_sessions.read().await;
        sessions.get(&tab_id).cloned()
    }
    .ok_or_else(|| format!("SFTP session not active for tab: {tab_id}"))?;

    client
        .upload_bytes(&data, &remote_path)
        .await
        .map_err(|e| format!("Failed to upload to '{remote_path}': {e}"))
}

// ==========================================
// Local Filesystem Operations for Dual-Pane
// ==========================================

#[tauri::command]
pub async fn local_get_home_dir() -> Result<String, String> {
    if let Ok(profile) = std::env::var("USERPROFILE") {
        return Ok(profile);
    }
    if let Ok(home) = std::env::var("HOME") {
        return Ok(home);
    }
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn local_list_dir(path: String) -> Result<Vec<FileEntry>, String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err(format!("Local path does not exist: {path}"));
    }

    let read_dir = std::fs::read_dir(p)
        .map_err(|e| format!("Failed to read local directory '{path}': {e}"))?;

    let mut results = Vec::new();
    for entry_res in read_dir {
        if let Ok(entry) = entry_res {
            let name = entry.file_name().to_string_lossy().to_string();
            let full_path = entry.path().to_string_lossy().to_string();
            let meta = entry.metadata().ok();

            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let modified = meta.as_ref().and_then(|m| {
                m.modified().ok().map(|st| {
                    let dt: chrono::DateTime<chrono::Utc> = st.into();
                    dt.format("%Y-%m-%d %H:%M").to_string()
                })
            });

            results.push(FileEntry {
                name,
                path: full_path,
                is_dir,
                size,
                modified,
                permissions: None,
            });
        }
    }

    results.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(results)
}

#[tauri::command]
pub async fn local_mkdir(path: String) -> Result<(), String> {
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("Failed to create local directory '{path}': {e}"))
}

#[tauri::command]
pub async fn local_remove(path: String, is_dir: bool) -> Result<(), String> {
    if is_dir {
        std::fs::remove_dir_all(&path)
    } else {
        std::fs::remove_file(&path)
    }
    .map_err(|e| format!("Failed to delete local path '{path}': {e}"))
}

#[tauri::command]
pub async fn local_rename(from: String, to: String) -> Result<(), String> {
    std::fs::rename(&from, &to)
        .map_err(|e| format!("Failed to rename local path '{from}' to '{to}': {e}"))
}
