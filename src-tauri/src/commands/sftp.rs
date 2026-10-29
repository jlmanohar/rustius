use std::path::{Path, PathBuf};
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
    credentials_override: Option<rustius_core::connection::CredentialOverride>,
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
    let mut connection = state
        .config_manager
        .resolve_connection(&connection)
        .await
        .map_err(|e| format!("Failed to resolve credentials: {e}"))?;

    connection.apply_override(credentials_override);

    let known_hosts_path = state.known_hosts_path();
    let client = SftpClient::connect(&connection, known_hosts_path)
        .await
        .map_err(|e| format!("Failed to connect SFTP: {e}"))?;

    // Determine initial remote directory (default home or root)
    let initial_path = client
        .canonicalize(".")
        .await
        .unwrap_or_else(|_| "/".to_string());

    let client = Arc::new(client);
    let old_session = {
        let mut sessions = state.sftp_sessions.write().await;
        let old = sessions.remove(&tab_id);
        sessions.insert(tab_id, client);
        old
    };
    if let Some(old) = old_session {
        let _ = old.disconnect().await;
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

    // Validate the full download destination path, not just the leaf filename
    let local_p = Path::new(&local_path);
    let _file_name = local_p
        .file_name()
        .and_then(|f| f.to_str())
        .ok_or_else(|| format!("Invalid destination filename in: {local_path}"))?;
    validate_safe_destination_path(&local_path)?;

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

    let mut candidate = downloads_dir.join(&file_name);
    if candidate.exists() {
        let stem = Path::new(&file_name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "downloaded_file".to_string());
        let ext = Path::new(&file_name)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();

        let mut counter = 1;
        while candidate.exists() {
            candidate = downloads_dir.join(format!("{stem} ({counter}){ext}"));
            counter += 1;
        }
    }

    let local_path = candidate.to_string_lossy().to_string();

    client
        .download(&remote_path, &local_path)
        .await
        .map_err(|e| format!("Failed to download '{remote_path}' to '{local_path}': {e}"))?;

    Ok(local_path)
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
    if !tokio::fs::try_exists(p).await.unwrap_or(false) {
        return Err(format!("Local path does not exist: {path}"));
    }

    let mut read_dir = tokio::fs::read_dir(p)
        .await
        .map_err(|e| format!("Failed to read local directory '{path}': {e}"))?;

    let mut results = Vec::new();
    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        let full_path = entry.path().to_string_lossy().to_string();
        let meta = entry.metadata().await.ok();

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
    validate_safe_destination_path(&path)?;
    tokio::fs::create_dir_all(&path)
        .await
        .map_err(|e| format!("Failed to create local directory '{path}': {e}"))
}

fn validate_safe_canonical_path(canon: &Path) -> Result<(), String> {
    if canon.parent().is_none() {
        return Err(format!(
            "Operation not permitted on filesystem root: {}",
            canon.display()
        ));
    }

    // Block home directory and its ancestors
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok();
    if let Some(h) = home {
        if let Ok(canon_home) = std::fs::canonicalize(&h) {
            if canon == canon_home {
                return Err(format!(
                    "Operation not permitted on user home directory: {}",
                    canon.display()
                ));
            }
            // Block ancestors of home (e.g. C:\Users when home is C:\Users\you)
            if canon_home.starts_with(canon) {
                return Err(format!(
                    "Operation not permitted on ancestor of home directory: {}",
                    canon.display()
                ));
            }
        }
    }

    // Block known system directories
    #[cfg(windows)]
    {
        // Check standard system env vars
        let sys_vars = ["SystemRoot", "ProgramFiles", "ProgramFiles(x86)", "ProgramData"];
        for var in &sys_vars {
            if let Ok(val) = std::env::var(var) {
                if let Ok(canon_sys) = std::fs::canonicalize(&val) {
                    if canon == canon_sys || canon.starts_with(&canon_sys) {
                        return Err(format!(
                            "Operation not permitted on system directory: {}",
                            canon.display()
                        ));
                    }
                }
            }
        }

        // Also check if the first directory component immediately under a drive root is a system folder
        let mut comps = canon.components();
        while let Some(c) = comps.next() {
            if matches!(c, std::path::Component::RootDir) {
                break;
            }
        }
        if let Some(std::path::Component::Normal(first_dir)) = comps.next() {
            let dir_str = first_dir.to_string_lossy().to_lowercase();
            let blocked = [
                "windows",
                "program files",
                "program files (x86)",
                "programdata",
                "recovery",
                "system volume information",
                "$recycle.bin",
            ];
            if blocked.contains(&dir_str.as_str()) {
                return Err(format!(
                    "Operation not permitted on system directory: {}",
                    canon.display()
                ));
            }
        }
    }
    #[cfg(not(windows))]
    {
        let blocked: &[&str] = &[
            "/etc", "/usr", "/bin", "/sbin", "/var", "/boot", "/sys", "/proc", "/dev",
        ];
        let canon_str = canon.to_string_lossy();
        for b in blocked {
            if canon_str.as_ref() == *b || canon_str.starts_with(&format!("{b}/")) {
                return Err(format!(
                    "Operation not permitted on system directory: {}",
                    canon.display()
                ));
            }
        }
    }

    Ok(())
}

fn validate_safe_local_path(path_str: &str) -> Result<&Path, String> {
    let p = Path::new(path_str);
    if path_str.trim().is_empty() {
        return Err("Empty path provided".into());
    }
    let components: Vec<_> = p.components().collect();
    if components.is_empty() || components.len() == 1 {
        return Err(format!(
            "Operation not permitted on filesystem root: {path_str}"
        ));
    }
    let canon = std::fs::canonicalize(p).map_err(|e| {
        format!("Cannot validate path '{}': {}", path_str, e)
    })?;
    validate_safe_canonical_path(&canon)?;
    Ok(p)
}

pub fn validate_safe_destination_path(path_str: &str) -> Result<PathBuf, String> {
    let p = Path::new(path_str);
    if path_str.trim().is_empty() {
        return Err("Empty path provided".into());
    }
    let components: Vec<_> = p.components().collect();
    if components.is_empty() || components.len() == 1 {
        return Err(format!(
            "Operation not permitted on filesystem root: {path_str}"
        ));
    }
    for comp in &components {
        if matches!(comp, std::path::Component::ParentDir) {
            return Err(format!(
                "Path traversal component ('..') rejected: {path_str}"
            ));
        }
    }

    // Find nearest existing ancestor
    let nearest_ancestor = p
        .ancestors()
        .find(|a| a.exists() && a.as_os_str() != "")
        .ok_or_else(|| format!("Cannot validate path without existing ancestor: {path_str}"))?;

    let canon_ancestor = std::fs::canonicalize(nearest_ancestor)
        .map_err(|e| format!("Cannot validate path '{}': {}", nearest_ancestor.display(), e))?;

    // Block ancestor if it's filesystem root
    if canon_ancestor.parent().is_none() && nearest_ancestor == p {
        return Err(format!(
            "Operation not permitted on filesystem root: {}",
            canon_ancestor.display()
        ));
    }

    // Reconstruct the virtual target path from the canonical ancestor
    let relative = p
        .strip_prefix(nearest_ancestor)
        .unwrap_or(Path::new(""));
    let virtual_canon = canon_ancestor.join(relative);

    validate_safe_canonical_path(&virtual_canon)?;
    Ok(virtual_canon)
}

/// Delete a local file or directory.
///
/// Recursively removes directory trees if `is_dir` is true, or deletes a single file otherwise.
#[tauri::command]
pub async fn local_remove(path: String, is_dir: bool) -> Result<(), String> {
    validate_safe_local_path(&path)?;
    if is_dir {
        tokio::fs::remove_dir_all(&path).await
    } else {
        tokio::fs::remove_file(&path).await
    }
    .map_err(|e| format!("Failed to delete local path '{path}': {e}"))
}

/// Rename or move a local file or directory.
///
/// Allows in-place renaming of files in the local pane of the dual-pane SFTP manager.
#[tauri::command]
pub async fn local_rename(from: String, to: String) -> Result<(), String> {
    validate_safe_local_path(&from)?;
    validate_safe_destination_path(&to)?;
    tokio::fs::rename(&from, &to)
        .await
        .map_err(|e| format!("Failed to rename local path '{from}' to '{to}': {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_safe_local_path_rejects_empty_and_root() {
        assert!(validate_safe_local_path("").is_err());
        assert!(validate_safe_local_path("   ").is_err());
        #[cfg(windows)]
        {
            assert!(validate_safe_local_path("C:\\").is_err());
            assert!(validate_safe_local_path("C:").is_err());
        }
        #[cfg(not(windows))]
        {
            assert!(validate_safe_local_path("/").is_err());
        }
    }

    #[test]
    fn test_validate_safe_local_path_rejects_home_and_ancestors() {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .expect("Home environment variable should be present");
        assert!(validate_safe_local_path(&home).is_err());

        // Ancestor of home (e.g. C:\Users or /home)
        if let Some(parent) = Path::new(&home).parent() {
            if parent.exists() && parent.parent().is_some() {
                assert!(validate_safe_local_path(&parent.to_string_lossy()).is_err());
            }
        }
    }

    #[test]
    fn test_validate_safe_local_path_rejects_system_dirs() {
        #[cfg(windows)]
        {
            assert!(validate_safe_local_path("C:\\Windows").is_err());
            assert!(validate_safe_local_path("C:\\Program Files").is_err());
        }
        #[cfg(not(windows))]
        {
            assert!(validate_safe_local_path("/etc").is_err());
            assert!(validate_safe_local_path("/usr").is_err());
        }
    }

    #[test]
    fn test_validate_safe_destination_path_allows_new_folder() {
        let temp_dir = std::env::temp_dir();
        let new_folder = temp_dir.join(format!("rustius_test_new_folder_{}", uuid::Uuid::new_v4()));
        assert!(!new_folder.exists());
        assert!(validate_safe_destination_path(&new_folder.to_string_lossy()).is_ok());
    }

    #[test]
    fn test_validate_safe_destination_path_allows_nested_windows_name() {
        let temp_dir = std::env::temp_dir();
        // A folder named "windows" inside temp should be allowed because it's not the OS system root
        let nested = temp_dir.join("windows").join("my_notes.txt");
        assert!(validate_safe_destination_path(&nested.to_string_lossy()).is_ok());
    }

    #[test]
    fn test_validate_safe_destination_path_allows_dots_in_filename() {
        let temp_dir = std::env::temp_dir();
        let dot_file = temp_dir.join("backup..tar.gz");
        assert!(validate_safe_destination_path(&dot_file.to_string_lossy()).is_ok());
    }

    #[test]
    fn test_validate_safe_destination_path_rejects_parent_traversal() {
        let temp_dir = std::env::temp_dir();
        let traversal = temp_dir.join("..").join("something");
        assert!(validate_safe_destination_path(&traversal.to_string_lossy()).is_err());
    }
}
