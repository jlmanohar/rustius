use crate::state::AppState;
use rustius_core::connection::{AppSettings, Connection, RustiusConfig};
use tauri::State;

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> Result<RustiusConfig, String> {
    Ok(state.config_manager.get_config().await)
}

#[tauri::command]
pub async fn save_app_settings(
    settings: AppSettings,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .config_manager
        .update_config(|config| {
            config.app_settings = settings;
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_connection(
    connection: Connection,
    parent_folder_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let parent_uuid = parent_folder_id
        .filter(|s| !s.trim().is_empty())
        .map(|id| rustius_core::connection::parse_uuid_or_fallback(&id));

    let conn_id = connection.id.to_string();
    state
        .config_manager
        .add_connection(connection, parent_uuid)
        .await
        .map_err(|e| e.to_string())?;

    Ok(conn_id)
}

#[tauri::command]
pub async fn add_folder(
    name: String,
    parent_folder_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let parent_uuid = parent_folder_id
        .filter(|s| !s.trim().is_empty())
        .map(|id| rustius_core::connection::parse_uuid_or_fallback(&id));

    let folder_id = state
        .config_manager
        .add_folder(name, parent_uuid)
        .await
        .map_err(|e| e.to_string())?;

    Ok(folder_id.to_string())
}

#[tauri::command]
pub async fn remove_node(node_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let uuid = rustius_core::connection::parse_uuid_or_fallback(&node_id);
    state
        .config_manager
        .remove_node(uuid)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn move_node(
    node_id: String,
    target_parent_id: Option<String>,
    target_index: usize,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let node_uuid = rustius_core::connection::parse_uuid_or_fallback(&node_id);
    let parent_uuid = target_parent_id
        .filter(|s| !s.trim().is_empty())
        .map(|id| rustius_core::connection::parse_uuid_or_fallback(&id));

    state
        .config_manager
        .move_node(node_uuid, parent_uuid, target_index)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_credential(
    credential: rustius_core::connection::CredentialProfile,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let id = state
        .config_manager
        .add_credential(credential)
        .await
        .map_err(|e| e.to_string())?;
    Ok(id.to_string())
}

#[tauri::command]
pub async fn update_credential(
    credential: rustius_core::connection::CredentialProfile,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .config_manager
        .update_credential(credential)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_credential(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let uuid = rustius_core::connection::parse_uuid_or_fallback(&id);
    state
        .config_manager
        .delete_credential(uuid)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_sessions(
    file_path: String,
    source: Option<String>,
    master_password: Option<String>,
    parent_folder_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<rustius_core::importer::ImportSummary, String> {
    let fmt = match source.as_deref().unwrap_or("auto").to_lowercase().as_str() {
        "mremoteng" => rustius_core::importer::ImportFormat::MRemoteNG,
        "mobaxterm" => rustius_core::importer::ImportFormat::MobaXterm,
        _ => rustius_core::importer::ImportFormat::Auto,
    };

    let parent_uuid = parent_folder_id
        .filter(|s| !s.trim().is_empty())
        .map(|id| rustius_core::connection::parse_uuid_or_fallback(&id));

    let nodes =
        rustius_core::importer::import_from_file(&file_path, fmt, master_password.as_deref())
            .map_err(|e| format!("Import failed: {e}"))?;

    state
        .config_manager
        .import_tree(nodes, parent_uuid)
        .await
        .map_err(|e| format!("Failed to save imported sessions: {e}"))
}

#[tauri::command]
pub async fn import_sessions_content(
    content: String,
    source: Option<String>,
    master_password: Option<String>,
    parent_folder_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<rustius_core::importer::ImportSummary, String> {
    let fmt = match source.as_deref().unwrap_or("auto").to_lowercase().as_str() {
        "mremoteng" => rustius_core::importer::ImportFormat::MRemoteNG,
        "mobaxterm" => rustius_core::importer::ImportFormat::MobaXterm,
        _ => rustius_core::importer::ImportFormat::Auto,
    };

    let parent_uuid = parent_folder_id
        .filter(|s| !s.trim().is_empty())
        .map(|id| rustius_core::connection::parse_uuid_or_fallback(&id));

    let nodes = rustius_core::importer::import_content(&content, fmt, master_password.as_deref())
        .map_err(|e| format!("Import failed: {e}"))?;

    state
        .config_manager
        .import_tree(nodes, parent_uuid)
        .await
        .map_err(|e| format!("Failed to save imported sessions: {e}"))
}

#[tauri::command]
pub async fn get_connection_password(
    connection_id: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let uuid = rustius_core::connection::parse_uuid_or_fallback(&connection_id);
    let conn = state
        .config_manager
        .get_connection(uuid)
        .await
        .map_err(|e| e.to_string())?;

    let resolved = state
        .config_manager
        .resolve_connection(&conn)
        .await
        .map_err(|e| e.to_string())?;

    match resolved.auth {
        rustius_core::connection::AuthMethod::Password { password, .. } => Ok(password),
        rustius_core::connection::AuthMethod::KeyFile { passphrase, .. } => Ok(passphrase),
        _ => Ok(None),
    }
}

#[tauri::command]
pub async fn get_credential_password(
    credential_id: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let uuid = rustius_core::connection::parse_uuid_or_fallback(&credential_id);
    let config = state.config_manager.get_config().await;
    let cred = config
        .credentials
        .iter()
        .find(|c| c.id == uuid)
        .ok_or_else(|| format!("Credential profile not found: {}", credential_id))?;

    let resolved = state
        .config_manager
        .resolve_credential(cred)
        .map_err(|e| e.to_string())?;

    match resolved.auth {
        rustius_core::connection::AuthMethod::Password { password, .. } => Ok(password),
        rustius_core::connection::AuthMethod::KeyFile { passphrase, .. } => Ok(passphrase),
        _ => Ok(None),
    }
}

/// Comprehensive vault and credential backend status.
#[derive(serde::Serialize)]
pub struct VaultStatus {
    pub is_portable: bool,
    pub configured_backend: rustius_core::connection::VaultBackend,
    pub effective_backend: rustius_core::connection::VaultBackend,
    pub has_vault_file: bool,
    pub is_unlocked: bool,
}

#[tauri::command]
pub async fn vault_get_status(state: State<'_, AppState>) -> Result<VaultStatus, String> {
    let is_portable = state.config_manager.is_portable();
    let configured_backend = state
        .config_manager
        .get_config()
        .await
        .app_settings
        .vault_backend;
    let effective_backend = state.config_manager.effective_vault_backend().await;
    let has_vault_file = state.config_manager.secret_manager().has_vault_file();
    let is_unlocked = state.config_manager.secret_manager().is_vault_unlocked();

    Ok(VaultStatus {
        is_portable,
        configured_backend,
        effective_backend,
        has_vault_file,
        is_unlocked,
    })
}

#[tauri::command]
pub async fn vault_unlock(password: String, state: State<'_, AppState>) -> Result<bool, String> {
    state
        .config_manager
        .secret_manager()
        .unlock_vault(&password)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn vault_lock(state: State<'_, AppState>) -> Result<(), String> {
    state.config_manager.secret_manager().lock_vault();
    Ok(())
}

#[tauri::command]
pub async fn vault_set_master_password(
    password: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .config_manager
        .secret_manager()
        .set_master_password(&password)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn vault_change_backend(
    target: rustius_core::connection::VaultBackend,
    master_password: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .config_manager
        .migrate_vault_backend(target, master_password)
        .await
        .map_err(|e| e.to_string())
}
