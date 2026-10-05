use crate::state::AppState;
use rustius_core::connection::{AppSettings, Connection, RustiusConfig};
use tauri::State;
use uuid::Uuid;

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
) -> Result<(), String> {
    let parent_uuid = parent_folder_id
        .map(|id| Uuid::parse_str(&id).map_err(|e| e.to_string()))
        .transpose()?;

    state
        .config_manager
        .add_connection(connection, parent_uuid)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_folder(
    name: String,
    parent_folder_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let parent_uuid = parent_folder_id
        .map(|id| Uuid::parse_str(&id).map_err(|e| e.to_string()))
        .transpose()?;

    let folder_id = state
        .config_manager
        .add_folder(name, parent_uuid)
        .await
        .map_err(|e| e.to_string())?;

    Ok(folder_id.to_string())
}

#[tauri::command]
pub async fn remove_node(node_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let uuid = Uuid::parse_str(&node_id).map_err(|e| e.to_string())?;
    state
        .config_manager
        .remove_node(uuid)
        .await
        .map_err(|e| e.to_string())
}
