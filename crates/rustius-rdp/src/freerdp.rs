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
use tracing::info;

#[cfg(windows)]
use windows_sys::Win32::Foundation::HWND;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, EnumChildWindows, EnumWindows, GetWindowThreadProcessId, SetWindowPos, ShowWindow,
    SW_HIDE, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
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
    parent_hwnd: isize,
    child_pid: u32,
    child_hwnd: parking_lot::Mutex<isize>,
    is_connected: Arc<AtomicBool>,
    child_process: parking_lot::Mutex<Option<Child>>,
}

#[cfg(windows)]
unsafe extern "system" fn enum_child_proc(hwnd: HWND, lparam: isize) -> i32 {
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    let target = &mut *(lparam as *mut (u32, HWND));
    if pid == target.0 {
        target.1 = hwnd;
        return 0; // stop enumeration
    }
    1 // continue
}

#[cfg(windows)]
pub fn find_freerdp_hwnd(parent: HWND, pid: u32) -> HWND {
    let mut result: (u32, HWND) = (pid, std::ptr::null_mut());
    unsafe {
        if !parent.is_null() {
            EnumChildWindows(
                parent,
                Some(enum_child_proc),
                &mut result as *mut _ as isize,
            );
        }
        if result.1.is_null() {
            EnumWindows(
                Some(enum_child_proc),
                &mut result as *mut _ as isize,
            );
        }
    }
    result.1
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

        // Native Window Embedding directly into the application window
        if config.parent_window != 0 {
            cmd.arg(format!("/parent-window:{}", config.parent_window));
        }

        // Size
        if config.width > 0 && config.height > 0 {
            cmd.arg(format!("/size:{}x{}", config.width.max(100), config.height.max(100)));
        }

        // Performance & Hardware Acceleration Flags
        cmd.arg("/cert:ignore")
            .arg("+dynamic-resolution")
            .arg("+clipboard")
            .arg("/sound")
            .arg("/gfx")
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
        let child_pid = child.id().unwrap_or(0);
        info!("FreeRDP session started with PID: {}", child_pid);

        Ok(Self {
            host: config.host,
            port: config.port,
            parent_hwnd: config.parent_window,
            child_pid,
            child_hwnd: parking_lot::Mutex::new(0),
            is_connected: Arc::new(AtomicBool::new(true)),
            child_process: parking_lot::Mutex::new(Some(child)),
        })
    }

    /// Dynamically reposition and resize the embedded FreeRDP window to match the tab layout.
    pub fn update_bounds(&self, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
        #[cfg(windows)]
        {
            let mut hwnd = *self.child_hwnd.lock();
            if hwnd == 0 && self.child_pid != 0 {
                let discovered = find_freerdp_hwnd(self.parent_hwnd as HWND, self.child_pid);
                if !discovered.is_null() {
                    hwnd = discovered as isize;
                    *self.child_hwnd.lock() = hwnd;
                    info!("Discovered FreeRDP HWND: 0x{:X}", hwnd);
                }
            }
            if hwnd != 0 {
                unsafe {
                    SetWindowPos(
                        hwnd as HWND,
                        0 as HWND, // HWND_TOP
                        x,
                        y,
                        width.max(10),
                        height.max(10),
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                }
            }
        }
        Ok(())
    }

    /// Show or hide the native embedded window when switching tabs.
    pub fn set_visibility(&self, visible: bool) -> Result<()> {
        #[cfg(windows)]
        {
            let mut hwnd = *self.child_hwnd.lock();
            if hwnd == 0 && self.child_pid != 0 {
                let discovered = find_freerdp_hwnd(self.parent_hwnd as HWND, self.child_pid);
                if !discovered.is_null() {
                    hwnd = discovered as isize;
                    *self.child_hwnd.lock() = hwnd;
                }
            }
            if hwnd != 0 {
                unsafe {
                    if visible {
                        SetWindowPos(
                            hwnd as HWND,
                            0 as HWND,
                            0,
                            0,
                            0,
                            0,
                            SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_NOMOVE | SWP_NOSIZE,
                        );
                        ShowWindow(hwnd as HWND, SW_SHOW);
                    } else {
                        ShowWindow(hwnd as HWND, SW_HIDE);
                    }
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
        {
            let hwnd = *self.child_hwnd.lock();
            if hwnd != 0 {
                unsafe {
                    ShowWindow(hwnd as HWND, SW_HIDE);
                    DestroyWindow(hwnd as HWND);
                }
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
