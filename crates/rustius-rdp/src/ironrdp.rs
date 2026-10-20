use anyhow::{Context, Result};
use base64::Engine;
pub use ironrdp::input::MouseButton;
use ironrdp::input::{Database, MousePosition, Operation, Scancode, WheelRotations};
use std::sync::Arc;

use ironrdp_client::config::{ClipboardType, ConfigBuilder, Destination};
use ironrdp_client::output_channel::output_channel;
use ironrdp_client::rdp::{RdpClient, RdpInputEvent, RdpInputSender, RdpOutputEvent};
use ironrdp_cliprdr::backend::{ClipboardMessage, ClipboardMessageProxy};
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
    pub clipboard_sharing: Option<bool>,
}

/// A single dirty rectangle tile within a frame update.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IronRdpTile {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub data: String, // Base64 encoded RGBA pixel bytes
}

/// A batch of dirty tiles emitted atomically for a single screen update.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IronRdpFrameBatch {
    pub desktop_width: u16,
    pub desktop_height: u16,
    pub tiles: Vec<IronRdpTile>,
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

/// Efficient binary frame batch builder that packs desktop updates into a single byte buffer.
///
/// Binary Protocol:
/// - Offset 0..2: desktop_width (u16 LE)
/// - Offset 2..4: desktop_height (u16 LE)
/// - Offset 4..6: tile_count (u16 LE)
/// For each tile:
///   - 0..2: x (u16 LE)
///   - 2..4: y (u16 LE)
///   - 4..6: width (u16 LE)
///   - 6..8: height (u16 LE)
///   - 8..12: byte_len (u32 LE) = width * height * 4
///   - 12..12+byte_len: raw RGBA bytes
#[derive(Debug, Clone)]
pub struct BinaryFrameBatchBuilder {
    pub desktop_width: u16,
    pub desktop_height: u16,
    pub tile_count: u16,
    buffer: Vec<u8>,
}

impl BinaryFrameBatchBuilder {
    pub fn new(desktop_width: u16, desktop_height: u16) -> Self {
        let mut buffer = Vec::with_capacity(32768);
        buffer.extend_from_slice(&desktop_width.to_le_bytes());
        buffer.extend_from_slice(&desktop_height.to_le_bytes());
        buffer.extend_from_slice(&0u16.to_le_bytes());
        Self {
            desktop_width,
            desktop_height,
            tile_count: 0,
            buffer,
        }
    }

    pub fn add_tile(&mut self, x: u16, y: u16, width: u16, height: u16, u32_buffer: &[u32]) {
        self.tile_count += 1;
        let pixel_count = (width as usize) * (height as usize);
        let byte_len = (pixel_count * 4) as u32;

        self.buffer.extend_from_slice(&x.to_le_bytes());
        self.buffer.extend_from_slice(&y.to_le_bytes());
        self.buffer.extend_from_slice(&width.to_le_bytes());
        self.buffer.extend_from_slice(&height.to_le_bytes());
        self.buffer.extend_from_slice(&byte_len.to_le_bytes());

        self.buffer.reserve(pixel_count * 4);
        for &pixel in u32_buffer.iter().take(pixel_count) {
            let r = ((pixel >> 16) & 0xFF) as u8;
            let g = ((pixel >> 8) & 0xFF) as u8;
            let b = (pixel & 0xFF) as u8;
            self.buffer.extend_from_slice(&[r, g, b, 255]);
        }
    }

    pub fn finish(mut self) -> Vec<u8> {
        let count_bytes = self.tile_count.to_le_bytes();
        self.buffer[4] = count_bytes[0];
        self.buffer[5] = count_bytes[1];
        self.buffer
    }

    pub fn is_empty(&self) -> bool {
        self.tile_count == 0
    }
}

/// Events emitted by an active IronRDP session.
#[derive(Clone, Debug)]
pub enum IronRdpEvent {
    Connected,
    FrameBatch(IronRdpFrameBatch),
    BinaryFrameBatch(Vec<u8>),
    Terminated(RdpExitDiagnostic),
}

/// Forwarding proxy that routes clipboard events to the IronRDP client input queue.
#[derive(Clone, Debug)]
struct SessionClipboardProxy {
    sender: RdpInputSender,
    is_ready: Arc<std::sync::atomic::AtomicBool>,
}

impl ClipboardMessageProxy for SessionClipboardProxy {
    fn send_clipboard_message(&self, message: ClipboardMessage) {
        if matches!(message, ClipboardMessage::SendInitiateFileCopy(_))
            && !self.is_ready.load(std::sync::atomic::Ordering::SeqCst)
        {
            debug!(
                "Dropped SendInitiateFileCopy because clipboard channel is not in Ready state yet"
            );
            return;
        }
        if self.sender.send_clipboard(message).is_err() {
            warn!("Unable to enqueue clipboard message because the RDP session is closed");
        }
    }
}

/// Active IronRDP session controlling connection lifecycle and input injection.
pub struct IronRdpSession {
    input_sender: RdpInputSender,
    input_database: Mutex<Database>,
    desktop_size: RwLock<(u16, u16)>,
    #[cfg(windows)]
    clipboard_thread_id: std::sync::atomic::AtomicU32,
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
        let egfx_enabled = config.egfx.unwrap_or(true);
        let clipboard_enabled = config.clipboard_sharing.unwrap_or(true);
        let clipboard_type = if clipboard_enabled {
            ClipboardType::Enable
        } else {
            ClipboardType::Disable
        };

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
            .with_graphics_pipeline(egfx_enabled)
            .with_clipboard(clipboard_type);

        if egfx_enabled {
            info!("EGFX enabled: attaching GraphicsPipelineClient with OpenH264 AVC420 decoder");
            builder = builder.with_dvc(|_ps| {
                match ironrdp_egfx::decode::OpenH264Decoder::new() {
                    Ok(decoder) => {
                        struct RustiusEgfxHandler;
                        impl ironrdp_egfx::client::GraphicsPipelineHandler for RustiusEgfxHandler {
                            fn on_capabilities_confirmed(
                                &mut self,
                                caps: &ironrdp_egfx::pdu::CapabilitySet,
                            ) {
                                info!("Remote RDP server confirmed EGFX capabilities: {:?}", caps);
                            }
                            fn on_reset_graphics(&mut self, width: u32, height: u32) {
                                info!("EGFX reset graphics to {}x{}", width, height);
                            }
                        }

                        Some(ironrdp_egfx::client::GraphicsPipelineClient::new(
                            Box::new(RustiusEgfxHandler),
                            Some(Box::new(decoder)),
                        ))
                    }
                    Err(e) => {
                        warn!(
                            "Failed to initialize OpenH264 decoder for EGFX: {:?}; falling back to non-AVC",
                            e
                        );
                        None
                    }
                }
            });
        }

        if let Some(pass) = &config.password {
            builder = builder.with_password(pass.clone());
        }
        if let Some(domain) = &config.domain {
            if !domain.trim().is_empty() {
                builder = builder.with_domain(domain.clone());
            }
        }

        let client_config = builder
            .build()
            .context("Failed to build IronRDP configuration")?;

        let (output_tx, mut output_rx) = output_channel(256);
        let mut client = RdpClient::new(client_config, output_tx).with_desktop_updates();
        let input_sender = client.input_sender();

        let is_ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let local_files = Arc::new(parking_lot::RwLock::new(Vec::new()));
        let proxy_arc: Arc<dyn ClipboardMessageProxy + Send + Sync> =
            Arc::new(SessionClipboardProxy {
                sender: input_sender.clone(),
                is_ready: Arc::clone(&is_ready),
            });
        let staging_dir = std::env::temp_dir().join("rustius").join("clipboard");

        #[cfg(windows)]
        let (clipboard_factory, clipboard_thread_id) = if clipboard_enabled {
            let (tx, rx) = std::sync::mpsc::channel();
            let proxy = SessionClipboardProxy {
                sender: input_sender.clone(),
                is_ready: Arc::clone(&is_ready),
            };
            let local_files_pump = Arc::clone(&local_files);
            let proxy_arc_pump = Arc::clone(&proxy_arc);
            let staging_dir_pump = staging_dir.clone();
            let is_ready_pump = Arc::clone(&is_ready);

            let pump_handle = std::thread::Builder::new()
                .name("ironrdp-cliprdr-pump".to_string())
                .spawn(move || {
                    let thread_id = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
                    let clip = match ironrdp_cliprdr_native::WinClipboard::new(proxy) {
                        Ok(c) => c,
                        Err(e) => {
                            error!("Failed to initialize WinClipboard: {:?}", e);
                            let _ = tx.send(None);
                            return;
                        }
                    };

                    // Post WM_ACTIVATE (WA_INACTIVE) to ensure the hidden clipboard window
                    // transitions its internal `window_is_active` state to false, enabling
                    // local clipboard changes to trigger announcements to the remote desktop.
                    unsafe {
                        unsafe extern "system" fn enum_thread_wnd_proc(
                            hwnd: windows_sys::Win32::Foundation::HWND,
                            _lparam: windows_sys::Win32::Foundation::LPARAM,
                        ) -> windows_sys::Win32::Foundation::BOOL {
                            windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(
                                hwnd,
                                windows_sys::Win32::UI::WindowsAndMessaging::WM_ACTIVATE,
                                windows_sys::Win32::UI::WindowsAndMessaging::WA_INACTIVE as usize,
                                0,
                            );
                            1
                        }

                        windows_sys::Win32::UI::WindowsAndMessaging::EnumThreadWindows(
                            thread_id,
                            Some(enum_thread_wnd_proc),
                            0,
                        );
                    }

                    // Wrap the native WinClipboard factory with RustiusCliprdrBackendFactory
                    // to provide file-contents streaming and remote file ingestion.
                    let wrapped_factory = Box::new(crate::clipboard::RustiusCliprdrBackendFactory::new(
                        clip.backend_factory(),
                        proxy_arc_pump.clone(),
                        local_files_pump.clone(),
                        staging_dir_pump.clone(),
                        is_ready_pump.clone(),
                    )) as Box<dyn ironrdp_cliprdr::backend::CliprdrBackendFactory + Send>;

                    let _ = tx.send(Some((wrapped_factory, thread_id)));

                    // Run Windows message pump until WM_QUIT is posted on session disconnect
                    unsafe {
                        let mut msg = std::mem::zeroed();
                        while windows_sys::Win32::UI::WindowsAndMessaging::GetMessageW(&mut msg, core::ptr::null_mut(), 0, 0) > 0 {
                            if msg.message == windows_sys::Win32::UI::WindowsAndMessaging::WM_CLIPBOARDUPDATE {
                                // Inspect whether CF_HDROP (files) are present on the host clipboard
                                const CF_HDROP: u32 = 15;
                                if windows_sys::Win32::System::DataExchange::IsClipboardFormatAvailable(CF_HDROP) != 0 {
                                    let mut opened = false;
                                    for _ in 0..10 {
                                        if windows_sys::Win32::System::DataExchange::OpenClipboard(msg.hwnd) != 0 {
                                            opened = true;
                                            break;
                                        }
                                        std::thread::sleep(std::time::Duration::from_millis(5));
                                    }

                                    if opened {
                                        let hdrop = windows_sys::Win32::System::DataExchange::GetClipboardData(CF_HDROP);
                                        let paths = if !hdrop.is_null() {
                                            crate::clipboard::extract_hdrop_paths(hdrop as _)
                                        } else {
                                            Vec::new()
                                        };
                                        windows_sys::Win32::System::DataExchange::CloseClipboard();

                                        if !paths.is_empty() {
                                            // Ignore files originating from our own remote staging directory
                                            let is_from_staging = paths.iter().all(|p| p.starts_with(&staging_dir_pump));
                                            if !is_from_staging {
                                                let descriptors = crate::clipboard::create_file_descriptors_from_paths(&paths);
                                                if !descriptors.is_empty() {
                                                    *local_files_pump.write() = paths;
                                                    if is_ready_pump.load(std::sync::atomic::Ordering::SeqCst) {
                                                        info!(
                                                            count = descriptors.len(),
                                                            "Captured files from host clipboard; announcing to remote session"
                                                        );
                                                        proxy_arc_pump.send_clipboard_message(
                                                            ironrdp_cliprdr::backend::ClipboardMessage::SendInitiateFileCopy(descriptors),
                                                        );
                                                    } else {
                                                        debug!(
                                                            count = descriptors.len(),
                                                            "Captured files from host clipboard while channel not Ready; cached for on_ready"
                                                        );
                                                    }
                                                }
                                            } else {
                                                debug!("Ignoring clipboard update for files from remote staging directory");
                                            }
                                        }

                                        // CRUCIAL: Do NOT dispatch WM_CLIPBOARDUPDATE to WinClipboard!
                                        // If we dispatch to WinClipboard when files were copied, WinClipboard
                                        // will look for CF_UNICODETEXT, find nothing, and send an empty
                                        // format list (SendInitiateCopy(vec![])), immediately overwriting
                                        // the SendInitiateFileCopy we just sent and causing the remote
                                        // desktop paste operation to fail with "Unspecified error".
                                        continue;
                                    }
                                } else {
                                    // No files on clipboard; clear stored local files
                                    *local_files_pump.write() = Vec::new();
                                }
                            }

                            windows_sys::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
                            windows_sys::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);

                        }
                    }
                    debug!("IronRDP WinClipboard message pump thread exited");
                    drop(clip);
                });

            if let Err(e) = pump_handle {
                error!("Failed to spawn ironrdp-cliprdr-pump thread: {:?}", e);
                (None, 0u32)
            } else {
                match rx.recv_timeout(std::time::Duration::from_secs(5)) {
                    Ok(Some((factory, thread_id))) => (Some(factory), thread_id),
                    _ => {
                        warn!("WinClipboard initialization timed out or failed");
                        (None, 0u32)
                    }
                }
            }
        } else {
            (None, 0u32)
        };

        #[cfg(not(windows))]
        let clipboard_factory = if clipboard_enabled {
            let wrapped_factory = Box::new(crate::clipboard::RustiusCliprdrBackendFactory::new(
                ironrdp_cliprdr_native::StubClipboard::new().backend_factory(),
                proxy_arc,
                local_files,
                staging_dir,
                is_ready,
            ))
                as Box<dyn ironrdp_cliprdr::backend::CliprdrBackendFactory + Send>;
            Some(wrapped_factory)
        } else {
            None
        };

        if let Some(factory) = clipboard_factory {
            client = client.with_cliprdr_backend_factory(factory);
            info!("Attached CLIPRDR backend factory to IronRDP client");
        }

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

        // Spawn background task processing output events from the client with micro-batching
        tokio::spawn(async move {
            'outer: while let Some(event) = output_rx.recv().await {
                match event {
                    RdpOutputEvent::DesktopUpdate(update) => {
                        let (buffer, full_w, full_h, region) = update.into_parts();
                        let mut fw = full_w.get();
                        let mut fh = full_h.get();

                        let mut builder = BinaryFrameBatchBuilder::new(fw, fh);

                        let left = region.left;
                        let top = region.top;
                        let rw = (region.right.saturating_sub(region.left) + 1).max(1);
                        let rh = (region.bottom.saturating_sub(region.top) + 1).max(1);
                        builder.add_tile(left, top, rw, rh, &buffer);

                        // Coalesce additional tiles arriving in the same burst (within 4ms)
                        // up to a maximum batch size of 64 tiles.
                        let batch_timeout = std::time::Duration::from_millis(4);
                        while builder.tile_count < 64 {
                            match tokio::time::timeout(batch_timeout, output_rx.recv()).await {
                                Ok(Some(RdpOutputEvent::DesktopUpdate(next_update))) => {
                                    let (next_buffer, next_fw, next_fh, next_region) =
                                        next_update.into_parts();
                                    if next_fw.get() != fw || next_fh.get() != fh {
                                        // Resolution changed: flush current batch
                                        if !builder.is_empty() {
                                            on_event_loop(IronRdpEvent::BinaryFrameBatch(
                                                builder.finish(),
                                            ));
                                        }

                                        fw = next_fw.get();
                                        fh = next_fh.get();
                                        let mut new_builder = BinaryFrameBatchBuilder::new(fw, fh);
                                        let next_rw =
                                            (next_region.right.saturating_sub(next_region.left)
                                                + 1)
                                            .max(1);
                                        let next_rh =
                                            (next_region.bottom.saturating_sub(next_region.top)
                                                + 1)
                                            .max(1);
                                        new_builder.add_tile(
                                            next_region.left,
                                            next_region.top,
                                            next_rw,
                                            next_rh,
                                            &next_buffer,
                                        );
                                        on_event_loop(IronRdpEvent::BinaryFrameBatch(
                                            new_builder.finish(),
                                        ));
                                        builder = BinaryFrameBatchBuilder::new(fw, fh);
                                        break;
                                    }
                                    let next_rw =
                                        (next_region.right.saturating_sub(next_region.left) + 1)
                                            .max(1);
                                    let next_rh =
                                        (next_region.bottom.saturating_sub(next_region.top) + 1)
                                            .max(1);
                                    builder.add_tile(
                                        next_region.left,
                                        next_region.top,
                                        next_rw,
                                        next_rh,
                                        &next_buffer,
                                    );
                                }
                                Ok(Some(other_event)) => {
                                    if !builder.is_empty() {
                                        on_event_loop(IronRdpEvent::BinaryFrameBatch(
                                            builder.finish(),
                                        ));
                                        builder = BinaryFrameBatchBuilder::new(fw, fh);
                                    }
                                    if handle_non_desktop_event(other_event, &*on_event_loop) {
                                        break 'outer;
                                    }
                                    break;
                                }
                                Ok(None) => {
                                    if !builder.is_empty() {
                                        on_event_loop(IronRdpEvent::BinaryFrameBatch(
                                            builder.finish(),
                                        ));
                                    }
                                    break 'outer;
                                }
                                Err(_) => {
                                    // 4ms batching window completed; flush gathered tiles
                                    break;
                                }
                            }
                        }

                        if !builder.is_empty() {
                            on_event_loop(IronRdpEvent::BinaryFrameBatch(builder.finish()));
                        }
                    }
                    non_desktop_event => {
                        if handle_non_desktop_event(non_desktop_event, &*on_event_loop) {
                            break 'outer;
                        }
                    }
                }
            }
        });

        Ok(Self {
            input_sender,
            input_database: Mutex::new(Database::default()),
            desktop_size: RwLock::new((initial_width, initial_height)),
            #[cfg(windows)]
            clipboard_thread_id: std::sync::atomic::AtomicU32::new(clipboard_thread_id),
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
        #[cfg(windows)]
        {
            let tid = self
                .clipboard_thread_id
                .swap(0, std::sync::atomic::Ordering::SeqCst);
            if tid != 0 {
                unsafe {
                    windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                        tid,
                        windows_sys::Win32::UI::WindowsAndMessaging::WM_QUIT,
                        0,
                        0,
                    );
                }
            }
        }
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

impl Drop for IronRdpSession {
    fn drop(&mut self) {
        self.disconnect();
    }
}

/// Handle non-desktop events (Connected, Image snapshot, ConnectionFailure, Terminated)
fn handle_non_desktop_event(event: RdpOutputEvent, on_event: &impl Fn(IronRdpEvent)) -> bool {
    match event {
        RdpOutputEvent::Connected => {
            info!("IronRDP session connected");
            on_event(IronRdpEvent::Connected);
            false
        }
        RdpOutputEvent::Image {
            buffer,
            width,
            height,
        } => {
            let w = width.get();
            let h = height.get();
            let mut builder = BinaryFrameBatchBuilder::new(w, h);
            builder.add_tile(0, 0, w, h, &buffer);
            on_event(IronRdpEvent::BinaryFrameBatch(builder.finish()));
            false
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
            on_event(IronRdpEvent::Terminated(diag));
            true
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
                    on_event(IronRdpEvent::Terminated(diag));
                }
                Err(error) => {
                    error!("IronRDP active session error: {:?}", error);
                    let error_str = error.to_string();
                    let raw_str = format!("{:?}", error);
                    let (title, message) = if raw_str.contains("rfx progressive")
                        || raw_str.contains("GraphicsPipeline")
                    {
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
                    on_event(IronRdpEvent::Terminated(diag));
                }
            }
            true
        }
        other => {
            debug!("Other IronRDP event: {:?}", other);
            false
        }
    }
}

/// Convert packed 0x00RRGGBB pixels to RGBA and encode to Base64 tile
#[allow(dead_code)]
fn encode_u32_buffer_to_tile(
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    buffer: &[u32],
) -> IronRdpTile {
    let pixel_count = (width as usize) * (height as usize);
    let mut rgba = Vec::with_capacity(pixel_count * 4);

    for &pixel in buffer.iter().take(pixel_count) {
        let r = ((pixel >> 16) & 0xFF) as u8;
        let g = ((pixel >> 8) & 0xFF) as u8;
        let b = (pixel & 0xFF) as u8;
        rgba.extend_from_slice(&[r, g, b, 255]);
    }

    let data = base64::engine::general_purpose::STANDARD.encode(&rgba);

    IronRdpTile {
        x,
        y,
        width,
        height,
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
        let decoded: IronRdpFrame =
            serde_json::from_str(&json).expect("Failed to deserialize frame");

        assert_eq!(decoded.x, 10);
        assert_eq!(decoded.y, 20);
        assert_eq!(decoded.width, 100);
        assert_eq!(decoded.height, 50);
        assert_eq!(decoded.desktop_width, 1920);
        assert_eq!(decoded.desktop_height, 1080);
        assert_eq!(decoded.data, "AQIDBA==");
    }

    #[test]
    fn test_ironrdp_frame_batch_serialization() {
        let batch = IronRdpFrameBatch {
            desktop_width: 1920,
            desktop_height: 1080,
            tiles: vec![
                IronRdpTile {
                    x: 0,
                    y: 0,
                    width: 64,
                    height: 64,
                    data: "AQIDBA==".to_string(),
                },
                IronRdpTile {
                    x: 64,
                    y: 0,
                    width: 64,
                    height: 64,
                    data: "BQYHCA==".to_string(),
                },
            ],
        };

        let json = serde_json::to_string(&batch).expect("Failed to serialize frame batch");
        let decoded: IronRdpFrameBatch =
            serde_json::from_str(&json).expect("Failed to deserialize frame batch");

        assert_eq!(decoded.desktop_width, 1920);
        assert_eq!(decoded.desktop_height, 1080);
        assert_eq!(decoded.tiles.len(), 2);
        assert_eq!(decoded.tiles[0].x, 0);
        assert_eq!(decoded.tiles[1].x, 64);
        assert_eq!(decoded.tiles[1].data, "BQYHCA==");
    }

    #[test]
    fn test_binary_frame_batch_packing() {
        let mut builder = BinaryFrameBatchBuilder::new(1920, 1080);
        let u32_pixels = vec![0x00FF0000, 0x0000FF00];
        builder.add_tile(10, 20, 2, 1, &u32_pixels);
        let bytes = builder.finish();

        assert!(bytes.len() >= 6 + 12 + 8);
        let dw = u16::from_le_bytes([bytes[0], bytes[1]]);
        let dh = u16::from_le_bytes([bytes[2], bytes[3]]);
        let num_tiles = u16::from_le_bytes([bytes[4], bytes[5]]);
        assert_eq!(dw, 1920);
        assert_eq!(dh, 1080);
        assert_eq!(num_tiles, 1);

        let x = u16::from_le_bytes([bytes[6], bytes[7]]);
        let y = u16::from_le_bytes([bytes[8], bytes[9]]);
        let w = u16::from_le_bytes([bytes[10], bytes[11]]);
        let h = u16::from_le_bytes([bytes[12], bytes[13]]);
        let len = u32::from_le_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]);
        assert_eq!(x, 10);
        assert_eq!(y, 20);
        assert_eq!(w, 2);
        assert_eq!(h, 1);
        assert_eq!(len, 8);
        assert_eq!(&bytes[18..22], &[255, 0, 0, 255]);
        assert_eq!(&bytes[22..26], &[0, 255, 0, 255]);
    }

    #[test]
    fn test_egfx_openh264_initialization() {
        let decoder = ironrdp_egfx::decode::OpenH264Decoder::new();
        assert!(decoder.is_ok(), "OpenH264 decoder must initialize successfully");

        struct TestHandler;
        impl ironrdp_egfx::client::GraphicsPipelineHandler for TestHandler {}

        let handler = TestHandler;
        use ironrdp_egfx::client::GraphicsPipelineHandler;
        let caps = handler.capabilities();
        assert!(!caps.is_empty());
        let has_v8_1 = caps.iter().any(|c| {
            matches!(
                c,
                ironrdp_egfx::pdu::CapabilitySet::V8_1 { flags }
                    if flags.contains(ironrdp_egfx::pdu::CapabilitiesV81Flags::AVC420_ENABLED)
            )
        });
        assert!(has_v8_1, "EGFX handler must advertise V8.1 with AVC420_ENABLED");
    }
}
