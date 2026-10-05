use crate::connection::{Connection, Folder, RustiusConfig, TreeNode};
use crate::error::RustiusError;
use crate::storage::ConfigStorage;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Manages loading, saving, and modifying the application configuration.
///
/// Uses `Arc<RwLock<RustiusConfig>>` for thread-safe concurrent access,
/// since multiple Tauri commands may read/write config simultaneously.
pub struct ConfigManager {
    config_path: PathBuf,
    storage: Box<dyn ConfigStorage>,
    config: Arc<RwLock<RustiusConfig>>,
}

impl ConfigManager {
    /// Create a new ConfigManager, loading existing config or creating defaults.
    pub fn new(config_dir: PathBuf, storage: Box<dyn ConfigStorage>) -> Result<Self> {
        let config_path = config_dir.join("config.toml");

        let config = if storage.exists(&config_path) {
            info!("Loading config from: {:?}", config_path);
            let content = storage.read(&config_path)?;
            toml::from_str(&content).context("Failed to parse config.toml")?
        } else {
            info!("No config found, creating defaults at: {:?}", config_path);
            let config = RustiusConfig::default();
            // Save the default config immediately
            let content = toml::to_string_pretty(&config)
                .context("Failed to serialize default config")?;
            storage.write(&config_path, &content)?;
            config
        };

        Ok(Self {
            config_path,
            storage,
            config: Arc::new(RwLock::new(config)),
        })
    }

    /// Get a clone of the current configuration (read-only snapshot).
    pub async fn get_config(&self) -> RustiusConfig {
        self.config.read().await.clone()
    }

    /// Persist the current in-memory config to disk.
    pub async fn save(&self) -> Result<()> {
        let config = self.config.read().await;
        let content = toml::to_string_pretty(&*config)
            .context("Failed to serialize config")?;
        self.storage.write(&self.config_path, &content)?;
        debug!("Config saved to {:?}", self.config_path);
        Ok(())
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

    /// Add a connection to the tree, optionally under a specific folder.
    pub async fn add_connection(
        &self,
        connection: Connection,
        parent_folder_id: Option<Uuid>,
    ) -> Result<()> {
        self.update_config(|config| {
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
    pub async fn add_folder(
        &self,
        name: String,
        parent_folder_id: Option<Uuid>,
    ) -> Result<Uuid> {
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
    pub async fn remove_node(&self, node_id: Uuid) -> Result<()> {
        self.update_config(|config| {
            Self::remove_from_tree(&mut config.tree, node_id);
        })
        .await
    }

    /// Find a connection by ID anywhere in the tree.
    pub async fn get_connection(&self, id: Uuid) -> Result<Connection> {
        let config = self.config.read().await;
        Self::find_connection_in_tree(&config.tree, id)
            .ok_or_else(|| {
                RustiusError::ConnectionNotFound(id.to_string()).into()
            })
    }

    // --- Private helper methods for tree traversal ---

    /// Recursively insert a node into the folder with the given ID.
    fn insert_into_folder(tree: &mut Vec<TreeNode>, folder_id: Uuid, node: TreeNode) -> bool {
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

        // Verify file was created
        assert!(dir.join("config.toml").exists());

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
            AuthMethod::Password {
                username: "root".into(),
                password: "toor".into(),
            },
        );
        let conn_id = conn.id;

        manager.add_connection(conn, None).await.expect("Failed to add connection");

        let config = manager.get_config().await;
        assert_eq!(config.tree.len(), 1);

        // Verify we can find it
        let found = manager.get_connection(conn_id).await.expect("Connection not found");
        assert_eq!(found.name, "My Server");

        // Cleanup
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
            AuthMethod::Password {
                username: "deploy".into(),
                password: "secret".into(),
            },
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
        let found = manager.get_connection(conn_id).await.expect("Connection not found");
        assert_eq!(found.name, "Web Server");

        // Cleanup
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
            AuthMethod::Password {
                username: "user".into(),
                password: "pass".into(),
            },
        );
        let conn_id = conn.id;

        manager.add_connection(conn, None).await.unwrap();
        assert_eq!(manager.get_config().await.tree.len(), 1);

        manager.remove_node(conn_id).await.unwrap();
        assert_eq!(manager.get_config().await.tree.len(), 0);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }
}
