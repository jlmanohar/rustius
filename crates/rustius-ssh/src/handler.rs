use std::path::PathBuf;

use anyhow::{anyhow, Result};
use russh::client::Handler;
use russh::keys::PublicKeyOrCertificate;
use tracing::{info, warn};

/// Shared russh client handler providing TOFU (Trust-On-First-Use) host key verification.
///
/// Validates server host keys against either:
/// - A custom `known_hosts` path (used in PORTABLE mode to keep keys on the USB drive).
/// - The standard user profile `~/.ssh/known_hosts` (used in INSTALLED mode).
///
/// - If the key is already known and matches: accepts connection.
/// - If the host is unknown: automatically learns and appends to `known_hosts` (TOFU policy).
/// - If the key has changed: rejects connection immediately to protect against MITM attacks.
#[derive(Debug, Clone)]
pub struct RustiusSshHandler {
    pub host: String,
    pub port: u16,
    pub known_hosts_path: Option<PathBuf>,
}

impl RustiusSshHandler {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            known_hosts_path: None,
        }
    }

    pub fn with_known_hosts_path(
        host: impl Into<String>,
        port: u16,
        known_hosts_path: Option<PathBuf>,
    ) -> Self {
        Self {
            host: host.into(),
            port,
            known_hosts_path,
        }
    }
}

impl Handler for RustiusSshHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let public_key_cow = match server_public_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => std::borrow::Cow::Borrowed(key),
            PublicKeyOrCertificate::Certificate(c) => {
                std::borrow::Cow::Owned(russh::keys::PublicKey::new(c.public_key().clone(), ""))
            }
        };
        let public_key = public_key_cow.as_ref();

        let check_res = if let Some(ref path) = self.known_hosts_path {
            russh::keys::known_hosts::check_known_hosts_path(&self.host, self.port, public_key, path)
        } else {
            russh::keys::known_hosts::check_known_hosts(&self.host, self.port, public_key)
        };

        match check_res {
            Ok(true) => {
                info!("SSH host key verified for {}:{}", self.host, self.port);
                Ok(true)
            }
            Ok(false) => {
                info!("New SSH host detected for {}:{}. Learning key (TOFU policy)...", self.host, self.port);
                let learn_res = if let Some(ref path) = self.known_hosts_path {
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    russh::keys::known_hosts::learn_known_hosts_path(&self.host, self.port, public_key, path)
                } else {
                    russh::keys::known_hosts::learn_known_hosts(&self.host, self.port, public_key)
                };

                if let Err(e) = learn_res {
                    warn!("Failed to append {}:{} to known_hosts: {}", self.host, self.port, e);
                } else {
                    info!("Successfully recorded host key for {}:{} in known_hosts", self.host, self.port);
                }
                Ok(true)
            }
            Err(e) => {
                warn!("CRITICAL: SSH host key verification failed for {}:{}: {}", self.host, self.port, e);
                Err(anyhow!("SSH host key verification failed for {}:{}: {}. Possible Man-In-The-Middle (MITM) attack!", self.host, self.port, e))
            }
        }
    }
}
