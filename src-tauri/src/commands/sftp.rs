use std::path::Path;
use std::sync::Arc;

use tauri::State;
use tracing::info;

use rustius_ssh::sftp::{FileEntry, SftpClient};

use crate::state::AppState;

/// Establish an SFTP session for a tab using saved connection credentials.
///
/// Connects to the remote host, authenticates, requests the SFTP subsystem,
/// canonicalizes the remote user's initial home directory, and stores the active
/// client in [`AppState::sftp_sessions`].
///
/// Returns the canonical initial path (e.g., `/home/username` or `/root`).
#[tauri::command]
pub async fn sftp_connect(
    tab_id: String,
    connection_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    info!(
        "Starting SFTP session for tab: {}, conn: {}",
        tab_id, connection_id
    );

    let uuid = rustius_core::connection::parse_uuid_or_fallback(&connection_id);
    let connection = state
        .config_manager
        .get_connection(uuid)
        .await
        .map_err(|e| format!("Connection not found (ID: {connection_id}): {e}"))?;
    let connection = state
        .config_manager
        .resolve_connection(&connection)
        .await
        .map_err(|e| format!("Failed to resolve credentials: {e}"))?;

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
        if let Some(old) = sessions.remove(&tab_id) {
            let _ = old.disconnect().await;
        }
        sessions.insert(tab_id, client);
    }

    Ok(initial_path)
}

/// Disconnect and remove the active SFTP session for the given tab.
#[tauri::command]
pub async fn sftp_disconnect(tab_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let session = {
        let mut sessions = state.sftp_sessions.write().await;
        sessions.remove(&tab_id)
    };
    if let Some(s) = session {
        let _ = s.disconnect().await;
    }
    Ok(())
}

/// List files and directories within a remote path.
///
/// Returns a sorted list of [`FileEntry`] items (directories first, then alphabetically).
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

/// Create a remote directory on the SFTP server.
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

/// Remove a remote file or empty directory from the SFTP server.
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

/// Rename or move a remote file or directory on the SFTP server.
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

/// Upload a file from the local filesystem to the remote SFTP server.
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

/// Download a file from the remote SFTP server to a specified local path.
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

/// Download a remote file directly into the OS user's Downloads directory.
///
/// Automatically creates the Downloads folder if it doesn't already exist and
/// returns the absolute path to the saved local file.
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
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join("Downloads")
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

/// Read all bytes of a remote file directly into memory over Tauri IPC.
///
/// Suitable for quick in-browser file previews without local disk persistence.
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

/// Stream an in-memory byte buffer directly to a remote destination path on the SFTP server.
///
/// Used by drag-and-drop uploads from the frontend webview where file buffers are read in JS.
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

/// Retrieve the local user's home directory.
///
/// Used by the dual-pane SFTP manager to initialize the local file browsing pane
/// to the current user's personal directory (`USERPROFILE` on Windows, `HOME` on Linux/macOS).
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

/// List files and directories within a local directory.
///
/// Powers the local filesystem pane in the dual-pane SFTP file manager, allowing
/// users to explore local folders and drag or transfer files to the remote server.
/// Results are sorted with directories first, followed by case-insensitive alphabetical order.
#[tauri::command]
pub async fn local_list_dir(path: String) -> Result<Vec<FileEntry>, String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err(format!("Local path does not exist: {path}"));
    }

    let read_dir = std::fs::read_dir(p)
        .map_err(|e| format!("Failed to read local directory '{path}': {e}"))?;

    let mut results = Vec::new();
    for entry in read_dir.flatten() {
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

    results.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(results)
}

/// Create a new local directory (including parent directories if missing).
///
/// Used when organizing local files or preparing a destination folder before SFTP downloads.
#[tauri::command]
pub async fn local_mkdir(path: String) -> Result<(), String> {
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("Failed to create local directory '{path}': {e}"))
}

/// Delete a local file or directory.
///
/// Recursively removes directory trees if `is_dir` is true, or deletes a single file otherwise.
#[tauri::command]
pub async fn local_remove(path: String, is_dir: bool) -> Result<(), String> {
    if is_dir {
        std::fs::remove_dir_all(&path)
    } else {
        std::fs::remove_file(&path)
    }
    .map_err(|e| format!("Failed to delete local path '{path}': {e}"))
}

/// Rename or move a local file or directory.
///
/// Allows in-place renaming of files in the local pane of the dual-pane SFTP manager.
#[tauri::command]
pub async fn local_rename(from: String, to: String) -> Result<(), String> {
    std::fs::rename(&from, &to)
        .map_err(|e| format!("Failed to rename local path '{from}' to '{to}': {e}"))
}
