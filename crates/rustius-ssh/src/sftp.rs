// SFTP file operations.
//
// This module will be fully implemented in Phase 4.

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Represents a file or directory entry from a remote SFTP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<String>,
    pub permissions: Option<String>,
}

/// SFTP client for file operations over an SSH channel.
pub struct SftpClient {
    // Will hold russh_sftp::client::SftpSession
}

impl SftpClient {
    /// List the contents of a remote directory.
    pub async fn list_dir(&self, _path: &str) -> Result<Vec<FileEntry>> {
        todo!("SFTP list_dir not yet implemented")
    }

    /// Download a file from the remote server.
    pub async fn download(&self, _remote_path: &str, _local_path: &str) -> Result<()> {
        todo!("SFTP download not yet implemented")
    }

    /// Upload a file to the remote server.
    pub async fn upload(&self, _local_path: &str, _remote_path: &str) -> Result<()> {
        todo!("SFTP upload not yet implemented")
    }

    /// Create a directory on the remote server.
    pub async fn mkdir(&self, _path: &str) -> Result<()> {
        todo!("SFTP mkdir not yet implemented")
    }

    /// Remove a file on the remote server.
    pub async fn remove(&self, _path: &str) -> Result<()> {
        todo!("SFTP remove not yet implemented")
    }

    /// Rename a file or directory on the remote server.
    pub async fn rename(&self, _from: &str, _to: &str) -> Result<()> {
        todo!("SFTP rename not yet implemented")
    }
}
