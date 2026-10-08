// RDP client session management powered by IronRDP via standalone worker.
// Handles X.224, TLS, NLA (CredSSP/NTLM), bitmap decoding, and input dispatching
// in an isolated binary architecture.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;
use tracing::{debug, info};

const MAGIC_FRAME: [u8; 4] = [0x52, 0x44, 0x50, 0x46]; // "RDPF"
const MAGIC_CTRL: [u8; 4] = [0x52, 0x44, 0x50, 0x43];  // "RDPC"
const MAGIC_INPUT: [u8; 4] = [0x52, 0x44, 0x50, 0x49]; // "RDPI"

use base64::Engine;

/// Decoded frame data to send to the frontend Canvas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpFrame {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// Base64 encoded RGBA pixel data for this frame region (size = width * height * 4).
    pub b64: String,
}

/// RDP input events forwarded from the frontend Canvas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RdpInputEvent {
    Key { key_code: u16, pressed: bool },
    Mouse { x: u32, y: u32, buttons: u8, wheel: i8 },
}

/// Manages an active RDP session.
pub struct RdpSession {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub width: u32,
    pub height: u32,
    is_connected: Arc<AtomicBool>,
    input_tx: mpsc::UnboundedSender<RdpInputEvent>,
    abort_handle: parking_lot::Mutex<Option<AbortHandle>>,
}

fn find_worker_executable() -> Result<std::path::PathBuf> {
    // 1. Next to current exe
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(parent) = exe_path.parent() {
            let next_to_exe = parent.join("rustius-rdp-worker.exe");
            if next_to_exe.exists() {
                return Ok(next_to_exe);
            }
            let in_resources = parent.join("resources").join("rustius-rdp-worker.exe");
            if in_resources.exists() {
                return Ok(in_resources);
            }
            let next_to_exe_no_ext = parent.join("rustius-rdp-worker");
            if next_to_exe_no_ext.exists() {
                return Ok(next_to_exe_no_ext);
            }
        }
    }

    // 2. In workers/rustius-rdp-worker/target/{debug,release}
    let candidates = [
        "workers/rustius-rdp-worker/target/debug/rustius-rdp-worker.exe",
        "workers/rustius-rdp-worker/target/release/rustius-rdp-worker.exe",
        "../workers/rustius-rdp-worker/target/debug/rustius-rdp-worker.exe",
        "../workers/rustius-rdp-worker/target/release/rustius-rdp-worker.exe",
        "workers/rustius-rdp-worker/target/debug/rustius-rdp-worker",
        "workers/rustius-rdp-worker/target/release/rustius-rdp-worker",
    ];

    for candidate in candidates {
        let path = std::path::PathBuf::from(candidate);
        if path.exists() {
            return Ok(path.canonicalize().unwrap_or(path));
        }
    }

    Err(anyhow!(
        "rustius-rdp-worker executable not found. Please ensure workers/rustius-rdp-worker is built."
    ))
}

impl RdpSession {
    /// Establish an RDP connection via standalone IronRDP worker and start frame streaming.
    pub async fn connect(
        host: &str,
        port: u16,
        username: &str,
        password: &str,
        width: u32,
        height: u32,
        frame_tx: mpsc::UnboundedSender<RdpFrame>,
    ) -> Result<Self> {
        info!("Initiating RDP connection to {}:{} (resolution: {}x{})", host, port, width, height);

        let worker_bin = find_worker_executable().context("Locating RDP worker binary")?;
        info!("Launching RDP worker from: {}", worker_bin.display());

        let mut cmd = Command::new(&worker_bin);
        cmd.arg("--host").arg(host)
            .arg("--port").arg(port.to_string())
            .arg("--username").arg(username)
            .arg("--password").arg(password)
            .arg("--width").arg(width.to_string())
            .arg("--height").arg(height.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW = 0x08000000 to prevent cmd prompt popup
            cmd.creation_flags(0x08000000);
        }

        let mut child: Child = cmd.spawn().context("Failed to spawn rustius-rdp-worker process")?;

        let mut child_stdout = child.stdout.take().context("Failed to capture worker stdout")?;
        let mut child_stdin = child.stdin.take().context("Failed to capture worker stdin")?;

        // Wait for worker to complete X.224, TLS, and NLA handshake (20s timeout)
        let mut first_magic = [0u8; 4];
        let init_res = tokio::time::timeout(tokio::time::Duration::from_secs(20), async {
            child_stdout.read_exact(&mut first_magic).await
        })
        .await;

        match init_res {
            Ok(Ok(_)) => {
                if first_magic == MAGIC_CTRL {
                    let mut len_buf = [0u8; 4];
                    child_stdout.read_exact(&mut len_buf).await.context("Failed reading ctrl len")?;
                    let len = u32::from_le_bytes(len_buf) as usize;
                    let mut json_buf = vec![0u8; len];
                    child_stdout.read_exact(&mut json_buf).await.context("Failed reading ctrl payload")?;
                    let msg = String::from_utf8(json_buf).context("Invalid UTF-8 in worker ctrl msg")?;
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&msg) {
                        if val.get("type").and_then(|v| v.as_str()) == Some("error") {
                            let err_msg = val.get("message").and_then(|v| v.as_str()).unwrap_or("Handshake error");
                            let _ = child.kill().await;
                            return Err(anyhow!("RDP connection failed: {}", err_msg));
                        }
                    }
                } else if first_magic == MAGIC_FRAME {
                    let mut hdr = [0u8; 20];
                    child_stdout.read_exact(&mut hdr).await.context("Failed reading initial frame header")?;
                    let x = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
                    let y = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
                    let w = u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]);
                    let h = u32::from_le_bytes([hdr[12], hdr[13], hdr[14], hdr[15]]);
                    let data_len = u32::from_le_bytes([hdr[16], hdr[17], hdr[18], hdr[19]]) as usize;
                    let mut data = vec![0u8; data_len];
                    child_stdout.read_exact(&mut data).await.context("Failed reading initial frame data")?;
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                    let _ = frame_tx.send(RdpFrame { x, y, width: w, height: h, b64 });
                }
            }
            Ok(Err(e)) => {
                let _ = child.kill().await;
                return Err(anyhow!("RDP worker exited during handshake: {}", e));
            }
            Err(_) => {
                let _ = child.kill().await;
                return Err(anyhow!("RDP connection timed out waiting for server"));
            }
        }

        let is_connected = Arc::new(AtomicBool::new(true));
        let is_connected_reader = is_connected.clone();
        let is_connected_writer = is_connected.clone();

        let (input_tx, mut input_rx) = mpsc::unbounded_channel::<RdpInputEvent>();

        let host_name = host.to_string();

        // Background worker supervisor task
        let worker_join_handle = tokio::spawn(async move {
            let mut read_magic = [0u8; 4];

            // 1. Frame reader loop
            let reader_task = async {
                while is_connected_reader.load(Ordering::SeqCst) {
                    if let Err(e) = child_stdout.read_exact(&mut read_magic).await {
                        debug!("Worker stdout stream closed: {:?}", e);
                        break;
                    }

                    if read_magic == MAGIC_FRAME {
                        let mut hdr = [0u8; 20];
                        if child_stdout.read_exact(&mut hdr).await.is_err() {
                            break;
                        }
                        let x = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
                        let y = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
                        let w = u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]);
                        let h = u32::from_le_bytes([hdr[12], hdr[13], hdr[14], hdr[15]]);
                        let data_len = u32::from_le_bytes([hdr[16], hdr[17], hdr[18], hdr[19]]) as usize;

                        let mut data = vec![0u8; data_len];
                        if child_stdout.read_exact(&mut data).await.is_err() {
                            break;
                        }

                        let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                        let _ = frame_tx.send(RdpFrame {
                            x,
                            y,
                            width: w,
                            height: h,
                            b64,
                        });
                    } else if read_magic == MAGIC_CTRL {
                        let mut len_buf = [0u8; 4];
                        if child_stdout.read_exact(&mut len_buf).await.is_err() {
                            break;
                        }
                        let len = u32::from_le_bytes(len_buf) as usize;
                        let mut json_buf = vec![0u8; len];
                        if child_stdout.read_exact(&mut json_buf).await.is_err() {
                            break;
                        }
                        if let Ok(msg) = String::from_utf8(json_buf) {
                            debug!("RDP worker status: {}", msg);
                        }
                    }
                }
            };

            // 2. Input writer loop
            let writer_task = async {
                while let Some(event) = input_rx.recv().await {
                    if !is_connected_writer.load(Ordering::SeqCst) {
                        break;
                    }
                    match event {
                        RdpInputEvent::Key { key_code, pressed } => {
                            let mut payload = Vec::with_capacity(8);
                            payload.extend_from_slice(&MAGIC_INPUT);
                            payload.push(1); // Key event
                            payload.extend_from_slice(&key_code.to_le_bytes());
                            payload.push(if pressed { 1 } else { 0 });
                            if child_stdin.write_all(&payload).await.is_err() || child_stdin.flush().await.is_err() {
                                break;
                            }
                        }
                        RdpInputEvent::Mouse { x, y, buttons, wheel } => {
                            let mut payload = Vec::with_capacity(12);
                            payload.extend_from_slice(&MAGIC_INPUT);
                            payload.push(2); // Mouse event
                            payload.extend_from_slice(&(x as u16).to_le_bytes());
                            payload.extend_from_slice(&(y as u16).to_le_bytes());
                            payload.push(buttons);
                            payload.push(wheel as u8);
                            if child_stdin.write_all(&payload).await.is_err() || child_stdin.flush().await.is_err() {
                                break;
                            }
                        }
                    }
                }
            };

            tokio::select! {
                _ = reader_task => {},
                _ = writer_task => {},
                _ = child.wait() => {},
            }

            is_connected_reader.store(false, Ordering::SeqCst);
            let _ = child.kill().await;
            info!("RDP worker session terminated for {}", host_name);
        });

        Ok(Self {
            host: host.to_string(),
            port,
            username: username.to_string(),
            width,
            height,
            is_connected,
            input_tx,
            abort_handle: parking_lot::Mutex::new(Some(worker_join_handle.abort_handle())),
        })
    }

    /// Forward a keyboard event to the active RDP session.
    pub fn send_key_event(&self, key_code: u16, pressed: bool) -> Result<()> {
        if !self.is_connected.load(Ordering::SeqCst) {
            return Err(anyhow!("RDP session is not connected"));
        }
        self.input_tx
            .send(RdpInputEvent::Key { key_code, pressed })
            .map_err(|e| anyhow!("Failed to send key event: {}", e))
    }

    /// Forward a mouse event (coordinates, buttons, wheel) to the active RDP session.
    pub fn send_mouse_event(&self, x: u32, y: u32, buttons: u8, wheel: i8) -> Result<()> {
        if !self.is_connected.load(Ordering::SeqCst) {
            return Err(anyhow!("RDP session is not connected"));
        }
        self.input_tx
            .send(RdpInputEvent::Mouse { x, y, buttons, wheel })
            .map_err(|e| anyhow!("Failed to send mouse event: {}", e))
    }

    /// Gracefully disconnect and abort the RDP session.
    pub fn disconnect(&self) {
        self.is_connected.store(false, Ordering::SeqCst);
        if let Some(handle) = self.abort_handle.lock().take() {
            handle.abort();
        }
        info!("Disconnected RDP session to {}:{}", self.host, self.port);
    }

    pub fn is_alive(&self) -> bool {
        self.is_connected.load(Ordering::SeqCst)
    }
}

impl Drop for RdpSession {
    fn drop(&mut self) {
        self.disconnect();
    }
}
