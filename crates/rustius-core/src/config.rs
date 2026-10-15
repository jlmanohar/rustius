use crate::connection::{
    AppSettings, AuthMethod, Connection, CredentialProfile, Folder, RustiusConfig, TreeNode,
    VaultBackend,
};
use crate::error::RustiusError;
use crate::secret::{SecretManager, SecretRef};
use crate::storage::ConfigStorage;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Lightweight store structure for connections and credential profiles on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionsStore {
    #[serde(default = "default_store_version")]
    pub version: String,
    #[serde(default)]
    pub credentials: Vec<CredentialProfile>,
    #[serde(default)]
    pub tree: Vec<TreeNode>,
}

fn default_store_version() -> String {
    "1.0.0".to_string()
}

/// Manages loading, saving, and modifying the application configuration.
///
/// Uses modular storage on disk:
/// - `settings.json`: UI settings, fonts, and terminal preferences
/// - `connections.json`: Connections tree hierarchy and credential profiles
/// - `notes/<id>.md`: Standalone Markdown runbooks per connection
///
/// Uses `Arc<RwLock<RustiusConfig>>` for thread-safe concurrent in-memory access.
pub struct ConfigManager {
    config_dir: PathBuf,
    settings_path: PathBuf,
    connections_path: PathBuf,
    notes_dir: PathBuf,
    storage: Box<dyn ConfigStorage>,
    config: Arc<RwLock<RustiusConfig>>,
    pub secret_manager: Arc<SecretManager>,
    pub is_portable: bool,
}

impl ConfigManager {
    /// Create a new ConfigManager, loading modular configs, migrating legacy TOML, or creating defaults.
    pub fn new(config_dir: PathBuf, storage: Box<dyn ConfigStorage>) -> Result<Self> {
        Self::new_with_portable(config_dir, false, storage)
    }

    /// Create a new ConfigManager with explicit portable mode flag.
    pub fn new_with_portable(
        config_dir: PathBuf,
        is_portable: bool,
        storage: Box<dyn ConfigStorage>,
    ) -> Result<Self> {
        let settings_path = config_dir.join("settings.json");
        let connections_path = config_dir.join("connections.json");
        let notes_dir = config_dir.join("notes");
        let legacy_toml_path = config_dir.join("config.toml");

        let config = if storage.exists(&connections_path) {
            info!("Loading modular config from: {:?}", config_dir);
            // 1. Load settings.json
            let app_settings = if storage.exists(&settings_path) {
                let s_str = storage.read(&settings_path)?;
                serde_json::from_str::<AppSettings>(&s_str).unwrap_or_default()
            } else {
                AppSettings::default()
            };

            // 2. Load connections.json
            let c_str = storage.read(&connections_path)?;
            let mut store: ConnectionsStore = serde_json::from_str(&c_str)
                .context("Failed to parse connections.json")?;

            // 3. Load Markdown runbooks from notes/ directory
            Self::populate_notes_in_tree(&mut store.tree, &notes_dir, storage.as_ref());

            RustiusConfig {
                version: store.version,
                app_settings,
                credentials: store.credentials,
                tree: store.tree,
            }
        } else if storage.exists(&legacy_toml_path) {
            info!(
                "Migrating legacy config.toml to modular storage in {:?}",
                config_dir
            );
            let content = storage.read(&legacy_toml_path)?;
            let legacy: RustiusConfig = toml::from_str(&content)
                .context("Failed to parse legacy config.toml during migration")?;

            // 1. Save settings.json
            let s_json = serde_json::to_string_pretty(&legacy.app_settings)?;
            storage.write(&settings_path, &s_json)?;

            // 2. Save notes to notes/ and write clean connections.json
            let mut clean_tree = legacy.tree.clone();
            Self::sync_and_strip_notes_in_tree(&mut clean_tree, &notes_dir, storage.as_ref())?;
            let store = ConnectionsStore {
                version: legacy.version.clone(),
                credentials: legacy.credentials.clone(),
                tree: clean_tree,
            };
            let c_json = serde_json::to_string_pretty(&store)?;
            storage.write(&connections_path, &c_json)?;

            // 3. Backup config.toml to config.toml.bak and delete original
            let _ = storage.write(&config_dir.join("config.toml.bak"), &content);
            let _ = storage.delete(&legacy_toml_path);

            info!("Migration to modular storage completed successfully");
            legacy
        } else {
            info!("No config found, creating modular defaults at: {:?}", config_dir);
            let config = RustiusConfig::default();

            // Save default settings.json
            let s_json = serde_json::to_string_pretty(&config.app_settings)?;
            storage.write(&settings_path, &s_json)?;

            // Save default connections.json
            let store = ConnectionsStore {
                version: config.version.clone(),
                credentials: config.credentials.clone(),
                tree: config.tree.clone(),
            };
            let c_json = serde_json::to_string_pretty(&store)?;
            storage.write(&connections_path, &c_json)?;

            config
        };

        let vault_path = config_dir.join("vault.enc");
        Ok(Self {
            config_dir,
            settings_path,
            connections_path,
            notes_dir,
            storage,
            config: Arc::new(RwLock::new(config)),
            secret_manager: Arc::new(SecretManager::new("rustius", vault_path)),
            is_portable,
        })
    }

    /// Check if currently configured in portable mode.
    pub fn is_portable(&self) -> bool {
        self.is_portable
    }

    /// Resolve the active effective credential backend synchronously.
    pub fn effective_vault_backend_sync(&self) -> VaultBackend {
        let backend = if let Ok(guard) = self.config.try_read() {
            guard.app_settings.vault_backend
        } else {
            VaultBackend::Auto
        };
        match backend {
            VaultBackend::Auto => {
                if self.is_portable {
                    VaultBackend::MasterPassword
                } else {
                    VaultBackend::Keyring
                }
            }
            other => other,
        }
    }

    /// Resolve the active effective credential backend asynchronously.
    pub async fn effective_vault_backend(&self) -> VaultBackend {
        let backend = self.config.read().await.app_settings.vault_backend;
        match backend {
            VaultBackend::Auto => {
                if self.is_portable {
                    VaultBackend::MasterPassword
                } else {
                    VaultBackend::Keyring
                }
            }
            other => other,
        }
    }

    /// Override the SecretManager instance (useful for test isolation).
    pub fn with_secret_manager(mut self, secret_manager: Arc<SecretManager>) -> Self {
        self.secret_manager = secret_manager;
        self
    }

    /// Access the underlying secret manager.
    pub fn secret_manager(&self) -> &Arc<SecretManager> {
        &self.secret_manager
    }

    /// Resolve a credential profile with its secret hydrated from the OS keyring or secret manager.
    pub fn resolve_credential(&self, cred: &CredentialProfile) -> Result<CredentialProfile> {
        let mut resolved = cred.clone();
        match &cred.auth {
            AuthMethod::Password { username, .. } => {
                let pass = cred.auth.resolve_password(&self.secret_manager)?;
                resolved.auth = AuthMethod::Password {
                    username: username.clone(),
                    password: pass,
                    secret_ref: None,
                };
            }
            AuthMethod::KeyFile {
                username,
                key_path,
                ..
            } => {
                let passphrase = cred.auth.resolve_passphrase(&self.secret_manager)?;
                let resolved_path = crate::connection::resolve_key_path(key_path, Some(&self.config_dir));
                resolved.auth = AuthMethod::KeyFile {
                    username: username.clone(),
                    key_path: resolved_path.to_string_lossy().into_owned(),
                    passphrase,
                    secret_ref: None,
                };
            }
            _ => {}
        }
        Ok(resolved)
    }

    /// Resolve all credentials in a connection into an in-memory copy with secrets hydrated.
    /// If the connection is linked to a reusable `CredentialProfile`, authentication and
    /// domain are inherited and hydrated from the profile.
    pub async fn resolve_connection(&self, conn: &Connection) -> Result<Connection> {
        let mut resolved = conn.clone();

        // If linked to a CredentialProfile, inherit the profile's authentication & domain
        if let Some(cred_id) = conn.credential_id {
            let config = self.config.read().await;
            if let Some(profile) = config.credentials.iter().find(|c| c.id == cred_id) {
                let resolved_profile = self.resolve_credential(profile)?;
                resolved.auth = resolved_profile.auth;

                // For RDP, if the profile defines a domain and connection does not explicitly override it, inherit domain
                if resolved.connection_type == crate::connection::ConnectionType::Rdp {
                    if let Some(ref prof_dom) = profile.domain {
                        if !prof_dom.trim().is_empty() {
                            if let Some(ref mut rdp_conf) = resolved.rdp_config {
                                if rdp_conf.domain.is_none()
                                    || rdp_conf.domain.as_deref().unwrap_or("").trim().is_empty()
                                {
                                    rdp_conf.domain = Some(prof_dom.clone());
                                }
                            } else {
                                resolved.rdp_config = Some(crate::connection::RdpConfig {
                                    domain: Some(prof_dom.clone()),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }

                return Ok(resolved);
            } else {
                warn!(
                    "Connection {} references non-existent credential profile {}",
                    conn.id, cred_id
                );
            }
        }

        // Fallback to connection's inline credentials
        match &conn.auth {
            AuthMethod::Password { username, .. } => {
                let pass = conn.auth.resolve_password(&self.secret_manager)?;
                resolved.auth = AuthMethod::Password {
                    username: username.clone(),
                    password: pass,
                    secret_ref: None,
                };
            }
            AuthMethod::KeyFile {
                username, key_path, ..
            } => {
                let passphrase = conn.auth.resolve_passphrase(&self.secret_manager)?;
                let resolved_path = crate::connection::resolve_key_path(key_path, Some(&self.config_dir));
                resolved.auth = AuthMethod::KeyFile {
                    username: username.clone(),
                    key_path: resolved_path.to_string_lossy().into_owned(),
                    passphrase,
                    secret_ref: None,
                };
            }
            _ => {}
        }
        Ok(resolved)
    }

    /// Helper: extracts mutable reference to the secret_ref in Password or KeyFile variants.
    fn get_auth_secret_ref_mut(auth: &mut AuthMethod) -> Option<&mut Option<SecretRef>> {
        match auth {
            AuthMethod::Password { secret_ref, .. } => Some(secret_ref),
            AuthMethod::KeyFile { secret_ref, .. } => Some(secret_ref),
            _ => None,
        }
    }

    /// Helper: extracts plaintext passwords/passphrases from an AuthMethod, stores them
    /// into the active secret backend under `key`, and points `secret_ref` to the storage.
    pub fn secure_auth_method(&self, auth: &mut AuthMethod, key: &str, entity_type: &str) {
        let backend = self.effective_vault_backend_sync();
        let (secret_opt, target_ref) = match auth {
            AuthMethod::Password { password, secret_ref, .. } => (password, secret_ref),
            AuthMethod::KeyFile { passphrase, secret_ref, .. } => (passphrase, secret_ref),
            _ => return,
        };

        if let Some(p) = secret_opt.take() {
            if !p.is_empty() {
                match backend {
                    VaultBackend::MasterPassword => {
                        if let Err(e) = self.secret_manager.set_vault_secret(key, &p) {
                            warn!("Failed to store {} secret in vault for {}: {}", entity_type, key, e);
                            *secret_opt = Some(p);
                        } else {
                            *target_ref = Some(SecretRef::Vault { key: key.to_string() });
                        }
                    }
                    _ => {
                        if let Err(e) = self.secret_manager.set_secret(key, &p) {
                            warn!("Failed to store {} secret in OS keyring for {}: {}", entity_type, key, e);
                            *secret_opt = Some(p);
                        } else {
                            *target_ref = Some(SecretRef::Keyring { key: key.to_string() });
                        }
                    }
                }
            }
        }
    }

    pub fn secure_connection(&self, conn: &mut Connection) {
        let key = format!("connection:{}", conn.id);
        self.secure_auth_method(&mut conn.auth, &key, "connection");
    }

    /// Secure a credential profile before writing to disk:
    /// Moves plaintext passwords and passphrases into the active secret backend under "credential:<id>".
    pub fn secure_credential(&self, cred: &mut CredentialProfile) {
        let key = format!("credential:{}", cred.id);
        self.secure_auth_method(&mut cred.auth, &key, "profile");
    }

    /// Add a credential profile to configuration and secure its secrets.
    pub async fn add_credential(&self, mut cred: CredentialProfile) -> Result<Uuid> {
        self.secure_credential(&mut cred);
        let id = cred.id;
        self.update_config(|config| {
            if let Some(pos) = config.credentials.iter().position(|c| c.id == id) {
                config.credentials[pos] = cred;
            } else {
                config.credentials.push(cred);
            }
        })
        .await?;
        info!("Saved credential profile: {}", id);
        Ok(id)
    }

    /// Update an existing credential profile and secure its secrets.
    pub async fn update_credential(&self, mut cred: CredentialProfile) -> Result<()> {
        self.secure_credential(&mut cred);
        let id = cred.id;
        self.update_config(|config| {
            if let Some(pos) = config.credentials.iter().position(|c| c.id == id) {
                config.credentials[pos] = cred;
            } else {
                config.credentials.push(cred);
            }
        })
        .await?;
        info!("Updated credential profile: {}", id);
        Ok(())
    }

    /// Delete a credential profile and clean up associated secrets.
    pub async fn delete_credential(&self, id: Uuid) -> Result<()> {
        let key = format!("credential:{}", id);
        let _ = self.secret_manager.delete_secret(&key);
        let _ = self.secret_manager.delete_vault_secret(&key);

        self.update_config(|config| {
            config.credentials.retain(|c| c.id != id);
            // Detach any connections that referenced this profile
            fn detach_in_nodes(nodes: &mut [TreeNode], target_id: Uuid) {
                for node in nodes {
                    match node {
                        TreeNode::Connection(c) => {
                            if c.credential_id == Some(target_id) {
                                c.credential_id = None;
                            }
                        }
                        TreeNode::Folder(f) => {
                            detach_in_nodes(&mut f.children, target_id);
                        }
                    }
                }
            }
            detach_in_nodes(&mut config.tree, id);
        })
        .await?;
        info!("Deleted credential profile: {}", id);
        Ok(())
    }

    /// Get a credential profile by its ID (read-only snapshot).
    pub async fn get_credential(&self, id: Uuid) -> Result<CredentialProfile> {
        let config = self.config.read().await;
        config
            .credentials
            .iter()
            .find(|c| c.id == id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Credential profile not found: {}", id))
    }

    /// Get a clone of the current configuration (read-only snapshot).
    pub async fn get_config(&self) -> RustiusConfig {
        self.config.read().await.clone()
    }

    /// Persist the current in-memory config to modular files on disk.
    pub async fn save(&self) -> Result<()> {
        let config = self.config.read().await;

        // 1. Save settings.json
        let settings_json = serde_json::to_string_pretty(&config.app_settings)
            .context("Failed to serialize settings.json")?;
        self.storage.write(&self.settings_path, &settings_json)?;

        // 2. Save notes to notes/ and strip them for connections.json
        let mut clean_tree = config.tree.clone();
        Self::sync_and_strip_notes_in_tree(&mut clean_tree, &self.notes_dir, self.storage.as_ref())?;

        // 3. Save connections.json
        let connections_store = ConnectionsStore {
            version: config.version.clone(),
            credentials: config.credentials.clone(),
            tree: clean_tree,
        };
        let connections_json = serde_json::to_string_pretty(&connections_store)
            .context("Failed to serialize connections.json")?;
        self.storage.write(&self.connections_path, &connections_json)?;

        debug!(
            "Modular config saved (settings: {:?}, connections: {:?})",
            self.settings_path, self.connections_path
        );
        Ok(())
    }

    /// Read a connection's Markdown runbook note directly from disk.
    pub fn get_connection_note(&self, conn_id: Uuid) -> Option<String> {
        let note_path = self.notes_dir.join(format!("{}.md", conn_id));
        if self.storage.exists(&note_path) {
            self.storage.read(&note_path).ok()
        } else {
            None
        }
    }

    /// Save a connection's Markdown runbook note directly to disk and update in-memory state.
    pub async fn save_connection_note(&self, conn_id: Uuid, note_content: &str) -> Result<()> {
        let note_path = self.notes_dir.join(format!("{}.md", conn_id));
        if !note_content.trim().is_empty() {
            self.storage.write(&note_path, note_content)?;
        } else if self.storage.exists(&note_path) {
            let _ = self.storage.delete(&note_path);
        }

        // Update in-memory config
        {
            let mut config = self.config.write().await;
            if let Some(conn) = Self::find_connection_in_tree_mut(&mut config.tree, conn_id) {
                conn.notes = if note_content.trim().is_empty() {
                    None
                } else {
                    Some(note_content.to_string())
                };
            }
        }
        Ok(())
    }

    /// Get path to the base configuration directory.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// Get path to the modular settings.json file.
    pub fn settings_path(&self) -> &Path {
        &self.settings_path
    }

    /// Get path to the modular connections.json file.
    pub fn connections_path(&self) -> &Path {
        &self.connections_path
    }

    /// Get path to the notes/ directory.
    pub fn notes_dir(&self) -> &Path {
        &self.notes_dir
    }

    /// Apply a mutation to the config and save it.
    ///
    /// The closure `f` receives a mutable reference to the config.
    /// After the closure returns, the config is automatically saved to disk.
    pub async fn update_config<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce(&mut RustiusConfig),
    {
        {
            let mut config = self.config.write().await;
            f(&mut config);
        }
        self.save().await
    }

    /// Add or update a connection in the tree, securing credentials via OS Keyring.
    pub async fn add_connection(
        &self,
        mut connection: Connection,
        parent_folder_id: Option<Uuid>,
    ) -> Result<()> {
        self.secure_connection(&mut connection);
        self.update_config(|config| {
            // If connection with this ID already exists, update it in place
            if Self::update_in_tree(&mut config.tree, &connection) {
                return;
            }

            let node = TreeNode::Connection(connection);
            if let Some(folder_id) = parent_folder_id {
                // Find the folder and insert the connection
                if !Self::insert_into_folder(&mut config.tree, folder_id, node.clone()) {
                    warn!("Folder {} not found, adding to root", folder_id);
                    config.tree.push(node);
                }
            } else {
                config.tree.push(node);
            }
        })
        .await
    }

    /// Create a new folder in the tree.
    pub async fn add_folder(&self, name: String, parent_folder_id: Option<Uuid>) -> Result<Uuid> {
        let folder_id = Uuid::new_v4();
        let folder = Folder {
            id: folder_id,
            name,
            parent_id: parent_folder_id,
            children: Vec::new(),
            expanded: true,
        };

        self.update_config(|config| {
            let node = TreeNode::Folder(folder);
            if let Some(parent_id) = parent_folder_id {
                if !Self::insert_into_folder(&mut config.tree, parent_id, node.clone()) {
                    warn!("Parent folder {} not found, adding to root", parent_id);
                    config.tree.push(node);
                }
            } else {
                config.tree.push(node);
            }
        })
        .await?;

        Ok(folder_id)
    }

    /// Remove a node (connection or folder) from the tree by its ID.
    /// If removing a connection, also purges its secret from keyrings and deletes its markdown note.
    pub async fn remove_node(&self, node_id: Uuid) -> Result<()> {
        let conn_ids = {
            let config = self.config.read().await;
            Self::collect_connection_ids_in_node(&config.tree, node_id)
        };
        for id in conn_ids {
            let key = format!("connection:{}", id);
            let _ = self.secret_manager.delete_secret(&key);
            let _ = self.secret_manager.delete_vault_secret(&key);
            let note_path = self.notes_dir.join(format!("{}.md", id));
            let _ = self.storage.delete(&note_path);
        }
        self.update_config(|config| {
            Self::remove_from_tree(&mut config.tree, node_id);
        })
        .await
    }

    /// Migrate secrets between OS Keyring and Encrypted Master Password Vault.
    pub async fn migrate_vault_backend(
        &self,
        target_backend: VaultBackend,
        master_password: Option<String>,
    ) -> Result<()> {
        let effective_target = match target_backend {
            VaultBackend::Auto => {
                if self.is_portable {
                    VaultBackend::MasterPassword
                } else {
                    VaultBackend::Keyring
                }
            }
            other => other,
        };

        if effective_target == VaultBackend::MasterPassword {
            if !self.secret_manager.is_vault_unlocked() {
                let pwd = master_password
                    .filter(|p| !p.is_empty())
                    .context("Master password is required to initialize or unlock vault")?;
                self.secret_manager.unlock_vault(&pwd)?;
            }

            self.update_config(|config| {
                let mut vault_batch = Vec::new();
                let mut keyring_keys_to_delete = Vec::new();

                fn collect_node_secrets_for_vault(
                    nodes: &mut [TreeNode],
                    sm: &SecretManager,
                    batch: &mut Vec<(String, String)>,
                    to_delete: &mut Vec<String>,
                ) {
                    for node in nodes {
                        match node {
                            TreeNode::Connection(c) => {
                                if let Some(secret_ref) = ConfigManager::get_auth_secret_ref_mut(&mut c.auth) {
                                    if let Some(SecretRef::Keyring { key }) = secret_ref.as_ref() {
                                        let key = key.clone();
                                        if let Ok(Some(secret)) = sm.get_secret(&key) {
                                            batch.push((key.clone(), secret));
                                            to_delete.push(key.clone());
                                            *secret_ref = Some(SecretRef::Vault { key });
                                        }
                                    }
                                }
                            }
                            TreeNode::Folder(f) => {
                                collect_node_secrets_for_vault(&mut f.children, sm, batch, to_delete);
                            }
                        }
                    }
                }

                // 1. Reusable credentials
                for cred in &mut config.credentials {
                    if let Some(secret_ref) = ConfigManager::get_auth_secret_ref_mut(&mut cred.auth) {
                        if let Some(SecretRef::Keyring { key }) = secret_ref.as_ref() {
                            let key = key.clone();
                            if let Ok(Some(secret)) = self.secret_manager.get_secret(&key) {
                                vault_batch.push((key.clone(), secret));
                                keyring_keys_to_delete.push(key.clone());
                                *secret_ref = Some(SecretRef::Vault { key });
                            }
                        }
                    }
                }

                // 2. Tree connections
                collect_node_secrets_for_vault(&mut config.tree, &self.secret_manager, &mut vault_batch, &mut keyring_keys_to_delete);

                let batch_refs: Vec<(&str, &str)> = vault_batch.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                if let Err(e) = self.secret_manager.set_vault_secrets_batch(&batch_refs) {
                    warn!("Failed to batch write secrets to vault during migration: {}", e);
                }
                for k in keyring_keys_to_delete {
                    let _ = self.secret_manager.delete_secret(&k);
                }

                config.app_settings.vault_backend = target_backend;
            })
            .await?;
        } else {
            // Migrating to Keyring
            self.update_config(|config| {
                fn migrate_tree_to_keyring(nodes: &mut [TreeNode], sm: &SecretManager) {
                    for node in nodes {
                        match node {
                            TreeNode::Connection(c) => {
                                if let Some(secret_ref) = ConfigManager::get_auth_secret_ref_mut(&mut c.auth) {
                                    if let Some(SecretRef::Vault { key }) = secret_ref.as_ref() {
                                        let key = key.clone();
                                        if let Ok(Some(secret)) = sm.get_vault_secret(&key) {
                                            let _ = sm.set_secret(&key, &secret);
                                            let _ = sm.delete_vault_secret(&key);
                                            *secret_ref = Some(SecretRef::Keyring { key });
                                        }
                                    }
                                }
                            }
                            TreeNode::Folder(f) => {
                                migrate_tree_to_keyring(&mut f.children, sm);
                            }
                        }
                    }
                }

                // 1. Reusable credentials
                for cred in &mut config.credentials {
                    if let Some(secret_ref) = ConfigManager::get_auth_secret_ref_mut(&mut cred.auth) {
                        if let Some(SecretRef::Vault { key }) = secret_ref.as_ref() {
                            let key = key.clone();
                            if let Ok(Some(secret)) = self.secret_manager.get_vault_secret(&key) {
                                let _ = self.secret_manager.set_secret(&key, &secret);
                                let _ = self.secret_manager.delete_vault_secret(&key);
                                *secret_ref = Some(SecretRef::Keyring { key });
                            }
                        }
                    }
                }

                // 2. Tree connections
                migrate_tree_to_keyring(&mut config.tree, &self.secret_manager);
                config.app_settings.vault_backend = target_backend;
            })
            .await?;
        }

        info!("Successfully migrated vault backend to {:?}", target_backend);
        Ok(())
    }

    /// Find a connection by ID anywhere in the tree.
    pub async fn get_connection(&self, id: Uuid) -> Result<Connection> {
        let config = self.config.read().await;
        Self::find_connection_in_tree(&config.tree, id)
            .ok_or_else(|| RustiusError::ConnectionNotFound(id.to_string()).into())
    }

    /// Recursively secures all connections in a slice of tree nodes.
    pub fn secure_tree_nodes(&self, nodes: &mut [TreeNode]) {
        for node in nodes {
            match node {
                TreeNode::Connection(conn) => {
                    self.secure_connection(conn);
                }
                TreeNode::Folder(folder) => {
                    self.secure_tree_nodes(&mut folder.children);
                }
            }
        }
    }

    /// Import a list of tree nodes (folders and connections), securing all credentials
    /// in the OS keyring and appending them into the config tree.
    pub async fn import_tree(
        &self,
        mut nodes: Vec<TreeNode>,
        parent_folder_id: Option<Uuid>,
    ) -> Result<crate::importer::ImportSummary> {
        self.secure_tree_nodes(&mut nodes);
        let summary = crate::importer::count_imported(&nodes);

        self.update_config(|config| {
            if let Some(parent_id) = parent_folder_id {
                for node in nodes {
                    if !Self::insert_into_folder(&mut config.tree, parent_id, node.clone()) {
                        config.tree.push(node);
                    }
                }
            } else {
                config.tree.extend(nodes);
            }
        })
        .await?;

        Ok(summary)
    }

    // --- Private helper methods for tree traversal ---

    /// Recursively update an existing connection in the tree if present.
    fn update_in_tree(tree: &mut [TreeNode], connection: &Connection) -> bool {
        for item in tree.iter_mut() {
            match item {
                TreeNode::Connection(c) if c.id == connection.id => {
                    *c = connection.clone();
                    return true;
                }
                TreeNode::Folder(f) => {
                    if Self::update_in_tree(&mut f.children, connection) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Recursively insert a node into the folder with the given ID.
    fn insert_into_folder(tree: &mut [TreeNode], folder_id: Uuid, node: TreeNode) -> bool {
        for item in tree.iter_mut() {
            if let TreeNode::Folder(ref mut folder) = item {
                if folder.id == folder_id {
                    folder.children.push(node);
                    return true;
                }
                if Self::insert_into_folder(&mut folder.children, folder_id, node.clone()) {
                    return true;
                }
            }
        }
        false
    }

    /// Recursively remove a node from the tree.
    fn remove_from_tree(tree: &mut Vec<TreeNode>, node_id: Uuid) -> bool {
        let len_before = tree.len();
        tree.retain(|node| match node {
            TreeNode::Connection(c) => c.id != node_id,
            TreeNode::Folder(f) => f.id != node_id,
        });
        if tree.len() < len_before {
            return true;
        }
        for item in tree.iter_mut() {
            if let TreeNode::Folder(ref mut folder) = item {
                if Self::remove_from_tree(&mut folder.children, node_id) {
                    return true;
                }
            }
        }
        false
    }

    /// Recursively find a connection in the tree by ID.
    fn find_connection_in_tree(tree: &[TreeNode], id: Uuid) -> Option<Connection> {
        for node in tree {
            match node {
                TreeNode::Connection(c) if c.id == id => return Some(c.clone()),
                TreeNode::Folder(f) => {
                    if let Some(c) = Self::find_connection_in_tree(&f.children, id) {
                        return Some(c);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Recursively find a mutable connection in the tree by ID.
    fn find_connection_in_tree_mut(tree: &mut [TreeNode], id: Uuid) -> Option<&mut Connection> {
        for node in tree {
            match node {
                TreeNode::Connection(c) if c.id == id => return Some(c),
                TreeNode::Folder(f) => {
                    if let Some(c) = Self::find_connection_in_tree_mut(&mut f.children, id) {
                        return Some(c);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Read Markdown notes from notes/ and populate connection notes in the tree.
    fn populate_notes_in_tree(
        tree: &mut [TreeNode],
        notes_dir: &Path,
        storage: &dyn ConfigStorage,
    ) {
        for node in tree.iter_mut() {
            match node {
                TreeNode::Connection(conn) => {
                    let note_path = notes_dir.join(format!("{}.md", conn.id));
                    if storage.exists(&note_path) {
                        if let Ok(note_text) = storage.read(&note_path) {
                            if !note_text.trim().is_empty() {
                                conn.notes = Some(note_text);
                            }
                        }
                    }
                }
                TreeNode::Folder(folder) => {
                    Self::populate_notes_in_tree(&mut folder.children, notes_dir, storage);
                }
            }
        }
    }

    /// Save connection notes to notes/ and strip them from the tree so connections.json remains compact.
    fn sync_and_strip_notes_in_tree(
        tree: &mut [TreeNode],
        notes_dir: &Path,
        storage: &dyn ConfigStorage,
    ) -> Result<()> {
        for node in tree.iter_mut() {
            match node {
                TreeNode::Connection(conn) => {
                    let note_path = notes_dir.join(format!("{}.md", conn.id));
                    if let Some(ref note_text) = conn.notes {
                        if !note_text.trim().is_empty() {
                            storage.write(&note_path, note_text)?;
                        } else if storage.exists(&note_path) {
                            let _ = storage.delete(&note_path);
                        }
                    } else if storage.exists(&note_path) {
                        let _ = storage.delete(&note_path);
                    }
                    conn.notes = None;
                }
                TreeNode::Folder(folder) => {
                    Self::sync_and_strip_notes_in_tree(&mut folder.children, notes_dir, storage)?;
                }
            }
        }
        Ok(())
    }

    /// Collect all connection IDs within a node (including nested children of folders).
    fn collect_connection_ids_in_node(tree: &[TreeNode], node_id: Uuid) -> Vec<Uuid> {
        let mut ids = Vec::new();
        for node in tree {
            match node {
                TreeNode::Connection(c) if c.id == node_id => {
                    ids.push(c.id);
                }
                TreeNode::Folder(f) if f.id == node_id => {
                    Self::collect_all_connection_ids(&f.children, &mut ids);
                }
                TreeNode::Folder(f) => {
                    let child_ids = Self::collect_connection_ids_in_node(&f.children, node_id);
                    ids.extend(child_ids);
                }
                _ => {}
            }
        }
        ids
    }

    fn collect_all_connection_ids(tree: &[TreeNode], ids: &mut Vec<Uuid>) {
        for node in tree {
            match node {
                TreeNode::Connection(c) => ids.push(c.id),
                TreeNode::Folder(f) => Self::collect_all_connection_ids(&f.children, ids),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::{AuthMethod, ConnectionType};
    use crate::storage::PlaintextStorage;

    fn test_config_dir() -> PathBuf {
        std::env::temp_dir().join(format!("rustius_test_{}", Uuid::new_v4()))
    }

    #[tokio::test]
    async fn test_create_default_config() {
        let dir = test_config_dir();
        let manager = ConfigManager::new(dir.clone(), Box::new(PlaintextStorage))
            .expect("Failed to create ConfigManager");

        let config = manager.get_config().await;
        assert_eq!(config.version, "1.0.0");
        assert!(config.tree.is_empty());

        // Verify modular files were created
        assert!(dir.join("connections.json").exists());
        assert!(dir.join("settings.json").exists());

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_migrate_legacy_toml_to_modular() {
        let dir = test_config_dir();
        std::fs::create_dir_all(&dir).unwrap();

        // Create a simulated legacy config.toml with connections and notes
        let legacy_conn_id = Uuid::new_v4();
        let legacy_toml = format!(
            r##"version = "1.0.0"

[app_settings]
theme = "dark"
font_family = "JetBrains Mono"
font_size = 15
default_terminal_type = "xterm-256color"
show_sidebar = true
sidebar_width = 320
close_tab_on_exit = true
sftp_follow_terminal_folder = true

[[tree]]
node_type = "connection"
id = "{legacy_conn_id}"
name = "Legacy Ubuntu"
connection_type = "ssh"
host = "192.168.1.100"
port = 22
tags = ["prod", "cloud"]
notes = "# Runbook\n\nRun `systemctl restart app` on deploy."

[tree.auth]
type = "password"
username = "ubuntu"
password = "secretpassword"
"##
        );

        std::fs::write(dir.join("config.toml"), &legacy_toml).unwrap();

        // Instantiate manager, which should detect and migrate legacy config.toml
        let manager = ConfigManager::new(dir.clone(), Box::new(PlaintextStorage))
            .expect("Failed to create ConfigManager with migration");

        // Verify modular files exist
        assert!(dir.join("connections.json").exists());
        assert!(dir.join("settings.json").exists());
        assert!(dir.join("config.toml.bak").exists());
        assert!(!dir.join("config.toml").exists());

        // Verify notes file was extracted cleanly
        let note_file = dir.join("notes").join(format!("{}.md", legacy_conn_id));
        assert!(note_file.exists());
        let note_content = std::fs::read_to_string(&note_file).unwrap();
        assert!(note_content.contains("systemctl restart app"));

        // Verify in-memory config loaded successfully
        let cfg = manager.get_config().await;
        assert_eq!(cfg.app_settings.sidebar_width, 320);
        assert_eq!(cfg.tree.len(), 1);
        let conn = manager.get_connection(legacy_conn_id).await.unwrap();
        assert_eq!(conn.name, "Legacy Ubuntu");
        assert_eq!(
            conn.notes.as_deref(),
            Some("# Runbook\n\nRun `systemctl restart app` on deploy.")
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_modular_notes_crud_and_cleanup() {
        let dir = test_config_dir();
        let manager = ConfigManager::new(dir.clone(), Box::new(PlaintextStorage))
            .expect("Failed to create ConfigManager");

        let mut conn = Connection::new(
            "Database Node".into(),
            ConnectionType::Ssh,
            "10.0.0.5".into(),
            22,
            AuthMethod::password("dbadmin", Some("dbpass".into())),
        );
        conn.notes = Some("# DB Runbook\n\n- Step 1: Backup\n- Step 2: Migrate".into());
        let conn_id = conn.id;

        // 1. Add connection with notes
        manager.add_connection(conn, None).await.unwrap();

        // Verify note file exists
        let note_path = dir.join("notes").join(format!("{}.md", conn_id));
        assert!(note_path.exists());
        assert_eq!(
            std::fs::read_to_string(&note_path).unwrap(),
            "# DB Runbook\n\n- Step 1: Backup\n- Step 2: Migrate"
        );

        // 2. Direct note update
        manager
            .save_connection_note(conn_id, "# Updated Runbook v2")
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(&note_path).unwrap(),
            "# Updated Runbook v2"
        );

        let reloaded_conn = manager.get_connection(conn_id).await.unwrap();
        assert_eq!(
            reloaded_conn.notes.as_deref(),
            Some("# Updated Runbook v2")
        );

        // 3. Remove connection -> note file should be cleaned up automatically!
        manager.remove_node(conn_id).await.unwrap();
        assert!(!note_path.exists());

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_add_connection() {
        let dir = test_config_dir();
        let manager = ConfigManager::new(dir.clone(), Box::new(PlaintextStorage))
            .expect("Failed to create ConfigManager");

        let conn = Connection::new(
            "My Server".into(),
            ConnectionType::Ssh,
            "10.0.0.1".into(),
            22,
            AuthMethod::password("root", Some("toor".into())),
        );
        let conn_id = conn.id;

        manager
            .add_connection(conn, None)
            .await
            .expect("Failed to add connection");

        let config = manager.get_config().await;
        assert_eq!(config.tree.len(), 1);

        // Verify we can find it
        let found = manager
            .get_connection(conn_id)
            .await
            .expect("Connection not found");
        assert_eq!(found.name, "My Server");

        // Cleanup
        manager.remove_node(conn_id).await.unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_add_folder_and_connection() {
        let dir = test_config_dir();
        let manager = ConfigManager::new(dir.clone(), Box::new(PlaintextStorage))
            .expect("Failed to create ConfigManager");

        // Create a folder
        let folder_id = manager
            .add_folder("Production".into(), None)
            .await
            .expect("Failed to add folder");

        // Add a connection inside the folder
        let conn = Connection::new(
            "Web Server".into(),
            ConnectionType::Ssh,
            "10.0.0.2".into(),
            22,
            AuthMethod::password("deploy", Some("secret".into())),
        );
        let conn_id = conn.id;

        manager
            .add_connection(conn, Some(folder_id))
            .await
            .expect("Failed to add connection to folder");

        // Verify tree structure
        let config = manager.get_config().await;
        assert_eq!(config.tree.len(), 1); // Only the folder at root

        // Connection should be inside the folder
        if let TreeNode::Folder(f) = &config.tree[0] {
            assert_eq!(f.name, "Production");
            assert_eq!(f.children.len(), 1);
        } else {
            panic!("Expected a folder");
        }

        // Should still be findable by ID
        let found = manager
            .get_connection(conn_id)
            .await
            .expect("Connection not found");
        assert_eq!(found.name, "Web Server");

        // Cleanup
        manager.remove_node(conn_id).await.unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_remove_connection() {
        let dir = test_config_dir();
        let manager = ConfigManager::new(dir.clone(), Box::new(PlaintextStorage))
            .expect("Failed to create ConfigManager");

        let conn = Connection::new(
            "Temp Server".into(),
            ConnectionType::Ssh,
            "10.0.0.3".into(),
            22,
            AuthMethod::password("user", Some("pass".into())),
        );
        let conn_id = conn.id;

        manager.add_connection(conn, None).await.unwrap();
        assert_eq!(manager.get_config().await.tree.len(), 1);

        manager.remove_node(conn_id).await.unwrap();
        assert_eq!(manager.get_config().await.tree.len(), 0);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_keyring_secret_resolution() {
        let dir = test_config_dir();
        let test_service = format!("rustius-test-{}", Uuid::new_v4());
        let manager = ConfigManager::new(dir.clone(), Box::new(PlaintextStorage))
            .expect("Failed to create ConfigManager")
            .with_secret_manager(Arc::new(SecretManager::new(test_service, dir.join("vault.enc"))));

        let plain_pass = "top-secret-password-xyz";
        let conn = Connection::new(
            "Secure Server".into(),
            ConnectionType::Ssh,
            "10.0.0.9".into(),
            22,
            AuthMethod::password("admin", Some(plain_pass.into())),
        );
        let conn_id = conn.id;

        // Adding connection should secure it (strip password from struct, set secret_ref)
        manager.add_connection(conn, None).await.unwrap();

        let stored = manager.get_connection(conn_id).await.unwrap();
        if let AuthMethod::Password {
            password,
            secret_ref,
            ..
        } = &stored.auth
        {
            assert_eq!(password, &None, "Password must not be stored in plaintext!");
            assert!(secret_ref.is_some(), "Secret reference must be populated!");
        } else {
            panic!("Expected password auth");
        }

        // Resolving connection should rehydrate plaintext password from keyring
        let resolved = manager.resolve_connection(&stored).await.unwrap();
        if let AuthMethod::Password { password, .. } = &resolved.auth {
            assert_eq!(password.as_deref(), Some(plain_pass));
        } else {
            panic!("Expected password auth");
        }

        // Cleanup
        manager.remove_node(conn_id).await.unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_credential_profile_crud_and_inheritance() {
        let dir = test_config_dir();
        let test_service = format!("rustius-test-{}", Uuid::new_v4());
        let manager = ConfigManager::new(dir.clone(), Box::new(PlaintextStorage))
            .expect("Failed to create ConfigManager")
            .with_secret_manager(Arc::new(SecretManager::new(test_service, dir.join("vault.enc"))));

        // 1. Create a reusable credential profile with domain + password
        let profile = CredentialProfile::new(
            "Enterprise Domain Admin",
            AuthMethod::password("ad_admin", Some("CorpSecret2026!".into())),
        )
        .with_domain("CORP");
        let profile_id = profile.id;

        manager
            .add_credential(profile)
            .await
            .expect("Failed to add credential profile");

        // Verify profile exists in config and secrets are secured
        let cfg = manager.get_config().await;
        assert_eq!(cfg.credentials.len(), 1);
        let stored_profile = &cfg.credentials[0];
        assert_eq!(stored_profile.name, "Enterprise Domain Admin");
        assert_eq!(stored_profile.domain.as_deref(), Some("CORP"));
        if let AuthMethod::Password {
            password,
            secret_ref,
            ..
        } = &stored_profile.auth
        {
            assert_eq!(
                password, &None,
                "Plaintext password must be secured in keyring"
            );
            assert!(
                secret_ref.is_some(),
                "SecretRef must be set for credential profile"
            );
        } else {
            panic!("Expected password auth");
        }

        // 2. Create an RDP connection that references this profile
        let mut rdp_conn = Connection::new(
            "Primary DC".into(),
            ConnectionType::Rdp,
            "dc01.corp.local".into(),
            3389,
            AuthMethod::password("", None),
        );
        rdp_conn.credential_id = Some(profile_id);

        manager
            .add_connection(rdp_conn.clone(), None)
            .await
            .unwrap();

        // 3. Resolve connection: should inherit domain, username, and hydrated password
        let resolved = manager.resolve_connection(&rdp_conn).await.unwrap();
        if let AuthMethod::Password {
            username, password, ..
        } = &resolved.auth
        {
            assert_eq!(username, "ad_admin");
            assert_eq!(password.as_deref(), Some("CorpSecret2026!"));
        } else {
            panic!("Expected password auth");
        }
        let rdp_config = resolved
            .rdp_config
            .expect("RDP config should be generated or present");
        assert_eq!(rdp_config.domain.as_deref(), Some("CORP"));

        // 4. Update the profile password (simulating credential rotation)
        let mut updated_profile = manager.get_credential(profile_id).await.unwrap();
        updated_profile.auth =
            AuthMethod::password("ad_admin", Some("NewRotatedPassword999!".into()));
        manager.update_credential(updated_profile).await.unwrap();

        // Resolving the same connection should immediately pick up the rotated password!
        let resolved_again = manager.resolve_connection(&rdp_conn).await.unwrap();
        if let AuthMethod::Password { password, .. } = &resolved_again.auth {
            assert_eq!(password.as_deref(), Some("NewRotatedPassword999!"));
        } else {
            panic!("Expected password auth");
        }

        // 5. Delete profile: connection should have credential_id detached
        manager.delete_credential(profile_id).await.unwrap();
        let cfg_after_delete = manager.get_config().await;
        assert_eq!(cfg_after_delete.credentials.len(), 0);
        let conn_after_delete = manager.get_connection(rdp_conn.id).await.unwrap();
        assert_eq!(
            conn_after_delete.credential_id, None,
            "Connection must be detached when profile is deleted"
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_portable_vault_backend_and_migration() {
        let dir = test_config_dir();
        let test_service = format!("rustius-test-{}", Uuid::new_v4());
        let manager = ConfigManager::new_with_portable(dir.clone(), true, Box::new(PlaintextStorage))
            .expect("Failed to create portable ConfigManager")
            .with_secret_manager(Arc::new(SecretManager::new(test_service, dir.join("vault.enc"))));

        assert!(manager.is_portable());
        assert_eq!(manager.effective_vault_backend().await, VaultBackend::MasterPassword);

        // Unlock vault with master password
        let master = "VaultPassword2026!";
        assert!(manager.secret_manager().unlock_vault(master).unwrap());

        // Add connection in portable mode
        let conn = Connection::new(
            "Portable Node".into(),
            ConnectionType::Ssh,
            "192.168.1.100".into(),
            22,
            AuthMethod::password("portable_user", Some("SecretP@ss!".into())),
        );
        let conn_id = conn.id;
        manager.add_connection(conn, None).await.unwrap();

        // Check that secret was stored as Vault reference
        let stored = manager.get_connection(conn_id).await.unwrap();
        if let AuthMethod::Password { secret_ref, .. } = &stored.auth {
            assert!(matches!(secret_ref, Some(SecretRef::Vault { .. })), "Must use SecretRef::Vault in portable mode");
        } else {
            panic!("Expected password auth");
        }

        // Resolving connection retrieves decrypted secret
        let resolved = manager.resolve_connection(&stored).await.unwrap();
        if let AuthMethod::Password { password, .. } = &resolved.auth {
            assert_eq!(password.as_deref(), Some("SecretP@ss!"));
        } else {
            panic!("Expected password auth");
        }

        // Migrate from Vault to Keyring
        manager.migrate_vault_backend(VaultBackend::Keyring, None).await.unwrap();
        let migrated_stored = manager.get_connection(conn_id).await.unwrap();
        if let AuthMethod::Password { secret_ref, .. } = &migrated_stored.auth {
            assert!(matches!(secret_ref, Some(SecretRef::Keyring { .. })), "Must be migrated to SecretRef::Keyring");
        } else {
            panic!("Expected password auth");
        }

        let migrated_resolved = manager.resolve_connection(&migrated_stored).await.unwrap();
        if let AuthMethod::Password { password, .. } = &migrated_resolved.auth {
            assert_eq!(password.as_deref(), Some("SecretP@ss!"));
        } else {
            panic!("Expected password auth");
        }

        // Cleanup
        manager.remove_node(conn_id).await.unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
