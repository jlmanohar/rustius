use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::DateTime;
use russh::client;
use russh::keys::PublicKeyOrCertificate;
use russh_sftp::client::SftpSession;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tracing::info;

use rustius_core::connection::{AuthMethod, Connection};

/// Represents a file or directory entry from an SFTP server or local filesystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<String>,
    pub permissions: Option<String>,
}

/// Minimal SSH client handler for the dedicated SFTP subsystem session.
///
/// Unlike the terminal SSH session, SFTP operates as an independent SSH subsystem channel
/// whose wire frames are managed by `russh_sftp::client::SftpSession`.
struct SftpClientHandler;

impl client::Handler for SftpClientHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// SFTP client for file operations over an SSH channel.
pub struct SftpClient {
    session: SftpSession,
    _handle: client::Handle<SftpClientHandler>,
}

impl SftpClient {
    /// Connect to remote host and initialize SFTP subsystem.
    pub async fn connect(connection: &Connection) -> Result<Self> {
        let config = Arc::new(russh::client::Config {
            keepalive_interval: Some(std::time::Duration::from_secs(5)),
            keepalive_max: 3,
            inactivity_timeout: Some(std::time::Duration::from_secs(3600)),
            ..Default::default()
        });
        let handler = SftpClientHandler;
        let addr = format!("{}:{}", connection.host, connection.port);
        info!("Connecting SFTP subsystem to {}", addr);

        let connect_fut = async {
            let mut handle = russh::client::connect(
                config,
                (connection.host.as_str(), connection.port),
                handler,
            )
            .await
            .context(format!("Failed to connect to {}", addr))?;

            // Authenticate
            match &connection.auth {
                AuthMethod::Password { username, password, .. } => {
                    let pass = password.as_deref().unwrap_or("");
                    let res = handle.authenticate_password(username, pass).await?;
                    if !matches!(res, russh::client::AuthResult::Success) {
                        anyhow::bail!("Password authentication failed for user {}", username);
                    }
                }
                AuthMethod::KeyFile {
                    username,
                    key_path,
                    passphrase,
                    ..
                } => {
                    let key = russh::keys::load_secret_key(key_path, passphrase.as_deref())
                        .context(format!("Failed to load key from {}", key_path))?;
                    let key_with_alg =
                        russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), None);
                    let res = handle
                        .authenticate_publickey(username, key_with_alg)
                        .await?;
                    if !matches!(res, russh::client::AuthResult::Success) {
                        anyhow::bail!("Public key authentication failed for user {}", username);
                    }
                }
                AuthMethod::Agent { .. } => {
                    anyhow::bail!("SSH agent authentication not yet implemented");
                }
            }

            let channel = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                handle.channel_open_session(),
            )
            .await
            .map_err(|_| anyhow::anyhow!("Timed out opening SFTP channel"))?
            .context("Failed to open channel for SFTP")?;

            channel.request_subsystem(false, "sftp").await?;
            let stream = channel.into_stream();
            let session = SftpSession::new(stream).await?;

            Ok(Self {
                session,
                _handle: handle,
            })
        };

        tokio::time::timeout(std::time::Duration::from_secs(12), connect_fut)
            .await
            .map_err(|_| anyhow::anyhow!("SFTP connection to {} timed out after 12 seconds", addr))?
    }

    /// Disconnect and terminate the SFTP session.
    pub async fn disconnect(&self) -> Result<()> {
        let _ = self
            ._handle
            .disconnect(russh::Disconnect::ByApplication, "", "")
            .await;
        Ok(())
    }

    /// Resolve canonical path (e.g. "." resolves to remote user home directory)
    pub async fn canonicalize(&self, path: &str) -> Result<String> {
        let resolved = self.session.canonicalize(path).await?;
        Ok(resolved)
    }

    /// List directory contents.
    pub async fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>> {
        let entries = self.session.read_dir(path).await?;
        let mut results = Vec::new();

        for entry in entries {
            let name = entry.file_name();
            // Skip . and ..
            if name == "." || name == ".." {
                continue;
            }

            let is_dir = entry.file_type().is_dir();
            let meta = entry.metadata();
            let size = meta.size.unwrap_or(0);

            let permissions = meta.permissions.map(|p| format!("{:04o}", p & 0o7777));
            let modified = meta.mtime.and_then(|m| {
                DateTime::from_timestamp(m as i64, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            });

            // Ensure proper POSIX path formatting
            let full_path = if path == "/" {
                format!("/{}", name)
            } else if path.ends_with('/') {
                format!("{}{}", path, name)
            } else {
                format!("{}/{}", path, name)
            };

            results.push(FileEntry {
                name,
                path: full_path,
                is_dir,
                size,
                modified,
                permissions,
            });
        }

        // Sort directories first, then alphabetical by name
        results.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        Ok(results)
    }

    /// Download a remote file to a local destination path.
    pub async fn download(&self, remote_path: &str, local_path: &str) -> Result<()> {
        let mut remote_file = self.session.open(remote_path).await?;

        // Ensure local parent dir exists
        let local_p = std::path::Path::new(local_path);
        if let Some(parent) = local_p.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut local_file = tokio::fs::File::create(local_path).await?;
        tokio::io::copy(&mut remote_file, &mut local_file).await?;
        local_file.flush().await?;
        Ok(())
    }

    /// Read all bytes of a remote file into memory.
    ///
    /// Used by frontend drag-and-drop / viewer operations where data is passed directly
    /// over Tauri IPC without writing to an intermediate local staging file.
    pub async fn download_bytes(&self, remote_path: &str) -> Result<Vec<u8>> {
        let mut remote_file = self.session.open(remote_path).await?;
        let mut buffer = Vec::new();
        tokio::io::copy(&mut remote_file, &mut buffer).await?;
        Ok(buffer)
    }

    /// Upload a local file to a remote destination path.
    pub async fn upload(&self, local_path: &str, remote_path: &str) -> Result<()> {
        let mut local_file = tokio::fs::File::open(local_path).await?;
        let mut remote_file = self.session.create(remote_path).await?;
        tokio::io::copy(&mut local_file, &mut remote_file).await?;
        remote_file.flush().await?;
        Ok(())
    }

    /// Upload a byte buffer directly to a remote destination path.
    ///
    /// Enables direct upload of files dropped onto the SFTP sidebar from the browser webview,
    /// streaming the in-memory payload without requiring pre-saving to disk.
    pub async fn upload_bytes(&self, data: &[u8], remote_path: &str) -> Result<()> {
        let mut remote_file = self.session.create(remote_path).await?;
        let mut cursor = std::io::Cursor::new(data);
        tokio::io::copy(&mut cursor, &mut remote_file).await?;
        remote_file.flush().await?;
        Ok(())
    }

    /// Create a remote directory.
    pub async fn mkdir(&self, path: &str) -> Result<()> {
        self.session.create_dir(path).await?;
        Ok(())
    }

    /// Remove a remote file.
    pub async fn remove_file(&self, path: &str) -> Result<()> {
        self.session.remove_file(path).await?;
        Ok(())
    }

    /// Remove a remote directory.
    pub async fn remove_dir(&self, path: &str) -> Result<()> {
        self.session.remove_dir(path).await?;
        Ok(())
    }

    /// Rename a remote file or directory.
    pub async fn rename(&self, from: &str, to: &str) -> Result<()> {
        self.session.rename(from, to).await?;
        Ok(())
    }
}
