use std::sync::Arc;

use anyhow::{Context, Result};
use russh::client::{self, Handler};
use russh::keys::PublicKeyOrCertificate;
use tokio::sync::mpsc;
use tracing::info;

use rustius_core::connection::{AuthMethod, Connection};

/// Client handler implementation for russh callbacks.
///
/// In russh, protocol-level negotiation (such as TOFU host key verification)
/// is dispatched through this trait implementation. Channel-level data and flow control
/// are consumed directly from the channel read half to prevent backpressure stalls.
pub struct SshClientHandler;

impl Handler for SshClientHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Active SSH pseudo-terminal session.
///
/// Holds the writing half of the interactive SSH channel and the client connection handle.
/// Incoming terminal output and control messages are consumed in a dedicated background task
/// that drains the channel's internal packet queue, preventing backpressure deadlocks.
pub struct SshSession {
    write_half: russh::ChannelWriteHalf<client::Msg>,
    _handle: client::Handle<SshClientHandler>,
}

impl SshSession {
    /// Connect to remote SSH host, authenticate, allocate a PTY, and spawn an interactive shell.
    ///
    /// - `output_tx`: Unbounded sender delivering raw remote terminal bytes to the event loop.
    /// - `exit_tx`: Sender signaling remote process exit or channel EOF with the exit code.
    pub async fn connect(
        connection: &Connection,
        output_tx: mpsc::UnboundedSender<Vec<u8>>,
        exit_tx: mpsc::Sender<u32>,
    ) -> Result<Self> {
        let config = Arc::new(russh::client::Config::default());
        let handler = SshClientHandler;

        let addr = format!("{}:{}", connection.host, connection.port);
        info!("Connecting SSH to {}", addr);

        let mut handle =
            russh::client::connect(config, (connection.host.as_str(), connection.port), handler)
                .await
                .context(format!("Failed to connect to {}", addr))?;

        // Authenticate
        match &connection.auth {
            AuthMethod::Password { username, password } => {
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
            } => {
                let key = russh::keys::load_secret_key(key_path, passphrase.as_deref())
                    .context(format!("Failed to load key from {}", key_path))?;
                let key_with_alg = russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), None);
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

        // Open channel and request PTY + shell
        let channel = handle.channel_open_session().await?;

        channel
            .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[])
            .await?;

        channel.request_shell(true).await?;

        let (mut read_half, write_half) = channel.split();

        // Spawn a background task to continuously drain read_half.
        // Critical: russh queues every incoming packet into an internal mpsc channel
        // with capacity 100. If read_half is not continuously consumed via `wait()`,
        // the queue fills up after 100 packets, blocking the russh connection event loop
        // and freezing all terminal I/O.
        tokio::spawn(async move {
            while let Some(msg) = read_half.wait().await {
                match msg {
                    russh::ChannelMsg::Data { data } => {
                        let _ = output_tx.send(data.to_vec());
                    }
                    russh::ChannelMsg::ExtendedData { data, ext } => {
                        // ext == 1 is STDERR
                        if ext == 1 {
                            let _ = output_tx.send(data.to_vec());
                        }
                    }
                    russh::ChannelMsg::ExitStatus { exit_status } => {
                        let _ = exit_tx.send(exit_status).await;
                    }
                    russh::ChannelMsg::Eof | russh::ChannelMsg::Close => {
                        let _ = exit_tx.try_send(0);
                    }
                    _ => {}
                }
            }
            let _ = exit_tx.try_send(0);
        });

        Ok(Self {
            write_half,
            _handle: handle,
        })
    }

    /// Send raw input data (e.g. user keystrokes) to the remote shell.
    pub async fn write(&self, data: &[u8]) -> Result<()> {
        self.write_half
            .data(data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write SSH data: {e}"))?;
        Ok(())
    }

    /// Notify the remote PTY that the terminal viewport dimensions have changed.
    pub async fn resize(&self, cols: u32, rows: u32) -> Result<()> {
        self.write_half
            .window_change(cols, rows, 0, 0)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to resize SSH window: {e}"))?;
        Ok(())
    }

    /// Close the interactive SSH channel.
    pub async fn disconnect(&self) -> Result<()> {
        self.write_half
            .close()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to close SSH channel: {e}"))?;
        Ok(())
    }
}
