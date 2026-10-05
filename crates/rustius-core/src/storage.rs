use anyhow::Result;
use std::path::Path;

/// Abstraction over how config data is read/written.
///
/// Phase 1: `PlaintextStorage` — reads/writes plain TOML files.
/// Phase 2 (future): `EncryptedStorage` — AES-256-GCM with master password.
///
/// This trait ensures we can swap the storage backend without changing
/// any of the config management or data model code.
pub trait ConfigStorage: Send + Sync {
    /// Read the content of a config file as a string.
    fn read(&self, path: &Path) -> Result<String>;

    /// Write content to a config file, creating parent directories if needed.
    fn write(&self, path: &Path, content: &str) -> Result<()>;

    /// Check if a config file exists.
    fn exists(&self, path: &Path) -> bool;
}

/// Phase 1 implementation: plain TOML files on disk.
pub struct PlaintextStorage;

impl ConfigStorage for PlaintextStorage {
    fn read(&self, path: &Path) -> Result<String> {
        Ok(std::fs::read_to_string(path)?)
    }

    fn write(&self, path: &Path, content: &str) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(std::fs::write(path, content)?)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_plaintext_storage_roundtrip() {
        let storage = PlaintextStorage;
        let dir = std::env::temp_dir().join("rustius_test_storage");
        let path = dir.join("test_config.toml");

        let content = "[app_settings]\ntheme = \"dark\"\n";
        storage.write(&path, content).expect("Failed to write");
        assert!(storage.exists(&path));

        let read_back = storage.read(&path).expect("Failed to read");
        assert_eq!(read_back, content);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_plaintext_storage_nonexistent() {
        let storage = PlaintextStorage;
        let path = PathBuf::from("/nonexistent/path/config.toml");
        assert!(!storage.exists(&path));
    }
}
