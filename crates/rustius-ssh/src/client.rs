use std::sync::Arc;

use anyhow::{Context, Result};
use russh::client;
use tokio::sync::mpsc;
use tracing::info;

use rustius_core::connection::Connection;

use crate::auth::authenticate;
use crate::handler::RustiusSshHandler;

/// Active SSH pseudo-terminal session.
///
/// Holds the writing half of the interactive SSH channel and the client connection handle.
/// Incoming terminal output and control messages are consumed in a dedicated background task
/// that drains the channel's internal packet queue, preventing backpressure deadlocks.
pub struct SshSession {
    write_half: russh::ChannelWriteHalf<client::Msg>,
    _handle: client::Handle<RustiusSshHandler>,
}

impl SshSession {
    /// Connect to remote SSH host, authenticate, allocate a PTY, and spawn an interactive shell.
    ///
    /// - `output_tx`: Unbounded sender delivering raw remote terminal bytes to the event loop.
    /// - `exit_tx`: Sender signaling remote process exit or channel EOF with the exit code.
    pub async fn connect(
        connection: &Connection,
        known_hosts_path: Option<std::path::PathBuf>,
        output_tx: mpsc::UnboundedSender<Vec<u8>>,
        exit_tx: mpsc::Sender<u32>,
    ) -> Result<Self> {
        let keepalive_secs = connection
            .ssh_config
            .as_ref()
            .and_then(|c| c.keepalive_interval)
            .unwrap_or(5);
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
            "Connecting SSH to {} (keepalive: {}s, timeout: {}s)",
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

            Ok::<_, anyhow::Error>(handle)
        };

        let handle =
            tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), connect_fut)
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "Connection to {} timed out after {} seconds (host unreachable or offline)",
                        addr,
                        timeout_secs
                    )
                })?
                .context(format!("Failed to connect to {}", addr))?;

        // Open channel and request PTY + shell with 5-second timeout
        let channel = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            handle.channel_open_session(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Timed out opening SSH channel"))?
        .context("Failed to open SSH session channel")?;

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
            let mut received_exit_status = false;
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
                        received_exit_status = true;
                        let _ = exit_tx.send(exit_status).await;
                    }
                    russh::ChannelMsg::Eof | russh::ChannelMsg::Close if !received_exit_status => {
                        received_exit_status = true;
                        let _ = exit_tx.try_send(0);
                    }
                    _ => {}
                }
            }
            if !received_exit_status {
                // Connection terminated abruptly midway (e.g. remote host shut down, keepalive timeout, broken pipe)
                let _ = exit_tx.send(255).await;
            }
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
        let _ = self.write_half.close().await;
        let _ = self
            ._handle
            .disconnect(russh::Disconnect::ByApplication, "", "")
            .await;
        Ok(())
    }
}
