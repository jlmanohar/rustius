use rustius_core::config::ConfigManager;
use rustius_core::storage::PlaintextStorage;
use rustius_rdp::client::RdpSession;
use rustius_ssh::client::SshSession;
use rustius_ssh::sftp::SftpClient;
use rustius_terminal::pty::LocalTerminal;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

pub struct AppState {
    pub config_manager: Arc<ConfigManager>,
    pub local_terminals: Arc<RwLock<HashMap<String, Arc<LocalTerminal>>>>,
    pub ssh_sessions: Arc<RwLock<HashMap<String, Arc<SshSession>>>>,
    pub sftp_sessions: Arc<RwLock<HashMap<String, Arc<SftpClient>>>>,
    pub rdp_sessions: Arc<RwLock<HashMap<String, Arc<RdpSession>>>>,
    pub freerdp_sessions: Arc<RwLock<HashMap<String, Arc<rustius_rdp::freerdp::FreeRdpSession>>>>,
    pub data_dir: PathBuf,
    pub is_portable: bool,
}

/// Resolve the platform-standard user profile data directory:
/// - Windows: %APPDATA%\rustius
/// - macOS:   ~/Library/Application Support/rustius
/// - Linux:   $XDG_CONFIG_HOME/rustius or ~/.config/rustius
fn get_system_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("rustius");
        }
        if let Ok(userprofile) = std::env::var("USERPROFILE") {
            return PathBuf::from(userprofile).join("AppData").join("Roaming").join("rustius");
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join("Library").join("Application Support").join("rustius");
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Ok(xdg_cfg) = std::env::var("XDG_CONFIG_HOME") {
            return PathBuf::from(xdg_cfg).join("rustius");
        }
        if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
            return PathBuf::from(xdg_data).join("rustius");
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".config").join("rustius");
        }
    }

    PathBuf::from("rustius-data")
}

/// Resolves data directory following VS Code portable mode conventions:
/// 1. In dev/debug builds, uses `./rustius-data` relative to the workspace for quick testing.
/// 2. If a `data` or `rustius-data` directory exists alongside the executable, activates PORTABLE mode.
/// 3. If a `.portable` marker file exists alongside the executable, activates PORTABLE mode (using `data`).
/// 4. Otherwise, falls back to the OS user profile AppData (INSTALLED mode).
fn resolve_data_dir() -> (PathBuf, bool) {
    if cfg!(debug_assertions) {
        return (PathBuf::from("rustius-data"), true);
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            // Check for explicit "rustius-data" directory alongside the binary
            let portable_rustius = exe_dir.join("rustius-data");
            if portable_rustius.is_dir() {
                return (portable_rustius, true);
            }

            // Check for VS Code style "data" directory alongside the binary
            let portable_vscode = exe_dir.join("data");
            if portable_vscode.is_dir() {
                return (portable_vscode, true);
            }

            // Check for a ".portable" marker file alongside the binary
            let portable_marker = exe_dir.join(".portable");
            if portable_marker.is_file() {
                return (portable_vscode, true);
            }
        }
    }

    (get_system_data_dir(), false)
}

impl AppState {
    pub fn new() -> anyhow::Result<Self> {
        let (base_dir, is_portable) = resolve_data_dir();

        if is_portable {
            info!("Running in PORTABLE mode. Data directory: {:?}", base_dir);
        } else {
            info!("Running in INSTALLED mode. Data directory: {:?}", base_dir);
        }

        if !base_dir.exists() {
            if let Err(err) = std::fs::create_dir_all(&base_dir) {
                error!("Failed to create data directory {:?}: {}", base_dir, err);
            }
        }

        let storage = Box::new(PlaintextStorage);
        let config_manager = Arc::new(ConfigManager::new(base_dir.clone(), storage)?);
        let local_terminals = Arc::new(RwLock::new(HashMap::new()));
        let ssh_sessions = Arc::new(RwLock::new(HashMap::new()));
        let sftp_sessions = Arc::new(RwLock::new(HashMap::new()));
        let rdp_sessions = Arc::new(RwLock::new(HashMap::new()));
        let freerdp_sessions = Arc::new(RwLock::new(HashMap::new()));

        Ok(Self {
            config_manager,
            local_terminals,
            ssh_sessions,
            sftp_sessions,
            rdp_sessions,
            freerdp_sessions,
            data_dir: base_dir,
            is_portable,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_data_dir_resolution() {
        let dir = get_system_data_dir();
        assert!(dir.to_string_lossy().contains("rustius"));
    }

    #[test]
    fn test_debug_resolve_data_dir() {
        let (dir, is_portable) = resolve_data_dir();
        assert_eq!(dir, PathBuf::from("rustius-data"));
        assert!(is_portable);
    }
}
