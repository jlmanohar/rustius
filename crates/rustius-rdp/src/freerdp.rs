// FreeRDP Session Management with Native Container Window Embedding.
// Hosts FreeRDP (wfreerdp on Windows / xfreerdp on Linux) in a native child
// container window for hardware-accelerated Direct3D / H.264 / 60+ FPS performance.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::process::{Child, Command};
use tracing::{info, warn};

#[cfg(windows)]
use windows_sys::Win32::Foundation::HWND;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SetWindowPos, ShowWindow, SW_HIDE, SW_SHOW, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_VISIBLE,
};

/// Configuration for launching an embedded FreeRDP session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreeRdpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub domain: Option<String>,
    pub parent_window: isize,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Active FreeRDP session managing the child container window and worker process.
pub struct FreeRdpSession {
    pub host: String,
    pub port: u16,
    child_hwnd: isize,
    is_connected: Arc<AtomicBool>,
    child_process: parking_lot::Mutex<Option<Child>>,
}

fn encode_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Locate the prebuilt or installed FreeRDP executable.
pub fn find_freerdp_executable() -> Result<PathBuf> {
    // 1. Check relative to current running executable
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(parent) = exe_path.parent() {
            let candidates = [
                parent.join("resources").join("binaries").join("freerdp").join("wfreerdp.exe"),
                parent.join("binaries").join("freerdp").join("wfreerdp.exe"),
                parent.join("wfreerdp.exe"),
                parent.join("resources").join("wfreerdp.exe"),
                parent.join("resources").join("binaries").join("freerdp").join("xfreerdp"),
                parent.join("xfreerdp"),
            ];
            for p in candidates {
                if p.exists() {
                    return Ok(p);
                }
            }
        }
    }

    // 2. Check source / project tree candidates
    let project_candidates = [
        "resources/binaries/freerdp/wfreerdp.exe",
        "../resources/binaries/freerdp/wfreerdp.exe",
        "../../resources/binaries/freerdp/wfreerdp.exe",
        "resources/binaries/freerdp/xfreerdp",
        "../resources/binaries/freerdp/xfreerdp",
    ];

    for candidate in project_candidates {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Ok(p.canonicalize().unwrap_or(p));
        }
    }

    // 3. Fallback to system PATH lookup
    #[cfg(windows)]
    let binary_name = "wfreerdp.exe";
    #[cfg(not(windows))]
    let binary_name = "xfreerdp";

    if let Ok(paths) = std::env::var("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(binary_name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    Err(anyhow!(
        "FreeRDP executable not found. Ensure wfreerdp/xfreerdp is placed in resources/binaries/freerdp/"
    ))
}

impl FreeRdpSession {
    /// Launches FreeRDP and docks it into a native child window inside parent_window.
    pub async fn connect(config: FreeRdpConfig) -> Result<Self> {
        let freerdp_bin = find_freerdp_executable().context("Locating FreeRDP binary")?;
        info!("Launching FreeRDP from: {}", freerdp_bin.display());

        let mut child_hwnd: isize = 0;

        #[cfg(windows)]
        {
            if config.parent_window != 0 {
                let class_name = encode_wide("STATIC");
                let title = encode_wide("RustiusFreeRdpHost");

                let created = unsafe {
                    CreateWindowExW(
                        0,
                        class_name.as_ptr(),
                        title.as_ptr(),
                        WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS | WS_VISIBLE,
                        config.x,
                        config.y,
                        config.width.max(10),
                        config.height.max(10),
                        config.parent_window as HWND,
                        0 as _,
                        0 as _,
                        std::ptr::null(),
                    )
                };

                if !created.is_null() {
                    child_hwnd = created as isize;
                    info!("Created native Win32 child host HWND: 0x{:X}", child_hwnd);
                } else {
                    warn!("Failed to create Win32 child window, falling back to parent HWND");
                    child_hwnd = config.parent_window;
                }
            }
        }

        #[cfg(not(windows))]
        {
            child_hwnd = config.parent_window;
        }

        let mut cmd = Command::new(&freerdp_bin);

        // Core connection target
        let target = if config.port != 0 && config.port != 3389 {
            format!("{}:{}", config.host, config.port)
        } else {
            config.host.clone()
        };
        cmd.arg(format!("/v:{}", target));

        // Credentials
        if !config.username.is_empty() {
            cmd.arg(format!("/u:{}", config.username));
        }
        if let Some(ref pass) = config.password {
            if !pass.is_empty() {
                cmd.arg(format!("/p:{}", pass));
            }
        }
        if let Some(ref dom) = config.domain {
            if !dom.is_empty() {
                cmd.arg(format!("/d:{}", dom));
            }
        }

        // Native Window Embedding
        if child_hwnd != 0 {
            cmd.arg(format!("/parent-window:{}", child_hwnd));
        }

        // Performance & Hardware Acceleration Flags
        cmd.arg("/cert:ignore")
            .arg("+dynamic-resolution")
            .arg("/smart-sizing")
            .arg("+clipboard")
            .arg("/sound")
            .arg("/gfx:AVC444:on,AVC420:on,RFX:on")
            .arg("/network:auto")
            .arg("/bpp:32");

        cmd.stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW = 0x08000000 to prevent console flash
            cmd.creation_flags(0x08000000);
        }

        let child = cmd.spawn().context("Failed to spawn FreeRDP process")?;
        info!("FreeRDP session started with PID: {:?}", child.id());

        Ok(Self {
            host: config.host,
            port: config.port,
            child_hwnd,
            is_connected: Arc::new(AtomicBool::new(true)),
            child_process: parking_lot::Mutex::new(Some(child)),
        })
    }

    /// Dynamically reposition and resize the embedded FreeRDP window to match the tab layout.
    pub fn update_bounds(&self, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
        #[cfg(windows)]
        if self.child_hwnd != 0 {
            unsafe {
                SetWindowPos(
                    self.child_hwnd as HWND,
                    0 as HWND, // HWND_TOP
                    x,
                    y,
                    width.max(10),
                    height.max(10),
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
        Ok(())
    }

    /// Show or hide the native embedded window when switching tabs.
    pub fn set_visibility(&self, visible: bool) -> Result<()> {
        #[cfg(windows)]
        if self.child_hwnd != 0 {
            unsafe {
                if visible {
                    SetWindowPos(
                        self.child_hwnd as HWND,
                        0 as HWND,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_NOMOVE | SWP_NOSIZE,
                    );
                    ShowWindow(self.child_hwnd as HWND, SW_SHOW);
                } else {
                    ShowWindow(self.child_hwnd as HWND, SW_HIDE);
                }
            }
        }
        Ok(())
    }

    /// Disconnect FreeRDP and destroy the native child window.
    pub fn disconnect(&self) {
        self.is_connected.store(false, Ordering::SeqCst);

        if let Some(mut child) = self.child_process.lock().take() {
            let _ = child.start_kill();
        }

        #[cfg(windows)]
        if self.child_hwnd != 0 {
            unsafe {
                ShowWindow(self.child_hwnd as HWND, SW_HIDE);
                DestroyWindow(self.child_hwnd as HWND);
            }
        }
        info!("Disconnected FreeRDP session for {}:{}", self.host, self.port);
    }

    pub fn is_alive(&self) -> bool {
        self.is_connected.load(Ordering::SeqCst)
    }

    /// Non-blocking check for whether the child FreeRDP process has exited.
    pub fn try_wait_child(&self) -> Result<Option<std::process::ExitStatus>> {
        if let Some(ref mut child) = *self.child_process.lock() {
            return child.try_wait().map_err(Into::into);
        }
        Ok(None)
    }
}

impl Drop for FreeRdpSession {
    fn drop(&mut self) {
        self.disconnect();
    }
}
