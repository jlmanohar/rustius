use std::sync::Arc;
use anyhow::{Context, Result};
use base64::Engine;
pub use ironrdp::input::MouseButton;
use ironrdp::input::{Database, MousePosition, Operation, Scancode, WheelRotations};

use ironrdp_client::config::{ConfigBuilder, Destination};
use ironrdp_client::output_channel::output_channel;
use ironrdp_client::rdp::{RdpClient, RdpInputEvent, RdpInputSender, RdpOutputEvent};
use ironrdp_pdu::rdp::capability_sets::MajorPlatformType;
use ironrdp_tls::CertificateValidation;

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use crate::freerdp::RdpExitDiagnostic;

/// Configuration for an IronRDP connection.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IronRdpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub domain: Option<String>,
    pub width: u16,
    pub height: u16,
    pub scale_factor: Option<u32>,
    pub egfx: Option<bool>,
}

/// Dirty rectangle or full image frame sent to the frontend.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IronRdpFrame {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub desktop_width: u16,
    pub desktop_height: u16,
    pub data: String, // Base64 encoded RGBA pixel bytes
}

/// Events emitted by an active IronRDP session.
#[derive(Clone, Debug)]
pub enum IronRdpEvent {
    Connected,
    Frame(IronRdpFrame),
    Terminated(RdpExitDiagnostic),
}

/// Active IronRDP session controlling connection lifecycle and input injection.
pub struct IronRdpSession {
    input_sender: RdpInputSender,
    input_database: Mutex<Database>,
    desktop_size: RwLock<(u16, u16)>,
}

impl IronRdpSession {
    /// Connect to the target host and start streaming events to `on_event`.
    pub async fn connect<F>(config: IronRdpConfig, on_event: F) -> Result<Self>
    where
        F: Fn(IronRdpEvent) + Send + Sync + 'static,
    {
        info!(
            "Connecting with IronRDP to {}:{} (user: {}, domain: {:?})",
            config.host, config.port, config.username, config.domain
        );

        let dest = Destination::from_parts(config.host.clone(), config.port);
        let egfx_enabled = config.egfx.unwrap_or(false);

        let mut builder = ConfigBuilder::new()
            .with_destination(dest)
            .with_username(config.username.clone())
            .with_client_build(10000)
            .with_client_name("rustius-client")
            .with_client_dir("rustius")
            .with_platform(MajorPlatformType::WINDOWS)
            .with_color_depth(32)
            .with_desktop_width(config.width)
            .with_desktop_height(config.height)
            .with_desktop_scale_factor(config.scale_factor.unwrap_or(100))
            .with_tls(true)
            .with_credssp(true)
            .with_certificate_validation(CertificateValidation::DangerouslyAcceptInvalidCertificate)
            .with_pointer_software_rendering(true)
            .with_graphics_pipeline(egfx_enabled);

        if let Some(pass) = &config.password {
            builder = builder.with_password(pass.clone());
        }
        if let Some(domain) = &config.domain {
            if !domain.trim().is_empty() {
                builder = builder.with_domain(domain.clone());
            }
        }

        let client_config = builder.build().context("Failed to build IronRDP configuration")?;

        let (output_tx, mut output_rx) = output_channel(256);
        let client = RdpClient::new(client_config, output_tx).with_desktop_updates();
        let input_sender = client.input_sender();

        let initial_width = config.width;
        let initial_height = config.height;

        let on_event = Arc::new(on_event);
        let on_event_loop = Arc::clone(&on_event);

        // Spawn dedicated thread running client.run() with a current-thread runtime
        std::thread::Builder::new()
            .name("ironrdp-session".to_string())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        error!("Failed to create local runtime for IronRDP: {:?}", e);
                        return;
                    }
                };

                debug!("Starting IronRDP client run loop on dedicated thread");
                rt.block_on(client.run());
                debug!("IronRDP client run loop exited");
            })
            .context("Failed to spawn IronRDP thread")?;


        // Spawn background task processing output events from the client
        tokio::spawn(async move {
            while let Some(event) = output_rx.recv().await {
                match event {
                    RdpOutputEvent::Connected => {
                        info!("IronRDP session connected");
                        on_event_loop(IronRdpEvent::Connected);
                    }
                    RdpOutputEvent::Image { buffer, width, height } => {
                        let w = width.get();
                        let h = height.get();

                        let frame = encode_u32_buffer_to_frame(0, 0, w, h, w, h, &buffer);
                        on_event_loop(IronRdpEvent::Frame(frame));
                    }
                    RdpOutputEvent::DesktopUpdate(update) => {
                        let (buffer, full_w, full_h, region) = update.into_parts();
                        let fw = full_w.get();
                        let fh = full_h.get();


                        let left = region.left;
                        let top = region.top;
                        let rw = (region.right.saturating_sub(region.left) + 1).max(1);
                        let rh = (region.bottom.saturating_sub(region.top) + 1).max(1);

                        let frame = encode_u32_buffer_to_frame(left, top, rw, rh, fw, fh, &buffer);
                        on_event_loop(IronRdpEvent::Frame(frame));
                    }
                    RdpOutputEvent::ConnectionFailure(error) => {
                        error!("IronRDP connection failed: {:?}", error);
                        let diag = RdpExitDiagnostic {
                            code: -1,
                            title: "Connection Failed".to_string(),
                            message: format!("IronRDP connection failure: {}", error),
                            severity: "error".to_string(),
                            raw_error: Some(format!("{:?}", error)),
                        };
                        on_event_loop(IronRdpEvent::Terminated(diag));
                        break;
                    }
                    RdpOutputEvent::Terminated(result) => {
                        match result {
                            Ok(reason) => {
                                info!("IronRDP session terminated gracefully: {}", reason);
                                let diag = RdpExitDiagnostic {
                                    code: 0,
                                    title: "Session Disconnected".to_string(),
                                    message: format!("Session disconnected: {}", reason),
                                    severity: "info".to_string(),
                                    raw_error: None,
                                };
                                on_event_loop(IronRdpEvent::Terminated(diag));
                            }
                            Err(error) => {
                                error!("IronRDP active session error: {:?}", error);
                                let error_str = error.to_string();
                                let raw_str = format!("{:?}", error);
                                let (title, message) = if raw_str.contains("rfx progressive") || raw_str.contains("GraphicsPipeline") {
                                    (
                                        "Graphics Pipeline Codec Error".to_string(),
                                        "RemoteFX progressive decoding failed. Keep EGFX disabled to use standard reliable bitmap updates.".to_string(),
                                    )
                                } else {
                                    (
                                        "Session Error".to_string(),
                                        format!("Session error: {}", error_str),
                                    )
                                };
                                let diag = RdpExitDiagnostic {
                                    code: -2,
                                    title,
                                    message,
                                    severity: "error".to_string(),
                                    raw_error: Some(raw_str),
                                };
                                on_event_loop(IronRdpEvent::Terminated(diag));
                            }
                        }
                        break;
                    }
                    other => {
                        debug!("Other IronRDP event: {:?}", other);
                    }
                }
            }
        });

        Ok(Self {
            input_sender,
            input_database: Mutex::new(Database::default()),
            desktop_size: RwLock::new((initial_width, initial_height)),
        })
    }

    /// Forward mouse move coordinate
    pub fn mouse_move(&self, x: u16, y: u16) {
        let op = Operation::MouseMove(MousePosition { x, y });
        self.apply_and_send_operations([op]);
    }

    /// Forward mouse button down / up
    pub fn mouse_button(&self, button: MouseButton, pressed: bool) {
        let op = if pressed {
            Operation::MouseButtonPressed(button)
        } else {
            Operation::MouseButtonReleased(button)
        };
        self.apply_and_send_operations([op]);
    }

    /// Forward mouse wheel delta
    pub fn mouse_wheel(&self, is_vertical: bool, rotation_units: i16) {
        let op = Operation::WheelRotations(WheelRotations {
            is_vertical,
            rotation_units,
        });
        self.apply_and_send_operations([op]);
    }

    /// Forward keyboard key based on web KeyboardEvent.code
    pub fn key_event(&self, code: &str, pressed: bool) {
        if let Some(scancode) = map_web_code_to_scancode(code) {
            let op = if pressed {
                Operation::KeyPressed(scancode)
            } else {
                Operation::KeyReleased(scancode)
            };
            self.apply_and_send_operations([op]);
        } else {
            warn!("Unrecognized web key code: {}", code);
        }
    }

    /// Dynamically resize the desktop
    pub fn resize(&self, width: u16, height: u16, scale_factor: u32) {
        *self.desktop_size.write() = (width, height);
        let _ = self.input_sender.try_send(RdpInputEvent::Resize {
            width,
            height,
            scale_factor,
            physical_size: None,
        });
    }

    /// Request session shutdown
    pub fn disconnect(&self) {
        self.input_sender.request_close();
    }

    fn apply_and_send_operations(&self, operations: impl IntoIterator<Item = Operation>) {
        let Ok(permit) = self.input_sender.try_reserve() else {
            return;
        };
        let mut db = self.input_database.lock();
        let events = db.apply(operations);
        if !events.is_empty() {
            permit.send(RdpInputEvent::FastPath(events));
        }
    }
}

/// Convert packed 0x00RRGGBB pixels to RGBA and encode to Base64 frame
fn encode_u32_buffer_to_frame(
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    desktop_width: u16,
    desktop_height: u16,
    buffer: &[u32],
) -> IronRdpFrame {
    let pixel_count = (width as usize) * (height as usize);
    let mut rgba = Vec::with_capacity(pixel_count * 4);

    for &pixel in buffer.iter().take(pixel_count) {
        let r = ((pixel >> 16) & 0xFF) as u8;
        let g = ((pixel >> 8) & 0xFF) as u8;
        let b = (pixel & 0xFF) as u8;
        rgba.extend_from_slice(&[r, g, b, 255]);
    }

    let data = base64::engine::general_purpose::STANDARD.encode(&rgba);

    IronRdpFrame {
        x,
        y,
        width,
        height,
        desktop_width,
        desktop_height,
        data,
    }
}

/// Convert DOM / Web `KeyboardEvent.code` to PC/AT set-1 `Scancode`
fn map_web_code_to_scancode(code: &str) -> Option<Scancode> {
    let (extended, make) = match code {
        "Escape" => (false, 0x01),
        "Digit1" => (false, 0x02),
        "Digit2" => (false, 0x03),
        "Digit3" => (false, 0x04),
        "Digit4" => (false, 0x05),
        "Digit5" => (false, 0x06),
        "Digit6" => (false, 0x07),
        "Digit7" => (false, 0x08),
        "Digit8" => (false, 0x09),
        "Digit9" => (false, 0x0A),
        "Digit0" => (false, 0x0B),
        "Minus" => (false, 0x0C),
        "Equal" => (false, 0x0D),
        "Backspace" => (false, 0x0E),
        "Tab" => (false, 0x0F),
        "KeyQ" => (false, 0x10),
        "KeyW" => (false, 0x11),
        "KeyE" => (false, 0x12),
        "KeyR" => (false, 0x13),
        "KeyT" => (false, 0x14),
        "KeyY" => (false, 0x15),
        "KeyU" => (false, 0x16),
        "KeyI" => (false, 0x17),
        "KeyO" => (false, 0x18),
        "KeyP" => (false, 0x19),
        "BracketLeft" => (false, 0x1A),
        "BracketRight" => (false, 0x1B),
        "Enter" => (false, 0x1C),
        "ControlLeft" => (false, 0x1D),
        "KeyA" => (false, 0x1E),
        "KeyS" => (false, 0x1F),
        "KeyD" => (false, 0x20),
        "KeyF" => (false, 0x21),
        "KeyG" => (false, 0x22),
        "KeyH" => (false, 0x23),
        "KeyJ" => (false, 0x24),
        "KeyK" => (false, 0x25),
        "KeyL" => (false, 0x26),
        "Semicolon" => (false, 0x27),
        "Quote" => (false, 0x28),
        "Backquote" => (false, 0x29),
        "ShiftLeft" => (false, 0x2A),
        "Backslash" => (false, 0x2B),
        "KeyZ" => (false, 0x2C),
        "KeyX" => (false, 0x2D),
        "KeyC" => (false, 0x2E),
        "KeyV" => (false, 0x2F),
        "KeyB" => (false, 0x30),
        "KeyN" => (false, 0x31),
        "KeyM" => (false, 0x32),
        "Comma" => (false, 0x33),
        "Period" => (false, 0x34),
        "Slash" => (false, 0x35),
        "ShiftRight" => (false, 0x36),
        "NumpadMultiply" => (false, 0x37),
        "AltLeft" => (false, 0x38),
        "Space" => (false, 0x39),
        "CapsLock" => (false, 0x3A),
        "F1" => (false, 0x3B),
        "F2" => (false, 0x3C),
        "F3" => (false, 0x3D),
        "F4" => (false, 0x3E),
        "F5" => (false, 0x3F),
        "F6" => (false, 0x40),
        "F7" => (false, 0x41),
        "F8" => (false, 0x42),
        "F9" => (false, 0x43),
        "F10" => (false, 0x44),
        "NumLock" => (false, 0x45),
        "ScrollLock" => (false, 0x46),
        "Numpad7" => (false, 0x47),
        "Numpad8" => (false, 0x48),
        "Numpad9" => (false, 0x49),
        "NumpadSubtract" => (false, 0x4A),
        "Numpad4" => (false, 0x4B),
        "Numpad5" => (false, 0x4C),
        "Numpad6" => (false, 0x4D),
        "NumpadAdd" => (false, 0x4E),
        "Numpad1" => (false, 0x4F),
        "Numpad2" => (false, 0x50),
        "Numpad3" => (false, 0x51),
        "Numpad0" => (false, 0x52),
        "NumpadDecimal" => (false, 0x53),
        "F11" => (false, 0x57),
        "F12" => (false, 0x58),
        // Extended keys (E0 prefix)
        "NumpadEnter" => (true, 0x1C),
        "ControlRight" => (true, 0x1D),
        "NumpadDivide" => (true, 0x35),
        "AltRight" => (true, 0x38),
        "Home" => (true, 0x47),
        "ArrowUp" => (true, 0x48),
        "PageUp" => (true, 0x49),
        "ArrowLeft" => (true, 0x4B),
        "ArrowRight" => (true, 0x4D),
        "End" => (true, 0x4F),
        "ArrowDown" => (true, 0x50),
        "PageDown" => (true, 0x51),
        "Insert" => (true, 0x52),
        "Delete" => (true, 0x53),
        "MetaLeft" | "OSLeft" => (true, 0x5B),
        "MetaRight" | "OSRight" => (true, 0x5C),
        "ContextMenu" => (true, 0x5D),
        _ => return None,
    };

    Some(Scancode::from_u8(extended, make))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_code_to_scancode_mappings() {
        // Standard alphanumeric
        assert!(map_web_code_to_scancode("KeyA").is_some());
        assert!(map_web_code_to_scancode("KeyZ").is_some());
        assert!(map_web_code_to_scancode("Digit0").is_some());
        assert!(map_web_code_to_scancode("Space").is_some());
        assert!(map_web_code_to_scancode("Enter").is_some());
        assert!(map_web_code_to_scancode("Escape").is_some());

        // Modifiers & Extended keys
        assert!(map_web_code_to_scancode("ControlLeft").is_some());
        assert!(map_web_code_to_scancode("ControlRight").is_some());
        assert!(map_web_code_to_scancode("AltLeft").is_some());
        assert!(map_web_code_to_scancode("AltRight").is_some());
        assert!(map_web_code_to_scancode("Delete").is_some());
        assert!(map_web_code_to_scancode("ArrowUp").is_some());
        assert!(map_web_code_to_scancode("ArrowDown").is_some());
        assert!(map_web_code_to_scancode("MetaLeft").is_some());

        // Unknown key returns None
        assert!(map_web_code_to_scancode("NonExistentKey123").is_none());
    }

    #[test]
    fn test_ironrdp_frame_serialization() {
        let frame = IronRdpFrame {
            x: 10,
            y: 20,
            width: 100,
            height: 50,
            desktop_width: 1920,
            desktop_height: 1080,
            data: "AQIDBA==".to_string(),
        };

        let json = serde_json::to_string(&frame).expect("Failed to serialize frame");
        let decoded: IronRdpFrame = serde_json::from_str(&json).expect("Failed to deserialize frame");

        assert_eq!(decoded.x, 10);
        assert_eq!(decoded.y, 20);
        assert_eq!(decoded.width, 100);
        assert_eq!(decoded.height, 50);
        assert_eq!(decoded.desktop_width, 1920);
        assert_eq!(decoded.desktop_height, 1080);
        assert_eq!(decoded.data, "AQIDBA==");
    }
}
