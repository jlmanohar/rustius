use crate::state::AppState;
use rustius_terminal::pty::LocalTerminal;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

#[tauri::command]
pub async fn pty_spawn(
    tab_id: String,
    cols: u16,
    rows: u16,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Spawning local PTY session for tab: {}", tab_id);
    let (output_tx, mut output_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    let terminal = LocalTerminal::spawn(cols, rows, output_tx).map_err(|e| e.to_string())?;
    let terminal = Arc::new(terminal);

    // Save in state
    {
        let mut terminals = state.local_terminals.write().await;
        terminals.insert(tab_id.clone(), terminal);
    }

    // Spawn async background task to forward bytes to Tauri event
    let event_name = format!("terminal-data-{}", tab_id);
    tokio::spawn(async move {
        while let Some(bytes) = output_rx.recv().await {
            if let Err(e) = app.emit(&event_name, bytes) {
                error!("Failed to emit terminal data event: {:?}", e);
                break;
            }
        }
        debug!("PTY output forwarding loop exited for tab: {}", tab_id);
    });

    Ok(())
}

#[tauri::command]
pub async fn pty_write(
    tab_id: String,
    data: Vec<u8>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let terminal = {
        let terminals = state.local_terminals.read().await;
        terminals.get(&tab_id).cloned()
    };

    if let Some(term) = terminal {
        term.write(&data).map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err(format!("PTY session not found for tab: {}", tab_id))
    }
}

#[tauri::command]
pub async fn pty_resize(
    tab_id: String,
    cols: u16,
    rows: u16,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let terminal = {
        let terminals = state.local_terminals.read().await;
        terminals.get(&tab_id).cloned()
    };

    if let Some(term) = terminal {
        term.resize(cols, rows).map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err(format!("PTY session not found for tab: {}", tab_id))
    }
}

#[tauri::command]
pub async fn pty_close(tab_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut terminals = state.local_terminals.write().await;
    terminals.remove(&tab_id);
    info!("Closed PTY session for tab: {}", tab_id);
    Ok(())
}
