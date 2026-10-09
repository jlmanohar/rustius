use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use rustius_terminal::pty::LocalTerminal;

use crate::state::AppState;

/// Spawn a local PTY session running the user's default OS shell (e.g. PowerShell or bash).
///
/// Dispatches terminal output asynchronously via `terminal-data-{tab_id}` events
/// and manages lifecycle termination via `terminal-exit-{tab_id}`.
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
    let (exit_tx, mut exit_rx) = mpsc::channel::<u32>(1);

    let terminal =
        LocalTerminal::spawn(cols, rows, output_tx, exit_tx).map_err(|e| e.to_string())?;
    let terminal = Arc::new(terminal);

    // Save in state (closing any previous session for this tab)
    {
        let mut terminals = state.local_terminals.write().await;
        if let Some(old) = terminals.remove(&tab_id) {
            info!("Closing previous PTY session for tab: {}", tab_id);
            old.close();
        }
        terminals.insert(tab_id.clone(), terminal);
    }

    // Spawn async background task to forward bytes to Tauri event
    let data_event = format!("terminal-data-{}", tab_id);
    let exit_event = format!("terminal-exit-{}", tab_id);
    let app_clone = app.clone();
    let tab_id_clone = tab_id.clone();
    let local_terminals = state.local_terminals.clone();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(bytes) = output_rx.recv() => {
                    if let Err(e) = app_clone.emit(&data_event, bytes) {
                        error!("Failed to emit terminal data event: {:?}", e);
                        break;
                    }
                }
                Some(code) = exit_rx.recv() => {
                    // Drain any remaining buffered output bytes from the PTY
                    while let Ok(bytes) = output_rx.try_recv() {
                        let _ = app_clone.emit(&data_event, bytes);
                    }
                    info!("PTY session exited with code: {} for tab: {}", code, tab_id_clone);
                    let _ = app_clone.emit(&exit_event, code);
                    break;
                }
                else => {
                    info!("PTY output channel closed for tab: {}", tab_id_clone);
                    let _ = app_clone.emit(&exit_event, 0u32);
                    break;
                }
            }
        }
        {
            let mut terminals = local_terminals.write().await;
            terminals.remove(&tab_id_clone);
        }
        debug!(
            "PTY output forwarding loop exited for tab: {}",
            tab_id_clone
        );
    });

    Ok(())
}

/// Send keystrokes and raw user input bytes into the local PTY process.
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

/// Resize the pseudo-terminal window dimensions (columns and rows).
#[tauri::command]
pub async fn pty_resize(
    tab_id: String,
    cols: u16,
    rows: u16,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if cols < 10 || rows < 2 {
        return Ok(());
    }

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

/// Terminate the local PTY child process and close the session for this tab.
#[tauri::command]
pub async fn pty_close(tab_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let term = {
        let mut terminals = state.local_terminals.write().await;
        terminals.remove(&tab_id)
    };
    if let Some(t) = term {
        t.close();
    }
    info!("Closed PTY session for tab: {}", tab_id);
    Ok(())
}
