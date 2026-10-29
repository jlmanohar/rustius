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

/// Atomically replace `dest` with `src`.
///
/// On Windows, if `dest` exists, uses `ReplaceFileW` to atomically replace without a window
/// where `dest` is missing. If `dest` does not exist, uses standard `rename`.
/// On Unix, uses `rename` which is atomic and replaces `dest` if it exists.
pub fn atomic_replace_file(src: &Path, dest: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let to_wide = |p: &Path| -> Vec<u16> {
            p.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        };

        if dest.exists() {
            let target_wide = to_wide(dest);
            let tmp_wide = to_wide(src);
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
            if res == 0 {
                return Err(anyhow::anyhow!(
                    "Failed to atomically replace file on Windows: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let _ = std::fs::remove_file(src);
        } else {
            std::fs::rename(src, dest)?;
        }
    }

    #[cfg(not(windows))]
    {
        std::fs::rename(src, dest)?;
    }

    Ok(())
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

    atomic_replace_file(&tmp_path, path)
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

    #[test]
    fn test_atomic_replace_file_preserves_temp_on_failure() {
        let dir = std::env::temp_dir().join(format!("rustius_replace_test_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&dir);

        let src_file = dir.join("download.part");
        std::fs::write(&src_file, b"valuable data").unwrap();

        // Destination is a directory, which causes ReplaceFileW / rename to fail
        let dest_dir = dir.join("dest_dir");
        let _ = std::fs::create_dir_all(&dest_dir);

        let res = atomic_replace_file(&src_file, &dest_dir);
        assert!(res.is_err(), "Replacement against a directory must fail");

        // The temporary source file MUST NOT have been deleted
        assert!(src_file.exists(), "Source temporary file must be preserved on replacement failure");
        assert_eq!(std::fs::read(&src_file).unwrap(), b"valuable data");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
