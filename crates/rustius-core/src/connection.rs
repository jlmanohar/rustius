use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The type of remote connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionType {
    #[default]
    Ssh,
    Sftp,
    Rdp,
}

use crate::secret::{SecretManager, SecretRef};

/// SSH authentication method.
///
/// Uses serde's tagged enum representation so that serialized output
/// includes a `"type"` field discriminator (e.g., `type = "password"`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AuthMethod {
    Password {
        username: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        password: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        secret_ref: Option<SecretRef>,
    },
    KeyFile {
        username: String,
        key_path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        passphrase: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        secret_ref: Option<SecretRef>,
    },
    Agent {
        username: String,
    },
}

impl AuthMethod {
    /// Construct a Password authentication method without secret reference.
    pub fn password(username: impl Into<String>, password: Option<String>) -> Self {
        Self::Password {
            username: username.into(),
            password,
            secret_ref: None,
        }
    }

    /// Construct a KeyFile authentication method without secret reference.
    pub fn key_file(
        username: impl Into<String>,
        key_path: impl Into<String>,
        passphrase: Option<String>,
    ) -> Self {
        Self::KeyFile {
            username: username.into(),
            key_path: key_path.into(),
            passphrase,
            secret_ref: None,
        }
    }

    /// Resolve password from either `secret_ref` (e.g. OS Keyring) or legacy inline `password`.
    pub fn resolve_password(&self, secret_manager: &SecretManager) -> anyhow::Result<Option<String>> {
        match self {
            AuthMethod::Password {
                password,
                secret_ref,
                ..
            } => {
                if let Some(sref) = secret_ref {
                    secret_manager.resolve(sref)
                } else {
                    Ok(password.clone())
                }
            }
            _ => Ok(None),
        }
    }

    /// Resolve private key passphrase from either `secret_ref` (e.g. OS Keyring) or legacy inline `passphrase`.
    pub fn resolve_passphrase(&self, secret_manager: &SecretManager) -> anyhow::Result<Option<String>> {
        match self {
            AuthMethod::KeyFile {
                passphrase,
                secret_ref,
                ..
            } => {
                if let Some(sref) = secret_ref {
                    secret_manager.resolve(sref)
                } else {
                    Ok(passphrase.clone())
                }
            }
            _ => Ok(None),
        }
    }
}

pub fn parse_uuid_or_fallback(s: &str) -> Uuid {
    let trimmed = s.trim();
    Uuid::parse_str(trimmed)
        .unwrap_or_else(|_| Uuid::new_v5(&Uuid::NAMESPACE_OID, trimmed.as_bytes()))
}

fn deserialize_uuid_or_fallback<'de, D>(deserializer: D) -> std::result::Result<Uuid, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(parse_uuid_or_fallback(&s))
}

fn deserialize_opt_uuid_or_fallback<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Uuid>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        Some(s) if !s.trim().is_empty() => Ok(Some(parse_uuid_or_fallback(&s))),
        _ => Ok(None),
    }
}

fn default_timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// A single connection definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    #[serde(
        default = "Uuid::new_v4",
        deserialize_with = "deserialize_uuid_or_fallback"
    )]
    pub id: Uuid,
    pub name: String,
    #[serde(default, alias = "conn_type")]
    pub connection_type: ConnectionType,
    pub host: String,
    pub port: u16,
    pub auth: AuthMethod,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default = "default_timestamp")]
    pub created_at: String,
    #[serde(default = "default_timestamp")]
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
    #[serde(default)]
    pub terminal_type: Option<String>,
    /// Keep-alive interval in seconds.
    #[serde(default, alias = "keepalive_interval_secs")]
    pub keepalive_interval: Option<u64>,
    /// Connect timeout in seconds.
    #[serde(default)]
    pub connect_timeout_secs: Option<u64>,
    /// Legacy ciphers toggle.
    #[serde(default)]
    pub legacy_ciphers: bool,
    /// Legacy kex toggle.
    #[serde(default)]
    pub legacy_kex: bool,
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

fn default_rdp_width() -> u32 {
    1280
}

fn default_rdp_height() -> u32 {
    800
}

fn default_rdp_color_depth() -> u8 {
    24
}

/// RDP-specific configuration for a connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpConfig {
    #[serde(default = "default_rdp_width")]
    pub width: u32,
    #[serde(default = "default_rdp_height")]
    pub height: u32,
    /// Color depth: 16, 24, or 32.
    #[serde(default = "default_rdp_color_depth")]
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
    #[serde(
        default = "Uuid::new_v4",
        deserialize_with = "deserialize_uuid_or_fallback"
    )]
    pub id: Uuid,
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_opt_uuid_or_fallback")]
    pub parent_id: Option<Uuid>,
    #[serde(default)]
    pub children: Vec<TreeNode>,
    #[serde(default = "default_true")]
    pub expanded: bool,
}

/// A node in the connection tree — either a folder or a connection reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "node_type", rename_all = "lowercase")]
#[allow(clippy::large_enum_variant)]
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
    #[serde(default)]
    pub close_tab_on_exit: bool,
    /// Automatically synchronize SFTP file navigator with terminal working directory (e.g. on `cd`).
    /// Mimics MobaXterm's "Follow terminal folder" feature using OSC 7 sequences and terminal title updates.
    #[serde(default = "default_true")]
    pub sftp_follow_terminal_folder: bool,
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
            close_tab_on_exit: false,
            sftp_follow_terminal_folder: true,
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
            environment: None,
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
            AuthMethod::password("admin", Some("secret".into())),
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
    fn test_flexible_deserialization() {
        let json_input = r#"{
            "id": "conn-17282828",
            "name": "Frontend Test",
            "conn_type": "ssh",
            "host": "localhost",
            "port": 2222,
            "environment": "Production",
            "auth": {
                "type": "password",
                "username": "root"
            }
        }"#;

        let conn: Connection =
            serde_json::from_str(json_input).expect("Should deserialize leniently");
        assert_eq!(conn.name, "Frontend Test");
        assert_eq!(conn.connection_type, ConnectionType::Ssh);
        assert_eq!(conn.environment.as_deref(), Some("Production"));
        assert_eq!(conn.id, parse_uuid_or_fallback("conn-17282828"));
    }

    #[test]
    fn test_save_exact_frontend_connection() {
        let json_input = r#"{
            "id": "9ea70796-e4ca-4074-97ee-c5b842da5d74",
            "name": "test",
            "conn_type": "ssh",
            "host": "localhost",
            "port": 2222,
            "environment": "Production",
            "tags": [],
            "created_at": "2026-10-05T17:22:00.000Z",
            "updated_at": "2026-10-05T17:22:00.000Z",
            "auth": {
                "type": "password",
                "username": "root",
                "password": "test"
            },
            "ssh_config": {
                "connect_timeout_secs": 15,
                "keepalive_interval_secs": 30,
                "legacy_ciphers": false,
                "legacy_kex": false,
                "port_forwards": []
            }
        }"#;

        let conn: Connection =
            serde_json::from_str(json_input).expect("Should deserialize frontend JSON");
        let config = RustiusConfig {
            version: "1.0.0".into(),
            app_settings: AppSettings::default(),
            tree: vec![TreeNode::Connection(conn)],
        };

        let toml_str = toml::to_string_pretty(&config).expect("Failed to serialize to TOML");
        let parsed: RustiusConfig = toml::from_str(&toml_str).expect("Failed to parse TOML back");
        assert_eq!(parsed.tree.len(), 1);
    }

    #[test]
    fn test_default_config() {
        let config = RustiusConfig::default();
        assert_eq!(config.version, "1.0.0");
        assert_eq!(config.app_settings.theme, "dark");
        assert_eq!(config.app_settings.font_size, 14);
        assert!(config.app_settings.sftp_follow_terminal_folder);
        assert!(config.tree.is_empty());
    }

    #[test]
    fn test_connection_type_flexible_and_default() {
        // Test with neither connection_type nor conn_type present (defaults to Ssh)
        let json_no_type = r#"{
            "name": "Fallback Test",
            "host": "192.168.1.1",
            "port": 22,
            "auth": { "type": "password", "username": "root" }
        }"#;
        let conn: Connection = serde_json::from_str(json_no_type).expect("Should default to Ssh");
        assert_eq!(conn.connection_type, ConnectionType::Ssh);

        // Test with conn_type present
        let json_conn_type = r#"{
            "name": "RDP Test",
            "conn_type": "rdp",
            "host": "192.168.1.2",
            "port": 3389,
            "auth": { "type": "password", "username": "admin" }
        }"#;
        let conn_rdp: Connection = serde_json::from_str(json_conn_type).expect("Should parse conn_type");
        assert_eq!(conn_rdp.connection_type, ConnectionType::Rdp);

        // Test with connection_type present
        let json_connection_type = r#"{
            "name": "SFTP Test",
            "connection_type": "sftp",
            "host": "192.168.1.3",
            "port": 22,
            "auth": { "type": "password", "username": "ftpuser" }
        }"#;
        let conn_sftp: Connection = serde_json::from_str(json_connection_type).expect("Should parse connection_type");
        assert_eq!(conn_sftp.connection_type, ConnectionType::Sftp);
    }
}
