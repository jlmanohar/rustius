use std::path::Path;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{bail, Context, Result};
use rand::RngCore;

use crate::storage::ConfigStorage;

const MAGIC_HEADER: &[u8; 15] = b"RUSTIUS_ENC_V1\0";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;

/// An encrypted storage backend using AES-256-GCM and Argon2id key derivation.
///
/// Encrypts configuration files on disk with a user-supplied master password.
/// Files that do not begin with the encryption magic header are automatically treated
/// as legacy plaintext TOML, enabling seamless in-place migration without data loss.
pub struct EncryptedStorage {
    master_password: String,
}

impl EncryptedStorage {
    /// Initialize with a master password.
    pub fn new(master_password: impl Into<String>) -> Self {
        Self {
            master_password: master_password.into(),
        }
    }

    /// Derive a 256-bit symmetric key from master password and a salt using Argon2id.
    fn derive_key(&self, salt: &[u8]) -> Result<[u8; 32]> {
        let mut key = [0u8; 32];
        let argon = argon2::Argon2::default();
        argon
            .hash_password_into(self.master_password.as_bytes(), salt, &mut key)
            .map_err(|e| anyhow::anyhow!("Argon2 key derivation failed: {e}"))?;
        Ok(key)
    }

    /// Encrypt a plaintext string into a binary payload.
    pub fn encrypt(&self, plaintext: &str) -> Result<Vec<u8>> {
        let mut salt = [0u8; SALT_LEN];
        rand::thread_rng().fill_bytes(&mut salt);

        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);

        let key = self.derive_key(&salt)?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| anyhow::anyhow!("Failed to initialize AES-GCM cipher: {e}"))?;
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| anyhow::anyhow!("Encryption failed: {e}"))?;

        let mut payload = Vec::with_capacity(MAGIC_HEADER.len() + SALT_LEN + NONCE_LEN + ciphertext.len());
        payload.extend_from_slice(MAGIC_HEADER);
        payload.extend_from_slice(&salt);
        payload.extend_from_slice(&nonce_bytes);
        payload.extend_from_slice(&ciphertext);

        Ok(payload)
    }

    /// Decrypt a binary payload into a plaintext string.
    /// If the payload does not have the magic header, it attempts to parse it as UTF-8 plaintext (seamless fallback).
    pub fn decrypt(&self, payload: &[u8]) -> Result<String> {
        if payload.len() < MAGIC_HEADER.len() || &payload[..MAGIC_HEADER.len()] != MAGIC_HEADER {
            // Not encrypted: fallback to UTF-8 plaintext
            return String::from_utf8(payload.to_vec())
                .context("File is not encrypted and contains invalid UTF-8");
        }

        let header_end = MAGIC_HEADER.len();
        let salt_end = header_end + SALT_LEN;
        let nonce_end = salt_end + NONCE_LEN;

        if payload.len() < nonce_end {
            bail!("Corrupted encrypted payload: payload too short");
        }

        let salt = &payload[header_end..salt_end];
        let nonce_bytes = &payload[salt_end..nonce_end];
        let ciphertext = &payload[nonce_end..];

        let key = self.derive_key(salt)?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| anyhow::anyhow!("Failed to initialize AES-GCM cipher: {e}"))?;
        let nonce = Nonce::from_slice(nonce_bytes);

        let decrypted_bytes = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| anyhow::anyhow!("Decryption failed: incorrect master password or corrupted file"))?;

        String::from_utf8(decrypted_bytes)
            .context("Decrypted content is not valid UTF-8 string")
    }
}

impl ConfigStorage for EncryptedStorage {
    fn read(&self, path: &Path) -> Result<String> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;
        self.decrypt(&bytes)
    }

    fn write(&self, path: &Path, content: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let encrypted_bytes = self.encrypt(content)?;
        let tmp_path = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
        std::fs::write(&tmp_path, encrypted_bytes)
            .with_context(|| format!("Failed to write encrypted file: {}", tmp_path.display()))?;

        #[cfg(windows)]
        {
            if path.exists() {
                let _ = std::fs::remove_file(path);
            }
        }
        std::fs::rename(&tmp_path, path)
            .with_context(|| format!("Failed to atomically rename encrypted file: {}", path.display()))?;
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

    #[test]
    fn test_encrypted_storage_roundtrip() {
        let storage = EncryptedStorage::new("correct-horse-battery-staple");
        let sample_toml = "[app_settings]\ntheme = \"dark\"\n";

        let encrypted = storage.encrypt(sample_toml).expect("Encryption failed");
        assert_ne!(encrypted, sample_toml.as_bytes());
        assert!(encrypted.starts_with(MAGIC_HEADER));

        let decrypted = storage.decrypt(&encrypted).expect("Decryption failed");
        assert_eq!(decrypted, sample_toml);
    }

    #[test]
    fn test_encrypted_storage_wrong_password() {
        let storage1 = EncryptedStorage::new("password-1");
        let storage2 = EncryptedStorage::new("password-2");
        let sample_toml = "sensitive config data";

        let encrypted = storage1.encrypt(sample_toml).unwrap();
        let result = storage2.decrypt(&encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn test_plaintext_fallback() {
        let storage = EncryptedStorage::new("master-pass");
        let plaintext_toml = "[app_settings]\ntheme = \"light\"\n";

        // Raw bytes without magic header
        let decrypted = storage.decrypt(plaintext_toml.as_bytes()).expect("Fallback failed");
        assert_eq!(decrypted, plaintext_toml);
    }
}
