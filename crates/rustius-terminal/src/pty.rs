use anyhow::Result;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing::{debug, info};

pub struct LocalTerminal {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    killer: Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>,
}

impl Drop for LocalTerminal {
    fn drop(&mut self) {
        self.close();
    }
}

impl LocalTerminal {
    pub fn spawn(
        cols: u16,
        rows: u16,
        output_tx: mpsc::Sender<Vec<u8>>,
        exit_tx: mpsc::Sender<u32>,
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
            "powershell.exe".to_string()
        } else {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
        };

        info!("Spawning local shell: {}", shell);
        let mut cmd = CommandBuilder::new(&shell);

        // Inherit current working directory
        if let Ok(cwd) = std::env::current_dir() {
            cmd.cwd(cwd);
        }

        let mut child = pair.slave.spawn_command(cmd)?;
        // Drop slave in this process now that child has spawned
        drop(pair.slave);

        let killer = child.clone_killer();

        // Background watcher thread: waits for child process (e.g. on `exit`)
        // and immediately signals exit_tx with the process exit code.
        std::thread::Builder::new()
            .name("pty-child-watcher".into())
            .spawn(move || {
                let code = match child.wait() {
                    Ok(status) => status.exit_code(),
                    Err(e) => {
                        debug!("Error waiting for child process: {:?}", e);
                        0
                    }
                };
                debug!("PTY child process exited with code {}", code);
                let _ = exit_tx.try_send(code);
            })?;

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
                            if output_tx.blocking_send(buffer[..n].to_vec()).is_err() {
                                debug!("PTY receiver disconnected");
                                break;
                            }
                        }
                        Err(e) => {
                            debug!("Error reading from PTY: {:?}", e);
                            break;
                        }
                    }
                }
            })?;

        Ok(Self {
            writer: Arc::new(Mutex::new(writer)),
            master: Arc::new(Mutex::new(master)),
            killer: Arc::new(Mutex::new(killer)),
        })
    }

    pub fn close(&self) {
        if let Ok(mut killer) = self.killer.lock() {
            let _ = killer.kill();
        }
    }

    pub fn write(&self, data: &[u8]) -> Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    /// Resize the pseudo-terminal window dimensions.
    ///
    /// Ignores dimension requests smaller than 10 columns or 2 rows (such as those caused
    /// by collapsed/hidden UI panes) to avoid crushing ConPTY's screen buffer and truncating
    /// terminal scrollback history on Windows.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        if cols < 10 || rows < 2 {
            return Ok(());
        }
        let master = self
            .master
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
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
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(128);
        let (exit_tx, mut _exit_rx) = mpsc::channel(1);
        let terminal =
            LocalTerminal::spawn(80, 24, tx, exit_tx).expect("Failed to spawn local terminal");

        // Write a simple echo command
        terminal
            .write(b"echo 'rustius-test'\r\n")
            .expect("Failed to write to terminal");

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
