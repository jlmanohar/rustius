use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// Default service name under which Rustius credentials are registered in the OS keyring.
pub const KEYRING_SERVICE: &str = "rustius";

/// Pluggable secret reference identifying where a sensitive credential is stored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SecretRef {
    /// Stored in the native OS credential manager (Windows Credential Manager / DPAPI, macOS Keychain, Linux Secret Service).
    Keyring { key: String },
    /// Plaintext fallback (e.g. legacy or explicit in-memory).
    Plaintext { value: String },
    /// User must be prompted interactively on each connection attempt without persisting.
    AskOnConnect,
    /// Future: External vault reference (e.g. Bitwarden item ID, Delinea secret path).
    External { provider: String, id: String },
}

/// Helper for managing credentials in the native operating system keyring.
pub struct SecretManager {
    service: String,
}

impl Default for SecretManager {
    fn default() -> Self {
        Self::new(KEYRING_SERVICE)
    }
}

impl SecretManager {
    /// Initialize a secret manager for a given service name.
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    /// Store a secret string into the OS keyring for the given key/account.
    pub fn set_secret(&self, key: &str, secret: &str) -> Result<()> {
        let entry = keyring::Entry::new(&self.service, key)
            .map_err(|e| anyhow::anyhow!("Failed to create keyring entry for key {key}: {e}"))?;
        entry
            .set_password(secret)
            .map_err(|e| anyhow::anyhow!("Failed to save credential to OS keyring for key {key}: {e}"))?;
        debug!("Stored secret in OS keyring for key: {}", key);
        Ok(())
    }

    /// Retrieve a secret string from the OS keyring for the given key/account.
    /// Returns `Ok(None)` if no entry exists.
    pub fn get_secret(&self, key: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new(&self.service, key)
            .map_err(|e| anyhow::anyhow!("Failed to access keyring entry for key {key}: {e}"))?;
        match entry.get_password() {
            Ok(pass) => Ok(Some(pass)),
            Err(keyring::Error::NoEntry) => {
                debug!("No secret found in OS keyring for key: {}", key);
                Ok(None)
            }
            Err(e) => {
                warn!("Error retrieving secret from OS keyring for key {key}: {e}");
                Err(anyhow::anyhow!("Keyring lookup failed for key {key}: {e}"))
            }
        }
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
                Err(anyhow::anyhow!("Failed to delete keyring credential for key {key}: {e}"))
            }
        }
    }

    /// Resolves a secret reference to its plaintext value.
    pub fn resolve(&self, secret_ref: &SecretRef) -> Result<Option<String>> {
        match secret_ref {
            SecretRef::Keyring { key } => self.get_secret(key),
            SecretRef::Plaintext { value } => Ok(Some(value.clone())),
            SecretRef::AskOnConnect => Ok(None),
            SecretRef::External { provider, id } => {
                Err(anyhow::anyhow!("External vault provider '{provider}' (ID: {id}) resolution not yet configured"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_ref_serialization_roundtrip() {
        let secret = SecretRef::Keyring {
            key: "test-uuid-1234".to_string(),
        };
        let json = serde_json::to_string(&secret).unwrap();
        assert!(json.contains("\"type\":\"keyring\""));
        assert!(json.contains("\"key\":\"test-uuid-1234\""));

        let deserialized: SecretRef = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, secret);
    }

    #[test]
    fn test_secret_manager_roundtrip() {
        let manager = SecretManager::new("rustius-test");
        let test_key = format!("test-key-{}", uuid::Uuid::new_v4());
        let test_val = "super-secret-password-123!";

        // Store
        manager.set_secret(&test_key, test_val).expect("Failed to store secret");

        // Retrieve
        let retrieved = manager.get_secret(&test_key).expect("Failed to get secret");
        assert_eq!(retrieved.as_deref(), Some(test_val));

        // Resolve via SecretRef
        let secret_ref = SecretRef::Keyring { key: test_key.clone() };
        let resolved = manager.resolve(&secret_ref).expect("Failed to resolve secret");
        assert_eq!(resolved.as_deref(), Some(test_val));

        // Cleanup
        manager.delete_secret(&test_key).expect("Failed to delete secret");
        let after_delete = manager.get_secret(&test_key).expect("Failed to check after delete");
        assert_eq!(after_delete, None);
    }
}
