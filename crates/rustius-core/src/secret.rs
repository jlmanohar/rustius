use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::encrypted_storage::EncryptedStorage;

/// Default service name under which Rustius credentials are registered in the OS keyring.
pub const KEYRING_SERVICE: &str = "rustius";

/// Pluggable secret reference identifying where a sensitive credential is stored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SecretRef {
    /// Stored in the native OS credential manager (Windows Credential Manager / DPAPI, macOS Keychain, Linux Secret Service).
    Keyring { key: String },
    /// Stored in the local encrypted vault file (AES-256-GCM + Argon2id).
    Vault { key: String },
    /// Plaintext fallback (e.g. legacy or explicit in-memory).
    Plaintext { value: String },
    /// User must be prompted interactively on each connection attempt without persisting.
    AskOnConnect,
    /// Future: External vault reference (e.g. Bitwarden item ID, Delinea secret path).
    External { provider: String, id: String },
}

/// In-memory state of the decrypted master password vault.
#[derive(Default, Debug)]
pub struct VaultState {
    pub is_unlocked: bool,
    pub master_password: Option<String>,
    pub secrets: HashMap<String, String>,
}

/// Helper for managing credentials in both native OS keyring and encrypted file vault.
#[derive(Clone)]
pub struct SecretManager {
    service: String,
    vault_path: PathBuf,
    vault_state: Arc<RwLock<VaultState>>,
}

impl Default for SecretManager {
    fn default() -> Self {
        Self::new(KEYRING_SERVICE, PathBuf::from("vault.enc"))
    }
}

impl SecretManager {
    /// Initialize a secret manager for a given service name and vault file path.
    pub fn new(service: impl Into<String>, vault_path: impl Into<PathBuf>) -> Self {
        Self {
            service: service.into(),
            vault_path: vault_path.into(),
            vault_state: Arc::new(RwLock::new(VaultState::default())),
        }
    }

    /// Check whether a persistent encrypted vault file exists on disk.
    pub fn has_vault_file(&self) -> bool {
        self.vault_path.exists()
    }

    /// Check whether the vault is currently unlocked in memory.
    pub fn is_vault_unlocked(&self) -> bool {
        self.vault_state
            .read()
            .map(|g| g.is_unlocked)
            .unwrap_or(false)
    }

    /// Unlock the encrypted vault with the user's master password.
    /// If no vault exists yet on disk, initializes a fresh encrypted vault.
    pub fn unlock_vault(&self, master_password: &str) -> Result<bool> {
        if self.vault_path.exists() {
            let bytes = std::fs::read(&self.vault_path).with_context(|| {
                format!("Failed to read vault file: {}", self.vault_path.display())
            })?;
            let storage = EncryptedStorage::new(master_password);
            match storage.decrypt(&bytes) {
                Ok(json) => {
                    let secrets: HashMap<String, String> = serde_json::from_str(&json)
                        .context("Failed to deserialize decrypted vault contents")?;
                    let mut guard = self
                        .vault_state
                        .write()
                        .map_err(|e| anyhow::anyhow!("Lock poisoned: {e}"))?;
                    guard.is_unlocked = true;
                    guard.master_password = Some(master_password.to_string());
                    guard.secrets = secrets;
                    info!(
                        "Successfully unlocked encrypted vault from {:?}",
                        self.vault_path
                    );
                    Ok(true)
                }
                Err(e) => {
                    warn!("Failed to unlock vault (incorrect password or corrupted): {e}");
                    Ok(false)
                }
            }
        } else {
            // Initialize fresh empty vault
            let storage = EncryptedStorage::new(master_password);
            let empty_json = serde_json::to_string(&HashMap::<String, String>::new())?;
            let bytes = storage.encrypt(&empty_json)?;
            if let Some(parent) = self.vault_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&self.vault_path, bytes)?;

            let mut guard = self
                .vault_state
                .write()
                .map_err(|e| anyhow::anyhow!("Lock poisoned: {e}"))?;
            guard.is_unlocked = true;
            guard.master_password = Some(master_password.to_string());
            guard.secrets = HashMap::new();
            info!("Initialized new encrypted vault at {:?}", self.vault_path);
            Ok(true)
        }
    }

    /// Lock the vault by wiping master password and decrypted secrets from memory.
    pub fn lock_vault(&self) {
        if let Ok(mut guard) = self.vault_state.write() {
            guard.is_unlocked = false;
            guard.master_password = None;
            guard.secrets.clear();
            info!("Locked vault and cleared credentials from memory");
        }
    }

    /// Save the current in-memory secrets to disk using the current master password.
    fn save_vault_locked(&self, guard: &VaultState) -> Result<()> {
        let password = guard
            .master_password
            .as_deref()
            .context("Cannot persist vault without master password in memory")?;
        let storage = EncryptedStorage::new(password);
        let json = serde_json::to_string_pretty(&guard.secrets)?;
        let bytes = storage.encrypt(&json)?;
        if let Some(parent) = self.vault_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&self.vault_path, bytes)?;
        Ok(())
    }

    /// Set or update the master password, re-encrypting all existing secrets on disk.
    pub fn set_master_password(&self, new_password: &str) -> Result<()> {
        let mut guard = self
            .vault_state
            .write()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {e}"))?;
        if self.vault_path.exists() && !guard.is_unlocked {
            bail!("Vault is locked. Unlock vault before changing master password.");
        }
        guard.is_unlocked = true;
        guard.master_password = Some(new_password.to_string());
        self.save_vault_locked(&guard)?;
        info!("Updated vault master password and re-encrypted secrets");
        Ok(())
    }

    /// Store a secret string into the encrypted vault for the given key/account.
    pub fn set_vault_secret(&self, key: &str, secret: &str) -> Result<()> {
        let mut guard = self
            .vault_state
            .write()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {e}"))?;
        if !guard.is_unlocked {
            bail!("Vault is locked. Unlock vault before storing credentials.");
        }
        guard.secrets.insert(key.to_string(), secret.to_string());
        self.save_vault_locked(&guard)?;
        debug!("Stored secret in encrypted vault for key: {}", key);
        Ok(())
    }

    /// Store multiple secrets into the encrypted vault in a single atomic operation,
    /// running the Argon2id key derivation and file encryption only once.
    pub fn set_vault_secrets_batch(&self, entries: &[(&str, &str)]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut guard = self
            .vault_state
            .write()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {e}"))?;
        if !guard.is_unlocked {
            bail!("Vault is locked. Unlock vault before storing credentials.");
        }
        for (key, secret) in entries {
            guard.secrets.insert(key.to_string(), secret.to_string());
        }
        self.save_vault_locked(&guard)?;
        debug!("Batch stored {} secrets in encrypted vault", entries.len());
        Ok(())
    }

    /// Retrieve a secret string from the encrypted vault for the given key/account.
    pub fn get_vault_secret(&self, key: &str) -> Result<Option<String>> {
        let guard = self
            .vault_state
            .read()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {e}"))?;
        if !guard.is_unlocked {
            bail!("Vault is locked. Master password required to access credentials.");
        }
        Ok(guard.secrets.get(key).cloned())
    }

    /// Delete a secret from the encrypted vault for the given key/account.
    pub fn delete_vault_secret(&self, key: &str) -> Result<()> {
        let mut guard = self
            .vault_state
            .write()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {e}"))?;
        if !guard.is_unlocked {
            bail!("Vault is locked.");
        }
        if guard.secrets.remove(key).is_some() {
            self.save_vault_locked(&guard)?;
            debug!("Deleted secret from encrypted vault for key: {}", key);
        }
        Ok(())
    }

    /// Return all secrets currently stored in the vault (used during migrations).
    pub fn list_vault_secrets(&self) -> Result<HashMap<String, String>> {
        let guard = self
            .vault_state
            .read()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {e}"))?;
        if !guard.is_unlocked {
            bail!("Vault is locked. Unlock vault to read credentials.");
        }
        Ok(guard.secrets.clone())
    }

    /// Store a secret string into the OS keyring for the given key/account.
    pub fn set_secret(&self, key: &str, secret: &str) -> Result<()> {
        let entry = keyring::Entry::new(&self.service, key)
            .map_err(|e| anyhow::anyhow!("Failed to create keyring entry for key {key}: {e}"))?;
        let mut last_err = None;
        for attempt in 0..5 {
            match entry.set_password(secret) {
                Ok(()) => {
                    debug!("Stored secret in OS keyring for key: {}", key);
                    return Ok(());
                }
                Err(e) => {
                    last_err = Some(e);
                    std::thread::sleep(std::time::Duration::from_millis(15 * (attempt + 1)));
                }
            }
        }
        Err(anyhow::anyhow!(
            "Failed to save credential to OS keyring for key {key}: {:?}",
            last_err
        ))
    }

    /// Retrieve a secret string from the OS keyring for the given key/account.
    /// Returns `Ok(None)` if no entry exists.
    pub fn get_secret(&self, key: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new(&self.service, key)
            .map_err(|e| anyhow::anyhow!("Failed to access keyring entry for key {key}: {e}"))?;
        for attempt in 0..5 {
            match entry.get_password() {
                Ok(pass) => return Ok(Some(pass)),
                Err(keyring::Error::NoEntry) => {
                    if attempt < 2 {
                        std::thread::sleep(std::time::Duration::from_millis(15 * (attempt + 1)));
                        continue;
                    }
                    debug!("No secret found in OS keyring for key: {}", key);
                    return Ok(None);
                }
                Err(e) => {
                    if attempt == 4 {
                        warn!("Error retrieving secret from OS keyring for key {key}: {e}");
                        return Err(anyhow::anyhow!("Keyring lookup failed for key {key}: {e}"));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(15 * (attempt + 1)));
                }
            }
        }
        Ok(None)
    }

    /// Delete a secret from the OS keyring for the given key/account.
    pub fn delete_secret(&self, key: &str) -> Result<()> {
        let entry = keyring::Entry::new(&self.service, key)
            .map_err(|e| anyhow::anyhow!("Failed to create keyring entry for key {key}: {e}"))?;
        match entry.delete_credential() {
            Ok(()) => {
                debug!("Deleted secret from OS keyring for key: {}", key);
                Ok(())
            }
            Err(keyring::Error::NoEntry) => {
                debug!("Keyring entry already absent for key: {}", key);
                Ok(())
            }
            Err(e) => {
                warn!("Failed to delete secret from OS keyring for key {key}: {e}");
                Err(anyhow::anyhow!(
                    "Failed to delete keyring credential for key {key}: {e}"
                ))
            }
        }
    }

    /// Resolves a secret reference to its plaintext value.
    pub fn resolve(&self, secret_ref: &SecretRef) -> Result<Option<String>> {
        match secret_ref {
            SecretRef::Keyring { key } => self.get_secret(key),
            SecretRef::Vault { key } => self.get_vault_secret(key),
            SecretRef::Plaintext { value } => Ok(Some(value.clone())),
            SecretRef::AskOnConnect => Ok(None),
            SecretRef::External { provider, id } => Err(anyhow::anyhow!(
                "External vault provider '{provider}' (ID: {id}) resolution not yet configured"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_ref_serialization_roundtrip() {
        let secret_keyring = SecretRef::Keyring {
            key: "test-uuid-1234".to_string(),
        };
        let json_k = serde_json::to_string(&secret_keyring).unwrap();
        assert!(json_k.contains("\"type\":\"keyring\""));
        assert!(json_k.contains("\"key\":\"test-uuid-1234\""));

        let deserialized_k: SecretRef = serde_json::from_str(&json_k).unwrap();
        assert_eq!(deserialized_k, secret_keyring);

        let secret_vault = SecretRef::Vault {
            key: "vault-uuid-5678".to_string(),
        };
        let json_v = serde_json::to_string(&secret_vault).unwrap();
        assert!(json_v.contains("\"type\":\"vault\""));
        assert!(json_v.contains("\"key\":\"vault-uuid-5678\""));

        let deserialized_v: SecretRef = serde_json::from_str(&json_v).unwrap();
        assert_eq!(deserialized_v, secret_vault);
    }

    #[test]
    fn test_vault_roundtrip() {
        let temp_dir =
            std::env::temp_dir().join(format!("rustius-vault-test-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let vault_file = temp_dir.join("vault.enc");

        let manager = SecretManager::new("rustius-test", &vault_file);
        assert!(!manager.has_vault_file());
        assert!(!manager.is_vault_unlocked());

        // Initializing with master password
        let master = "CorrectHorseBatteryStaple!";
        assert!(manager.unlock_vault(master).unwrap());
        assert!(manager.has_vault_file());
        assert!(manager.is_vault_unlocked());

        // Store and retrieve secret
        manager.set_vault_secret("conn:1", "pass123").unwrap();
        assert_eq!(
            manager.get_vault_secret("conn:1").unwrap().as_deref(),
            Some("pass123")
        );

        // Resolve via SecretRef::Vault
        let sref = SecretRef::Vault {
            key: "conn:1".into(),
        };
        assert_eq!(manager.resolve(&sref).unwrap().as_deref(), Some("pass123"));

        // Lock vault
        manager.lock_vault();
        assert!(!manager.is_vault_unlocked());
        assert!(manager.get_vault_secret("conn:1").is_err());

        // Unlock with wrong password
        assert!(!manager.unlock_vault("WrongPassword").unwrap());
        assert!(!manager.is_vault_unlocked());

        // Unlock with right password
        assert!(manager.unlock_vault(master).unwrap());
        assert!(manager.is_vault_unlocked());
        assert_eq!(
            manager.get_vault_secret("conn:1").unwrap().as_deref(),
            Some("pass123")
        );

        // Test batch insertion
        let batch = [("conn:2", "pass2"), ("conn:3", "pass3")];
        manager.set_vault_secrets_batch(&batch).unwrap();
        assert_eq!(
            manager.get_vault_secret("conn:2").unwrap().as_deref(),
            Some("pass2")
        );
        assert_eq!(
            manager.get_vault_secret("conn:3").unwrap().as_deref(),
            Some("pass3")
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    #[ignore = "requires real OS keyring"]
    fn test_secret_manager_roundtrip() {
        let manager = SecretManager::default();
        let test_key = format!("test-key-{}", uuid::Uuid::new_v4());
        let test_val = "super-secret-password-123!";

        // Store
        manager
            .set_secret(&test_key, test_val)
            .expect("Failed to store secret");

        // Retrieve
        let retrieved = manager.get_secret(&test_key).expect("Failed to get secret");
        assert_eq!(retrieved.as_deref(), Some(test_val));

        // Resolve via SecretRef
        let secret_ref = SecretRef::Keyring {
            key: test_key.clone(),
        };
        let resolved = manager
            .resolve(&secret_ref)
            .expect("Failed to resolve secret");
        assert_eq!(resolved.as_deref(), Some(test_val));

        // Cleanup
        manager
            .delete_secret(&test_key)
            .expect("Failed to delete secret");
        let after_delete = manager
            .get_secret(&test_key)
            .expect("Failed to check after delete");
        assert_eq!(after_delete, None);
    }
}
