use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::DateTime;
use russh::client;
use russh_sftp::client::SftpSession;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tracing::info;

use rustius_core::connection::Connection;

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

use crate::auth::authenticate;
use crate::handler::RustiusSshHandler;

/// Maximum payload size allowed for in-memory buffer transfer (50 MB).
pub const MAX_IN_MEMORY_FILE_BYTES: u64 = 50 * 1024 * 1024;

/// SFTP client for file operations over an SSH channel.
pub struct SftpClient {
    session: SftpSession,
    _handle: client::Handle<RustiusSshHandler>,
}

impl SftpClient {
    /// Connect to remote host and initialize SFTP subsystem.
    pub async fn connect(
        connection: &Connection,
        known_hosts_path: Option<std::path::PathBuf>,
    ) -> Result<Self> {
        let keepalive_secs = connection
            .ssh_config
            .as_ref()
            .and_then(|c| c.keepalive_interval)
            .unwrap_or(30);
        let timeout_secs = connection
            .ssh_config
            .as_ref()
            .and_then(|c| c.connect_timeout_secs)
            .unwrap_or(12);

        let config = Arc::new(russh::client::Config {
            keepalive_interval: Some(std::time::Duration::from_secs(keepalive_secs)),
            keepalive_max: 3,
            inactivity_timeout: Some(std::time::Duration::from_secs(3600)),
            ..Default::default()
        });
        let handler = RustiusSshHandler::with_known_hosts_path(
            &connection.host,
            connection.port,
            known_hosts_path,
        );
        let addr = format!("{}:{}", connection.host, connection.port);
        info!(
            "Connecting SFTP subsystem to {} (keepalive: {}s, timeout: {}s)",
            addr, keepalive_secs, timeout_secs
        );

        let connect_fut = async {
            let mut handle = russh::client::connect(
                config,
                (connection.host.as_str(), connection.port),
                handler,
            )
            .await
            .context(format!("Failed to connect to {}", addr))?;

            // Authenticate using shared helper
            authenticate(&mut handle, &connection.auth).await?;

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

        tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), connect_fut)
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "SFTP connection to {} timed out after {} seconds",
                    addr,
                    timeout_secs
                )
            })?
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
        sort_file_entries(&mut results);

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

        let part_path = format!("{local_path}.part");
        let mut local_file = tokio::fs::File::create(&part_path).await?;
        if let Err(e) = tokio::io::copy(&mut remote_file, &mut local_file).await {
            drop(local_file);
            let _ = tokio::fs::remove_file(&part_path).await;
            return Err(e.into());
        }
        local_file.flush().await?;
        drop(local_file);

        #[cfg(windows)]
        {
            if std::path::Path::new(local_path).exists() {
                let _ = tokio::fs::remove_file(local_path).await;
            }
        }
        tokio::fs::rename(&part_path, local_path).await?;
        Ok(())
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
    /// Enforces a 50MB safety limit to protect against IPC buffer exhaustion.
    pub async fn upload_bytes(&self, data: &[u8], remote_path: &str) -> Result<()> {
        if data.len() as u64 > MAX_IN_MEMORY_FILE_BYTES {
            let size_mb = data.len() as f64 / (1024.0 * 1024.0);
            anyhow::bail!(
                "Payload is too large ({:.1} MB) for in-memory upload. Maximum allowed is 50 MB. Please upload from disk instead.",
                size_mb
            );
        }
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

/// Sort directory entries: directories first, then alphabetically case-insensitive.
pub fn sort_file_entries(entries: &mut [FileEntry]) {
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sort_file_entries() {
        let mut entries = vec![
            FileEntry {
                name: "zebra.txt".into(),
                path: "/zebra.txt".into(),
                is_dir: false,
                size: 100,
                modified: None,
                permissions: None,
            },
            FileEntry {
                name: "alpha".into(),
                path: "/alpha".into(),
                is_dir: true,
                size: 4096,
                modified: None,
                permissions: None,
            },
            FileEntry {
                name: "beta.txt".into(),
                path: "/beta.txt".into(),
                is_dir: false,
                size: 200,
                modified: None,
                permissions: None,
            },
            FileEntry {
                name: "Gamma".into(),
                path: "/Gamma".into(),
                is_dir: true,
                size: 4096,
                modified: None,
                permissions: None,
            },
        ];

        sort_file_entries(&mut entries);

        // Folders first (case-insensitive: alpha, Gamma), then files (beta.txt, zebra.txt)
        assert_eq!(entries[0].name, "alpha");
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].name, "Gamma");
        assert!(entries[1].is_dir);
        assert_eq!(entries[2].name, "beta.txt");
        assert!(!entries[2].is_dir);
        assert_eq!(entries[3].name, "zebra.txt");
        assert!(!entries[3].is_dir);
    }
}
