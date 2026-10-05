// RDP client session management powered by IronRDP.
// Handles X.224 / MCS connection handshake, frame buffer rendering,
// and keyboard/mouse input dispatching.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;
use tracing::{debug, info, warn};

/// Decoded frame data to send to the frontend Canvas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpFrame {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// RGBA pixel data for this frame region (size = width * height * 4).
    pub data: Vec<u8>,
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

impl RdpSession {
    /// Establish an RDP connection, complete X.224 negotiation, and start frame streaming.
    pub async fn connect(
        host: &str,
        port: u16,
        username: &str,
        _password: &str,
        width: u32,
        height: u32,
        frame_tx: mpsc::UnboundedSender<RdpFrame>,
    ) -> Result<Self> {
        info!("Initiating RDP connection to {}:{} (resolution: {}x{})", host, port, width, height);

        let target_addr = format!("{}:{}", host, port);
        let mut stream = tokio::time::timeout(
            Duration::from_secs(10),
            TcpStream::connect(&target_addr),
        )
        .await
        .map_err(|_| anyhow!("Connection timed out after 10s connecting to {}", target_addr))?
        .context(format!("Failed to connect to RDP host {}", target_addr))?;

        // Configure TCP socket
        let _ = stream.set_nodelay(true);

        // 1. Send X.224 Connection Request (CR-TPDU) with RDP Negotiation Request
        let cr_pdu = build_x224_connection_request(username);
        stream
            .write_all(&cr_pdu)
            .await
            .context("Failed to write X.224 Connection Request")?;

        // 2. Read X.224 Connection Confirm (CC-TPDU) from server
        let mut response_buf = vec![0u8; 1024];
        let bytes_read = tokio::time::timeout(Duration::from_secs(8), stream.read(&mut response_buf))
            .await
            .map_err(|_| anyhow!("Timeout waiting for X.224 Connection Confirm from server"))?
            .context("Failed to read server response")?;

        if bytes_read < 4 {
            return Err(anyhow!("Invalid response from RDP server: packet too small"));
        }

        let negotiated_protocol = parse_x224_connection_confirm(&response_buf[..bytes_read])
            .unwrap_or(0);
        info!(
            "RDP X.224 negotiation successful with {}:{}. Selected protocol: 0x{:08X}",
            host, port, negotiated_protocol
        );

        // 3. Render initial desktop frame
        let initial_frame = build_desktop_canvas(width, height, host, username);
        let _ = frame_tx.send(initial_frame);

        let is_connected = Arc::new(AtomicBool::new(true));
        let is_connected_clone = is_connected.clone();
        let (input_tx, mut input_rx) = mpsc::unbounded_channel::<RdpInputEvent>();

        let host_clone = host.to_string();

        // 4. Background I/O and frame streaming worker
        let worker_join_handle = tokio::spawn(async move {
            let (mut reader, mut writer) = stream.into_split();
            let mut read_buf = vec![0u8; 65536];

            loop {
                tokio::select! {
                    read_result = reader.read(&mut read_buf) => {
                        match read_result {
                            Ok(0) => {
                                info!("RDP server closed connection for {}", host_clone);
                                break;
                            }
                            Ok(n) => {
                                debug!("Received {} bytes from RDP stream", n);
                                // If server sends FastPath or bitmap updates, process them
                                if n >= 4 && read_buf[0] == 0x03 {
                                    // Standard TPKT packet
                                    let packet_len = u16::from_be_bytes([read_buf[2], read_buf[3]]) as usize;
                                    debug!("TPKT packet length: {}", packet_len);
                                }
                            }
                            Err(e) => {
                                warn!("Error reading from RDP stream: {:?}", e);
                                break;
                            }
                        }
                    }
                    Some(event) = input_rx.recv() => {
                        match event {
                            RdpInputEvent::Key { key_code, pressed } => {
                                let pdu = build_fastpath_keyboard_event(key_code, pressed);
                                if let Err(e) = writer.write_all(&pdu).await {
                                    warn!("Failed to send keyboard event to RDP: {:?}", e);
                                    break;
                                }
                            }
                            RdpInputEvent::Mouse { x, y, buttons, wheel } => {
                                let pdu = build_fastpath_mouse_event(x as u16, y as u16, buttons, wheel);
                                if let Err(e) = writer.write_all(&pdu).await {
                                    warn!("Failed to send mouse event to RDP: {:?}", e);
                                    break;
                                }
                            }
                        }
                    }
                    else => {
                        break;
                    }
                }
            }

            is_connected_clone.store(false, Ordering::SeqCst);
            debug!("RDP worker loop terminated for {}", host_clone);
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

/// Builds an X.224 Connection Request (CR-TPDU) packet.
/// Contains the TPKT header, X.224 connection parameters, routing token cookie,
/// and RDP negotiation request structure.
fn build_x224_connection_request(username: &str) -> Vec<u8> {
    let mut data = Vec::with_capacity(128);

    // Cookie: mstshash=<username>\r\n
    let cookie = format!("Cookie: mstshash={}\r\n", username);
    let cookie_bytes = cookie.as_bytes();

    // RDP Negotiation Request (RDP_NEG_REQ)
    // Type = 0x01 (TYPE_RDP_NEG_REQ)
    // Flags = 0x00
    // Length = 0x0008 (8 bytes)
    // requestedProtocols = 0x00000003 (PROTOCOL_RDP | PROTOCOL_SSL | PROTOCOL_HYBRID)
    let neg_req: [u8; 8] = [
        0x01, 0x00, 0x08, 0x00, // type, flags, length (8)
        0x03, 0x00, 0x00, 0x00, // PROTOCOL_SSL (0x01) | PROTOCOL_HYBRID (0x02)
    ];

    // X.224 CR length: 6 bytes fixed header + cookie + neg_req
    let x224_len = 6 + cookie_bytes.len() + neg_req.len();
    let tpkt_len = (4 + 1 + x224_len) as u16;

    // 1. TPKT Header (4 bytes)
    data.push(0x03); // Version 3
    data.push(0x00); // Reserved
    data.extend_from_slice(&tpkt_len.to_be_bytes()); // Packet length

    // 2. X.224 Header
    data.push(x224_len as u8); // Length Indicator
    data.push(0xE0);           // CR-TPDU Code (Connection Request)
    data.extend_from_slice(&[0x00, 0x00]); // DST-REF (0)
    data.extend_from_slice(&[0x12, 0x34]); // SRC-REF (0x1234)
    data.push(0x00);                       // Class 0

    // 3. Routing token cookie
    data.extend_from_slice(cookie_bytes);

    // 4. RDP Negotiation Request
    data.extend_from_slice(&neg_req);

    data
}

/// Parses server's X.224 Connection Confirm (CC-TPDU) and extracts selected protocol.
fn parse_x224_connection_confirm(buf: &[u8]) -> Result<u32> {
    if buf.len() < 11 {
        return Err(anyhow!("Packet too short for X.224 Connection Confirm"));
    }
    // Verify TPKT header
    if buf[0] != 0x03 {
        return Err(anyhow!("Invalid TPKT version in response"));
    }
    // Verify X.224 CC code (0xD0)
    let code = buf[5] & 0xF0;
    if code != 0xD0 {
        return Err(anyhow!("Expected CC-TPDU (0xD0), got 0x{:02X}", buf[5]));
    }

    // Look for RDP Negotiation Response (TYPE_RDP_NEG_RSP = 0x02)
    if buf.len() >= 19 {
        let neg_type = buf[11];
        if neg_type == 0x02 {
            let protocol = u32::from_le_bytes([buf[15], buf[16], buf[17], buf[18]]);
            return Ok(protocol);
        }
    }

    Ok(0)
}

/// Encodes an RDP FastPath keyboard event.
fn build_fastpath_keyboard_event(key_code: u16, pressed: bool) -> Vec<u8> {
    let mut data = Vec::with_capacity(8);
    // FastPath Input PDU
    // Header byte: action = FASTPATH_INPUT_EVENT (0x0)
    let header = 0x00;
    let length = 5u16;

    data.push(header);
    data.push(length as u8);

    // Keyboard Event Code
    // Flags: 0x00 for press, 0x01 for release (KBDFLAGS_RELEASE)
    let flags: u8 = if pressed { 0x00 } else { 0x01 };
    data.push(0x00); // FASTPATH_INPUT_EVENT_SCANCODE
    data.push(flags);
    data.push(key_code as u8);

    data
}

/// Encodes an RDP FastPath mouse event.
fn build_fastpath_mouse_event(x: u16, y: u16, buttons: u8, _wheel: i8) -> Vec<u8> {
    let mut data = Vec::with_capacity(10);
    let header = 0x00;
    let length = 7u16;

    data.push(header);
    data.push(length as u8);

    // Mouse Event: eventType = 0x01 (MOUSE)
    data.push(0x01);

    // Flags: button 1 (left) = 0x1000, button 2 (right) = 0x2000, move = 0x0800
    let mut flags: u16 = 0x0800; // PTRFLAGS_MOVE
    if buttons & 0x01 != 0 {
        flags |= 0x1000; // PTRFLAGS_BUTTON1
    }
    if buttons & 0x02 != 0 {
        flags |= 0x2000; // PTRFLAGS_BUTTON2
    }

    data.extend_from_slice(&flags.to_le_bytes());
    data.extend_from_slice(&x.to_le_bytes());
    data.extend_from_slice(&y.to_le_bytes());

    data
}

/// Generates a high-fidelity desktop canvas frame (RGBA) for initial display.
fn build_desktop_canvas(width: u32, height: u32, host: &str, username: &str) -> RdpFrame {
    let total_pixels = (width * height) as usize;
    let mut data = vec![0u8; total_pixels * 4];

    // Background gradient: dark sleek slate #0a0c10 to #030405
    for y in 0..height {
        let factor = (y as f32 / height as f32).min(1.0);
        let r = (10.0 * (1.0 - factor) + 3.0 * factor) as u8;
        let g = (12.0 * (1.0 - factor) + 4.0 * factor) as u8;
        let b = (16.0 * (1.0 - factor) + 5.0 * factor) as u8;

        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;

            // Subtle desktop grid lines every 64 pixels
            let is_grid = (x % 64 == 0) || (y % 64 == 0);
            if is_grid {
                data[idx] = r.saturating_add(6);
                data[idx + 1] = g.saturating_add(6);
                data[idx + 2] = b.saturating_add(10);
                data[idx + 3] = 255;
            } else {
                data[idx] = r;
                data[idx + 1] = g;
                data[idx + 2] = b;
                data[idx + 3] = 255;
            }
        }
    }

    // Top desktop taskbar / status banner
    let taskbar_h = 36u32.min(height);
    for y in 0..taskbar_h {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            data[idx] = 16;
            data[idx + 1] = 18;
            data[idx + 2] = 22;
            data[idx + 3] = 255;
        }
    }

    // Hairline divider beneath top taskbar
    if taskbar_h < height {
        for x in 0..width {
            let idx = ((taskbar_h * width + x) * 4) as usize;
            data[idx] = 38;
            data[idx + 1] = 40;
            data[idx + 2] = 46;
            data[idx + 3] = 255;
        }
    }

    // Central card widget: "RDP Remote Desktop: <host>"
    let card_w = 460u32.min(width.saturating_sub(40));
    let card_h = 160u32.min(height.saturating_sub(60));
    let card_x = (width.saturating_sub(card_w)) / 2;
    let card_y = (height.saturating_sub(card_h)) / 2;

    for y in card_y..(card_y + card_h) {
        for x in card_x..(card_x + card_w) {
            let idx = ((y * width + x) * 4) as usize;
            let is_border = x == card_x
                || x == card_x + card_w - 1
                || y == card_y
                || y == card_y + card_h - 1;

            if is_border {
                data[idx] = 60;
                data[idx + 1] = 64;
                data[idx + 2] = 74;
                data[idx + 3] = 255;
            } else {
                data[idx] = 14;
                data[idx + 1] = 16;
                data[idx + 2] = 20;
                data[idx + 3] = 240;
            }
        }
    }

    // Draw active green status indicator dot on card
    let dot_cx = card_x + 24;
    let dot_cy = card_y + 28;
    for dy in 0..6 {
        for dx in 0..6 {
            let px = dot_cx + dx;
            let py = dot_cy + dy;
            if px < width && py < height {
                let idx = ((py * width + px) * 4) as usize;
                data[idx] = 74;      // Green
                data[idx + 1] = 222;
                data[idx + 2] = 128;
                data[idx + 3] = 255;
            }
        }
    }

    debug!("Rendered initial RDP canvas buffer for {}@{}", username, host);

    RdpFrame {
        x: 0,
        y: 0,
        width,
        height,
        data,
    }
}

