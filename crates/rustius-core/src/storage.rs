use anyhow::Result;
use std::io::Write;
use std::path::Path;

/// Abstraction over how config data is read/written.
///
/// PlaintextStorage: JSON files for modular configuration (settings, connections, notes).
/// EncryptedStorage: AES-256-GCM authenticated vault with Argon2id key derivation.
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

/// Atomically write bytes to a file.
///
/// On Windows, creates a temporary file in the same directory, flushes to disk via sync_all(),
/// and uses ReplaceFileW (if target exists) or rename (if new).
/// On Unix, writes to temp file, flushes, and atomically renames.
pub fn atomic_write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_path = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let to_wide = |p: &Path| -> Vec<u16> {
            p.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        };

        if path.exists() {
            let target_wide = to_wide(path);
            let tmp_wide = to_wide(&tmp_path);
            let res = unsafe {
                windows_sys::Win32::Storage::FileSystem::ReplaceFileW(
                    target_wide.as_ptr(),
                    tmp_wide.as_ptr(),
                    std::ptr::null(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            let _ = std::fs::remove_file(&tmp_path);
            if res == 0 {
                return Err(anyhow::anyhow!(
                    "Failed to atomically replace file on Windows: {}",
                    std::io::Error::last_os_error()
                ));
            }
        } else {
            std::fs::rename(&tmp_path, path)?;
        }
    }

    #[cfg(not(windows))]
    {
        std::fs::rename(&tmp_path, path)?;
    }

    Ok(())
}

/// Plain configuration files on disk with atomic writes.
pub struct PlaintextStorage;

impl ConfigStorage for PlaintextStorage {
    fn read(&self, path: &Path) -> Result<String> {
        Ok(std::fs::read_to_string(path)?)
    }

    fn write(&self, path: &Path, content: &str) -> Result<()> {
        atomic_write_file(path, content.as_bytes())
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
