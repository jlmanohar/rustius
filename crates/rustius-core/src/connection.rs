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
    pub fn resolve_password(
        &self,
        secret_manager: &SecretManager,
    ) -> anyhow::Result<Option<String>> {
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
    pub fn resolve_passphrase(
        &self,
        secret_manager: &SecretManager,
    ) -> anyhow::Result<Option<String>> {
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

/// Helper to retrieve the current user's home directory across platforms.
pub fn get_user_home_dir() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(profile) = std::env::var("USERPROFILE") {
            if !profile.trim().is_empty() {
                return Some(std::path::PathBuf::from(profile));
            }
        }
        if let (Ok(drive), Ok(path)) = (std::env::var("HOMEDRIVE"), std::env::var("HOMEPATH")) {
            let combined = format!("{}{}", drive, path);
            if !combined.trim().is_empty() {
                return Some(std::path::PathBuf::from(combined));
            }
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return Some(std::path::PathBuf::from(home));
        }
    }

    None
}

/// Resolves an SSH private key path:
/// 1. Expands `~`, `~/...`, or `~\...` into the current user's home directory.
/// 2. If the path is relative and `base_dir` is provided, resolves relative to `base_dir`
///    (or `base_dir`'s parent if the file exists there).
/// 3. Preserves absolute paths as-is.
pub fn resolve_key_path(raw_path: &str, base_dir: Option<&std::path::Path>) -> std::path::PathBuf {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return std::path::PathBuf::new();
    }

    let candidate = if trimmed == "~" {
        get_user_home_dir().unwrap_or_else(|| std::path::PathBuf::from(trimmed))
    } else if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        if let Some(home) = get_user_home_dir() {
            home.join(rest)
        } else {
            std::path::PathBuf::from(trimmed)
        }
    } else {
        std::path::PathBuf::from(trimmed)
    };

    if candidate.is_absolute() {
        return candidate;
    }

    if let Some(base) = base_dir {
        let direct = base.join(&candidate);
        if direct.exists() {
            return direct;
        }
        if let Some(parent) = base.parent() {
            let parent_joined = parent.join(&candidate);
            if parent_joined.exists() {
                return parent_joined;
            }
        }
        direct
    } else {
        candidate
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

/// Helper struct for flexible, fault-tolerant deserialization of `Connection`.
/// Gracefully handles `connection_type`, `conn_type`, both simultaneously, or neither (defaults to `Ssh`),
/// preventing duplicate-field or missing-field deserialization errors.
#[derive(Deserialize)]
struct ConnectionRaw {
    #[serde(
        default = "Uuid::new_v4",
        deserialize_with = "deserialize_uuid_or_fallback"
    )]
    id: Uuid,
    name: String,
    #[serde(default)]
    connection_type: Option<ConnectionType>,
    #[serde(default)]
    conn_type: Option<ConnectionType>,
    host: String,
    port: u16,
    auth: AuthMethod,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default = "default_timestamp")]
    created_at: String,
    #[serde(default = "default_timestamp")]
    updated_at: String,
    #[serde(default)]
    ssh_config: Option<SshConfig>,
    #[serde(default)]
    rdp_config: Option<RdpConfig>,
    #[serde(default, deserialize_with = "deserialize_opt_uuid_or_fallback")]
    credential_id: Option<Uuid>,
}

impl From<ConnectionRaw> for Connection {
    fn from(raw: ConnectionRaw) -> Self {
        let connection_type = raw
            .connection_type
            .or(raw.conn_type)
            .unwrap_or(ConnectionType::Ssh);
        Connection {
            id: raw.id,
            name: raw.name,
            connection_type,
            host: raw.host,
            port: raw.port,
            auth: raw.auth,
            tags: raw.tags,
            environment: raw.environment,
            description: raw.description,
            notes: raw.notes,
            created_at: raw.created_at,
            updated_at: raw.updated_at,
            ssh_config: raw.ssh_config,
            rdp_config: raw.rdp_config,
            credential_id: raw.credential_id,
        }
    }
}

/// A single connection definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "ConnectionRaw")]
pub struct Connection {
    pub id: Uuid,
    pub name: String,
    pub connection_type: ConnectionType,
    pub host: String,
    pub port: u16,
    pub auth: AuthMethod,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
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
    /// Optional reference to a reusable CredentialProfile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_id: Option<Uuid>,
}

/// Temporary credential overrides for a one-off connection session.
/// Never saved to persistent storage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CredentialOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

impl Connection {
    /// Apply temporary credentials override for this session in memory only.
    pub fn apply_override(&mut self, creds: Option<CredentialOverride>) {
        let Some(creds) = creds else { return; };

        if let Some(username) = creds.username {
            let trimmed = username.trim();
            if !trimmed.is_empty() {
                match &mut self.auth {
                    AuthMethod::Password { username: u, .. } => *u = trimmed.to_string(),
                    AuthMethod::KeyFile { username: u, .. } => *u = trimmed.to_string(),
                    AuthMethod::Agent { username: u } => *u = trimmed.to_string(),
                }
            }
        }

        if let Some(password) = creds.password {
            match &mut self.auth {
                AuthMethod::Password {
                    password: p,
                    secret_ref,
                    ..
                } => {
                    *p = Some(password);
                    *secret_ref = None;
                }
                AuthMethod::KeyFile { username, .. } => {
                    self.auth = AuthMethod::Password {
                        username: username.clone(),
                        password: Some(password),
                        secret_ref: None,
                    };
                }
                AuthMethod::Agent { username } => {
                    self.auth = AuthMethod::Password {
                        username: username.clone(),
                        password: Some(password),
                        secret_ref: None,
                    };
                }
            }
        }

        if let Some(domain) = creds.domain {
            let domain_val = if domain.trim().is_empty() {
                None
            } else {
                Some(domain.trim().to_string())
            };
            if let Some(ref mut rdp) = self.rdp_config {
                rdp.domain = domain_val;
            } else {
                self.rdp_config = Some(RdpConfig {
                    domain: domain_val,
                    ..Default::default()
                });
            }
        }
    }
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
    /// Optional Windows domain for authentication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// Resolution mode: "auto" (fit window), "native", "1080p", "720p", "custom"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_mode: Option<String>,
    /// Remote desktop DPI scale percentage: 100, 125, 150, 175, 200, or 0 (auto)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_factor: Option<u32>,
    /// RDP engine: "ironrdp" (default) or "freerdp"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
}

impl Default for RdpConfig {
    fn default() -> Self {
        Self {
            width: default_rdp_width(),
            height: default_rdp_height(),
            color_depth: default_rdp_color_depth(),
            fullscreen: false,
            clipboard_sharing: true,
            domain: None,
            resolution_mode: None,
            scale_factor: None,
            engine: None,
        }
    }
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

impl TreeNode {
    /// Retrieve the unique identifier of this tree node.
    pub fn id(&self) -> Uuid {
        match self {
            TreeNode::Folder(f) => f.id,
            TreeNode::Connection(c) => c.id,
        }
    }
}

/// A reusable credential profile for authenticating across multiple connections.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CredentialProfile {
    #[serde(
        default = "Uuid::new_v4",
        deserialize_with = "deserialize_uuid_or_fallback"
    )]
    pub id: Uuid,
    /// Friendly display name (e.g., "CORP Domain Admin", "AWS Ubuntu Key")
    pub name: String,
    /// Optional Windows domain (for RDP logons)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// Authentication method (Password, KeyFile, or Agent)
    pub auth: AuthMethod,
    #[serde(default = "default_timestamp")]
    pub created_at: String,
    #[serde(default = "default_timestamp")]
    pub updated_at: String,
}

impl CredentialProfile {
    /// Create a new credential profile with default timestamps.
    pub fn new(name: impl Into<String>, auth: AuthMethod) -> Self {
        let now = default_timestamp();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            domain: None,
            auth,
            created_at: now.clone(),
            updated_at: now,
        }
    }

    /// Builder helper to set a Windows domain.
    pub fn with_domain(mut self, domain: impl Into<String>) -> Self {
        self.domain = Some(domain.into());
        self
    }
}

/// The root configuration file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustiusConfig {
    /// Config schema version for future migrations.
    pub version: String,
    pub app_settings: AppSettings,
    /// Reusable credential profiles.
    #[serde(default)]
    pub credentials: Vec<CredentialProfile>,
    /// Root-level tree nodes (folders and connections).
    #[serde(default)]
    pub tree: Vec<TreeNode>,
}

/// The credential storage backend selected by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VaultBackend {
    /// Automatically choose: MasterPassword in portable mode, Keyring in installed mode.
    #[default]
    Auto,
    /// Store credentials in the local encrypted vault file (Argon2id + AES-256-GCM).
    MasterPassword,
    /// Store credentials in the host operating system keyring (DPAPI/Keychain/SecretService).
    Keyring,
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
    /// Credential storage backend (Auto, MasterPassword, Keyring).
    #[serde(default)]
    pub vault_backend: VaultBackend,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            font_family: "Geist Mono".into(),
            font_size: 13,
            default_terminal_type: "xterm-256color".into(),
            show_sidebar: true,
            sidebar_width: 280,
            close_tab_on_exit: false,
            sftp_follow_terminal_folder: true,
            vault_backend: VaultBackend::default(),
        }
    }
}

impl Default for RustiusConfig {
    fn default() -> Self {
        Self {
            version: "1.0.0".into(),
            app_settings: AppSettings::default(),
            credentials: Vec::new(),
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
            description: None,
            notes: None,
            created_at: now.clone(),
            updated_at: now,
            ssh_config: None,
            rdp_config: None,
            credential_id: None,
        }
    }

    /// Set short description on connection.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set markdown notes on connection.
    pub fn with_notes(mut self, notes: impl Into<String>) -> Self {
        self.notes = Some(notes.into());
        self
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
            credentials: Vec::new(),
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
    fn test_connection_description_and_notes_roundtrip() {
        let conn = Connection::new(
            "Database Primary".into(),
            ConnectionType::Ssh,
            "db01.internal".into(),
            22,
            AuthMethod::password("postgres", Some("secret123".into())),
        )
        .with_description("Primary PostgreSQL billing cluster replica (us-east-1)")
        .with_notes("# Billing DB Runbook\n\n- [ ] Check VIP on 10.0.1.20\n- [ ] `systemctl status postgresql`");

        assert_eq!(
            conn.description.as_deref(),
            Some("Primary PostgreSQL billing cluster replica (us-east-1)")
        );
        assert!(conn.notes.as_ref().unwrap().contains("Billing DB Runbook"));

        // Roundtrip via JSON (Frontend IPC simulation)
        let json_str = serde_json::to_string(&conn).unwrap();
        let from_json: Connection = serde_json::from_str(&json_str).unwrap();
        assert_eq!(from_json.description, conn.description);
        assert_eq!(from_json.notes, conn.notes);

        // Roundtrip via TOML (File persistence simulation)
        let toml_str = toml::to_string(&conn).unwrap();
        let from_toml: Connection = toml::from_str(&toml_str).unwrap();
        assert_eq!(from_toml.description, conn.description);
        assert_eq!(from_toml.notes, conn.notes);
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
            credentials: Vec::new(),
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
        assert_eq!(config.app_settings.font_size, 13);
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
        let conn_rdp: Connection =
            serde_json::from_str(json_conn_type).expect("Should parse conn_type");
        assert_eq!(conn_rdp.connection_type, ConnectionType::Rdp);

        // Test with connection_type present
        let json_connection_type = r#"{
            "name": "SFTP Test",
            "connection_type": "sftp",
            "host": "192.168.1.3",
            "port": 22,
            "auth": { "type": "password", "username": "ftpuser" }
        }"#;
        let conn_sftp: Connection =
            serde_json::from_str(json_connection_type).expect("Should parse connection_type");
        assert_eq!(conn_sftp.connection_type, ConnectionType::Sftp);

        // Test with BOTH conn_type and connection_type present simultaneously
        let json_both = r#"{
            "name": "Both Fields Test",
            "conn_type": "rdp",
            "connection_type": "rdp",
            "host": "192.168.1.4",
            "port": 3389,
            "auth": { "type": "password", "username": "admin" }
        }"#;
        let conn_both: Connection = serde_json::from_str(json_both)
            .expect("Should parse both fields without duplicate error");
        assert_eq!(conn_both.connection_type, ConnectionType::Rdp);
    }

    #[test]
    fn test_rdp_connection_with_domain() {
        let json = r#"{
            "name": "Domain Controller",
            "conn_type": "rdp",
            "host": "dc01.corp.local",
            "port": 3389,
            "auth": {
                "type": "password",
                "username": "administrator"
            },
            "rdp_config": {
                "width": 1920,
                "height": 1080,
                "domain": "CORP"
            }
        }"#;

        let conn: Connection =
            serde_json::from_str(json).expect("Should parse RDP connection with domain");
        assert_eq!(conn.connection_type, ConnectionType::Rdp);
        let rdp_config = conn
            .rdp_config
            .as_ref()
            .expect("RDP config should be present");
        assert_eq!(rdp_config.domain.as_deref(), Some("CORP"));
        assert_eq!(rdp_config.width, 1920);
        assert_eq!(rdp_config.height, 1080);

        // Serialize and verify roundtrip
        let serialized = serde_json::to_string(&conn).expect("Should serialize");
        assert!(serialized.contains(r#""domain":"CORP""#));
        let roundtrip: Connection =
            serde_json::from_str(&serialized).expect("Should deserialize roundtrip");
        assert_eq!(
            roundtrip.rdp_config.unwrap().domain.as_deref(),
            Some("CORP")
        );
    }

    #[test]
    fn test_credential_profile_roundtrip() {
        let cred = CredentialProfile::new(
            "CORP Domain Admin",
            AuthMethod::password("administrator", Some("P@ssw0rd123".into())),
        )
        .with_domain("CORP");

        let mut config = RustiusConfig::default();
        config.credentials.push(cred.clone());

        // Serialize to TOML and JSON
        let toml_str = toml::to_string_pretty(&config).expect("Should serialize to TOML");
        assert!(toml_str.contains("CORP Domain Admin"));
        assert!(toml_str.contains("CORP"));

        let parsed: RustiusConfig =
            toml::from_str(&toml_str).expect("Should deserialize from TOML");
        assert_eq!(parsed.credentials.len(), 1);
        let loaded = &parsed.credentials[0];
        assert_eq!(loaded.name, "CORP Domain Admin");
        assert_eq!(loaded.domain.as_deref(), Some("CORP"));
        if let AuthMethod::Password {
            username, password, ..
        } = &loaded.auth
        {
            assert_eq!(username, "administrator");
            assert_eq!(password.as_deref(), Some("P@ssw0rd123"));
        } else {
            panic!("Expected password auth");
        }

        // Test connection with credential_id
        let mut conn = Connection::new(
            "DC-01".into(),
            ConnectionType::Rdp,
            "10.0.0.1".into(),
            3389,
            AuthMethod::password("", None),
        );
        conn.credential_id = Some(cred.id);

        let conn_json = serde_json::to_string(&conn).expect("Should serialize connection");
        assert!(conn_json.contains(&cred.id.to_string()));
        let conn_parsed: Connection =
            serde_json::from_str(&conn_json).expect("Should deserialize connection");
        assert_eq!(conn_parsed.credential_id, Some(cred.id));
    }

    #[test]
    fn test_resolve_key_path_tilde() {
        if let Some(home) = get_user_home_dir() {
            let res = resolve_key_path("~/.ssh/id_ed25519", None);
            assert_eq!(res, home.join(".ssh").join("id_ed25519"));

            let res_win = resolve_key_path(r"~\.ssh\id_rsa", None);
            assert_eq!(res_win, home.join(".ssh").join("id_rsa"));
        }
    }

    #[test]
    fn test_resolve_key_path_relative_with_base_dir() {
        let temp_base =
            std::env::temp_dir().join(format!("rustius-key-test-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&temp_base);

        let res = resolve_key_path("keys/id_ed25519", Some(&temp_base));
        assert_eq!(res, temp_base.join("keys/id_ed25519"));

        let _ = std::fs::remove_dir_all(&temp_base);
    }

    #[test]
    fn test_resolve_key_path_empty_and_absolute() {
        assert_eq!(resolve_key_path("", None), std::path::PathBuf::new());
        assert_eq!(resolve_key_path("   ", None), std::path::PathBuf::new());

        #[cfg(target_os = "windows")]
        {
            let abs = r"C:\Windows\System32\drivers\etc\hosts";
            assert_eq!(resolve_key_path(abs, None), std::path::PathBuf::from(abs));
        }

        #[cfg(not(target_os = "windows"))]
        {
            let abs = "/etc/hosts";
            assert_eq!(resolve_key_path(abs, None), std::path::PathBuf::from(abs));
        }
    }

    #[test]
    fn test_apply_override() {
        let mut conn = Connection {
            id: Uuid::new_v4(),
            name: "Test SSH".to_string(),
            connection_type: ConnectionType::Ssh,
            host: "example.com".to_string(),
            port: 22,
            auth: AuthMethod::password("original_user", Some("orig_pass".to_string())),
            tags: vec![],
            environment: None,
            description: None,
            notes: None,
            created_at: default_timestamp(),
            updated_at: default_timestamp(),
            ssh_config: None,
            rdp_config: None,
            credential_id: None,
        };

        // None override leaves it unchanged
        conn.apply_override(None);
        if let AuthMethod::Password { username, password, .. } = &conn.auth {
            assert_eq!(username, "original_user");
            assert_eq!(password.as_deref(), Some("orig_pass"));
        } else {
            panic!("Expected password auth");
        }

        // Apply override
        conn.apply_override(Some(CredentialOverride {
            username: Some("temp_admin".to_string()),
            password: Some("temporary_secret".to_string()),
            domain: Some("CORP".to_string()),
        }));

        if let AuthMethod::Password { username, password, .. } = &conn.auth {
            assert_eq!(username, "temp_admin");
            assert_eq!(password.as_deref(), Some("temporary_secret"));
        } else {
            panic!("Expected password auth");
        }

        assert_eq!(
            conn.rdp_config.as_ref().and_then(|r| r.domain.as_deref()),
            Some("CORP")
        );
    }
}
