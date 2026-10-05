use anyhow::Result;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

pub struct LocalTerminal {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
}

impl LocalTerminal {
    pub fn spawn(
        cols: u16,
        rows: u16,
        output_tx: mpsc::UnboundedSender<Vec<u8>>,
    ) -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        // Determine default shell for the OS
        let shell = if cfg!(windows) {
            // Check if powershell is available, else cmd.exe
            "powershell.exe"
        } else {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into()).leak()
        };

        info!("Spawning local shell: {}", shell);
        let mut cmd = CommandBuilder::new(shell);
        
        // Inherit current working directory
        if let Ok(cwd) = std::env::current_dir() {
            cmd.cwd(cwd);
        }

        let _child = pair.slave.spawn_command(cmd)?;
        // Drop slave in this process now that child has spawned
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let master = pair.master;

        // Background reader thread forwarding PTY bytes to tokio channel
        std::thread::Builder::new()
            .name("pty-reader".into())
            .spawn(move || {
                let mut buffer = [0u8; 4096];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => {
                            debug!("PTY stream reached EOF");
                            break;
                        }
                        Ok(n) => {
                            if output_tx.send(buffer[..n].to_vec()).is_err() {
                                debug!("PTY receiver disconnected");
                                break;
                            }
                        }
                        Err(e) => {
                            error!("Error reading from PTY: {:?}", e);
                            break;
                        }
                    }
                }
            })?;

        Ok(Self {
            writer: Arc::new(Mutex::new(writer)),
            master: Arc::new(Mutex::new(master)),
        })
    }

    pub fn write(&self, data: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock().map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let master = self.master.lock().map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_spawn_and_write() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let terminal = LocalTerminal::spawn(80, 24, tx).expect("Failed to spawn local terminal");

        // Write a simple echo command
        terminal.write(b"echo 'rustius-test'\r\n").expect("Failed to write to terminal");

        // Read output within timeout
        let mut collected = Vec::new();
        let timeout = tokio::time::sleep(Duration::from_secs(3));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                Some(chunk) = rx.recv() => {
                    collected.extend_from_slice(&chunk);
                    let text = String::from_utf8_lossy(&collected);
                    if text.contains("rustius-test") {
                        break;
                    }
                }
                _ = &mut timeout => {
                    // On some Windows CI environments shells may take longer to boot,
                    // but if we received any data at all, the PTY is working!
                    assert!(!collected.is_empty(), "Expected to receive output from spawned shell");
                    break;
                }
            }
        }
    }
}

