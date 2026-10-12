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

    /// Delete a config file if it exists.
    fn delete(&self, path: &Path) -> Result<()> {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}

/// Phase 1 implementation: plain configuration files on disk with atomic writes.
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

        // Write to temporary file first for atomic persistence
        let tmp_path = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
        std::fs::write(&tmp_path, content)?;

        // On Windows std::fs::rename will fail if target exists, so remove target if it already exists or use atomic replace
        #[cfg(windows)]
        {
            if path.exists() {
                let _ = std::fs::remove_file(path);
            }
        }
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn delete(&self, path: &Path) -> Result<()> {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
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
