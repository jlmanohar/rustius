use anyhow::{Context, Result};
use russh::client::{self, Handler};
use russh::keys::PublicKeyOrCertificate;
use russh::Channel;
use rustius_core::connection::{AuthMethod, Connection};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

pub struct SshClientHandler {
    output_tx: mpsc::UnboundedSender<Vec<u8>>,
    exit_tx: mpsc::Sender<u32>,
}

impl Handler for SshClientHandler {
    type Error = anyhow::Error;

    fn check_server_key(
        &mut self,
        _server_public_key: &PublicKeyOrCertificate,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        async { Ok(true) }
    }

    fn data(
        &mut self,
        _channel: russh::ChannelId,
        data: &[u8],
        _session: &mut client::Session,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let _ = self.output_tx.send(data.to_vec());
        async { Ok(()) }
    }

    fn extended_data(
        &mut self,
        _channel: russh::ChannelId,
        ext: u32,
        data: &[u8],
        _session: &mut client::Session,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        // SSH_EXTENDED_DATA_STDERR is 1; ignore any duplicate/other stream types
        if ext == 1 {
            let _ = self.output_tx.send(data.to_vec());
        }
        async { Ok(()) }
    }

    fn channel_close(
        &mut self,
        _channel: russh::ChannelId,
        _session: &mut client::Session,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let _ = self.exit_tx.try_send(0);
        async { Ok(()) }
    }

    fn channel_eof(
        &mut self,
        _channel: russh::ChannelId,
        _session: &mut client::Session,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let _ = self.exit_tx.try_send(0);
        async { Ok(()) }
    }

    fn exit_status(
        &mut self,
        _channel: russh::ChannelId,
        exit_status: u32,
        _session: &mut client::Session,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let _ = self.exit_tx.try_send(exit_status);
        async { Ok(()) }
    }
}

pub struct SshSession {
    channel: Arc<tokio::sync::Mutex<Channel<client::Msg>>>,
}

impl SshSession {
    pub async fn connect(
        connection: &Connection,
        output_tx: mpsc::UnboundedSender<Vec<u8>>,
        exit_tx: mpsc::Sender<u32>,
    ) -> Result<Self> {
        let config = Arc::new(russh::client::Config::default());
        let handler = SshClientHandler { output_tx, exit_tx };

        let addr = format!("{}:{}", connection.host, connection.port);
        info!("Connecting SSH to {}", addr);

        let mut handle = russh::client::connect(config, (connection.host.as_str(), connection.port), handler)
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
                let res = handle.authenticate_publickey(username, key_with_alg).await?;
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
            .request_pty(
                true,
                "xterm-256color",
                80,
                24,
                0,
                0,
                &[],
            )
            .await?;

        channel.request_shell(true).await?;

        Ok(Self {
            channel: Arc::new(tokio::sync::Mutex::new(channel)),
        })
    }

    pub async fn write(&self, data: &[u8]) -> Result<()> {
        let ch = self.channel.lock().await;
        ch.data(data).await.map_err(|e| anyhow::anyhow!("Failed to write SSH data: {e}"))?;
        Ok(())
    }

    pub async fn resize(&self, cols: u32, rows: u32) -> Result<()> {
        let ch = self.channel.lock().await;
        ch.window_change(cols, rows, 0, 0)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to resize SSH window: {e}"))?;
        Ok(())
    }

    pub async fn disconnect(&self) -> Result<()> {
        let ch = self.channel.lock().await;
        ch.close().await.map_err(|e| anyhow::anyhow!("Failed to close SSH channel: {e}"))?;
        Ok(())
    }
}
