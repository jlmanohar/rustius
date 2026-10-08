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

pub struct AppState {
    pub config_manager: Arc<ConfigManager>,
    pub local_terminals: Arc<RwLock<HashMap<String, Arc<LocalTerminal>>>>,
    pub ssh_sessions: Arc<RwLock<HashMap<String, Arc<SshSession>>>>,
    pub sftp_sessions: Arc<RwLock<HashMap<String, Arc<SftpClient>>>>,
    pub rdp_sessions: Arc<RwLock<HashMap<String, Arc<RdpSession>>>>,
    pub freerdp_sessions: Arc<RwLock<HashMap<String, Arc<rustius_rdp::freerdp::FreeRdpSession>>>>,
}

impl AppState {
    pub fn new() -> anyhow::Result<Self> {
        // Portable data directory: placed next to executable in release, or local ./rustius-data in dev
        let base_dir = if cfg!(debug_assertions) {
            PathBuf::from("rustius-data")
        } else {
            std::env::current_exe()?
                .parent()
                .map(|p| p.join("rustius-data"))
                .unwrap_or_else(|| PathBuf::from("rustius-data"))
        };

        if !base_dir.exists() {
            let _ = std::fs::create_dir_all(&base_dir);
        }

        let storage = Box::new(PlaintextStorage);
        let config_manager = Arc::new(ConfigManager::new(base_dir, storage)?);
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
        })
    }
}
