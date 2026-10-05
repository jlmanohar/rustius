use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The type of remote connection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionType {
    Ssh,
    Sftp,
    Rdp,
}

/// SSH authentication method.
///
/// Uses serde's tagged enum representation so that serialized output
/// includes a `"type"` field discriminator (e.g., `type = "password"`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AuthMethod {
    Password {
        username: String,
        password: String,
    },
    KeyFile {
        username: String,
        key_path: String,
        passphrase: Option<String>,
    },
    Agent {
        username: String,
    },
}

/// A single connection definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub id: Uuid,
    pub name: String,
    pub connection_type: ConnectionType,
    pub host: String,
    pub port: u16,
    pub auth: AuthMethod,
    #[serde(default)]
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Optional SSH-specific settings.
    pub ssh_config: Option<SshConfig>,
    /// Optional RDP-specific settings.
    pub rdp_config: Option<RdpConfig>,
}

/// SSH-specific configuration for a connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfig {
    /// Enable legacy algorithms (for old devices like network switches).
    #[serde(default)]
    pub allow_legacy_algorithms: bool,
    /// Custom terminal type (e.g., "xterm-256color").
    pub terminal_type: Option<String>,
    /// Keep-alive interval in seconds.
    pub keepalive_interval: Option<u64>,
    /// Port forwarding rules.
    #[serde(default)]
    pub port_forwards: Vec<PortForward>,
}

/// A single port forwarding rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortForward {
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
}

/// RDP-specific configuration for a connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpConfig {
    pub width: u32,
    pub height: u32,
    /// Color depth: 16, 24, or 32.
    pub color_depth: u8,
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(default = "default_true")]
    pub clipboard_sharing: bool,
}

fn default_true() -> bool {
    true
}

/// A folder that can contain connections and sub-folders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: Uuid,
    pub name: String,
    pub parent_id: Option<Uuid>,
    #[serde(default)]
    pub children: Vec<TreeNode>,
    #[serde(default = "default_true")]
    pub expanded: bool,
}

/// A node in the connection tree — either a folder or a connection reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "node_type", rename_all = "lowercase")]
pub enum TreeNode {
    Folder(Folder),
    Connection(Connection),
}

/// The root configuration file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustiusConfig {
    /// Config schema version for future migrations.
    pub version: String,
    pub app_settings: AppSettings,
    /// Root-level tree nodes (folders and connections).
    #[serde(default)]
    pub tree: Vec<TreeNode>,
}

/// Application-wide settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub theme: String,
    pub font_family: String,
    pub font_size: u16,
    pub default_terminal_type: String,
    pub show_sidebar: bool,
    pub sidebar_width: u32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            font_family: "JetBrains Mono".into(),
            font_size: 14,
            default_terminal_type: "xterm-256color".into(),
            show_sidebar: true,
            sidebar_width: 280,
        }
    }
}

impl Default for RustiusConfig {
    fn default() -> Self {
        Self {
            version: "1.0.0".into(),
            app_settings: AppSettings::default(),
            tree: Vec::new(),
        }
    }
}

impl Connection {
    /// Create a new connection with default timestamps.
    pub fn new(
        name: String,
        connection_type: ConnectionType,
        host: String,
        port: u16,
        auth: AuthMethod,
    ) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id: Uuid::new_v4(),
            name,
            connection_type,
            host,
            port,
            auth,
            tags: Vec::new(),
            notes: None,
            created_at: now.clone(),
            updated_at: now,
            ssh_config: None,
            rdp_config: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_serialization_roundtrip() {
        let conn = Connection::new(
            "Test Server".into(),
            ConnectionType::Ssh,
            "192.168.1.100".into(),
            22,
            AuthMethod::Password {
                username: "admin".into(),
                password: "secret".into(),
            },
        );

        let node = TreeNode::Connection(conn);
        let config = RustiusConfig {
            version: "1.0.0".into(),
            app_settings: AppSettings::default(),
            tree: vec![node],
        };

        // Serialize to TOML
        let toml_str = toml::to_string_pretty(&config).expect("Failed to serialize");
        assert!(!toml_str.is_empty());

        // Deserialize back
        let parsed: RustiusConfig = toml::from_str(&toml_str).expect("Failed to deserialize");
        assert_eq!(parsed.version, "1.0.0");
        assert_eq!(parsed.tree.len(), 1);
    }

    #[test]
    fn test_default_config() {
        let config = RustiusConfig::default();
        assert_eq!(config.version, "1.0.0");
        assert_eq!(config.app_settings.theme, "dark");
        assert_eq!(config.app_settings.font_size, 14);
        assert!(config.tree.is_empty());
    }
}
