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
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
#[cfg(windows)]
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, DestroyWindow, EnumChildWindows, EnumWindows, GetClientRect, GetParent,
    GetWindowLongW, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, SetParent,
    SetWindowLongPtrW, SetWindowLongW, SetWindowPos, ShowWindow, GWL_EXSTYLE, GWL_STYLE,
    GWLP_HWNDPARENT, SW_HIDE, SW_SHOW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_SHOWWINDOW,
    WS_CAPTION, WS_CHILD, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
    WS_POPUP, WS_SYSMENU, WS_THICKFRAME, WS_VISIBLE,
};


/// File-based diagnostics helper for FreeRDP embedding
pub fn log_freerdp(msg: &str) {
    tracing::info!("{}", msg);
    let log_path = std::env::temp_dir().join("rustius_freerdp.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log_path) {
        use std::io::Write;
        let _ = writeln!(f, "[FreeRDP] {}", msg);
    }
}

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
    child_hwnd: Arc<parking_lot::Mutex<isize>>,
    last_bounds: Arc<parking_lot::Mutex<(i32, i32, i32, i32)>>,
    is_connected: Arc<AtomicBool>,
    child_process: parking_lot::Mutex<Option<Child>>,
}

#[cfg(windows)]
unsafe extern "system" fn enum_child_proc(hwnd: HWND, lparam: isize) -> i32 {
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    let target = &mut *(lparam as *mut (u32, HWND));
    if pid == target.0 {
        let mut rect: RECT = std::mem::zeroed();
        GetWindowRect(hwnd, &mut rect);
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let visible = IsWindowVisible(hwnd) != 0;
        // Only accept windows that are visible or have non-trivial dimensions (> 100x100).
        // This explicitly skips 0x0 hidden clipboard/OLE message windows.
        if visible || (width > 100 && height > 100) {
            target.1 = hwnd;
            return 0; // stop enumeration: real display window found!
        }
    }
    1 // continue
}

#[cfg(windows)]
pub(crate) fn find_freerdp_hwnd(parent: HWND, pid: u32) -> HWND {
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

#[cfg(windows)]
unsafe fn apply_window_docking(
    parent_hwnd: HWND,
    hwnd: HWND,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) {
    if hwnd.is_null() || parent_hwnd.is_null() {
        return;
    }

    // 1. Get client rect of Tauri main window to clip strictly within app boundary
    let mut client_rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetClientRect(parent_hwnd, &mut client_rect);
    let parent_w = client_rect.right - client_rect.left;
    let parent_h = client_rect.bottom - client_rect.top;

    // If canvas coordinates are completely off-screen or parent has no size, hide
    if parent_w <= 0 || parent_h <= 0 || x >= parent_w || y >= parent_h {
        ShowWindow(hwnd, SW_HIDE);
        return;
    }

    // 2. Clamp width and height strictly within app boundary so it never spills out
    let clamped_w = width.min(parent_w - x).max(0);
    let clamped_h = height.min(parent_h - y).max(0);

    if clamped_w <= 10 || clamped_h <= 10 {
        ShowWindow(hwnd, SW_HIDE);
        return;
    }

    // 3. Reset parent to root desktop if it was previously reparented as a child
    let cur_parent = GetParent(hwnd);
    if cur_parent != 0 as HWND {
        SetParent(hwnd, 0 as HWND);
    }

    // 4. Remove borders, caption, and resize grips so it's a clean borderless popup
    let cur_style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    let new_style = (cur_style & !(WS_CAPTION | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SYSMENU | WS_CHILD))
        | WS_POPUP
        | WS_VISIBLE;
    SetWindowLongW(hwnd, GWL_STYLE, new_style as i32);

    // 5. Tool window style (no separate taskbar item)
    let cur_ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    let new_ex = (cur_ex & !WS_EX_APPWINDOW) | WS_EX_TOOLWINDOW;
    SetWindowLongW(hwnd, GWL_EXSTYLE, new_ex as i32);

    // 6. Set owner window to parent_hwnd so it stays Z-ordered above parent,
    // minimizes/restores with parent, and does NOT get occluded by WebView2 DirectComposition.
    SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, parent_hwnd as isize);

    // 7. Convert client (x, y) coordinates to screen coordinates
    let mut pt = POINT { x, y };
    ClientToScreen(parent_hwnd, &mut pt);

    log_freerdp(&format!(
        "Docking HWND 0x{:X} over parent 0x{:X}: client ({},{}) {}x{} -> screen ({},{}) clamped {}x{}",
        hwnd as usize, parent_hwnd as usize, x, y, width, height, pt.x, pt.y, clamped_w, clamped_h
    ));

    // 8. Position precisely over the canvas boundary
    BringWindowToTop(hwnd);
    SetWindowPos(
        hwnd,
        0 as HWND, // HWND_TOP
        pt.x,
        pt.y,
        clamped_w,
        clamped_h,
        SWP_SHOWWINDOW | SWP_FRAMECHANGED | SWP_NOACTIVATE,
    );
    ShowWindow(hwnd, SW_SHOW);
}

impl FreeRdpSession {
    /// Launches FreeRDP and docks it into a native container window inside parent_window.
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

        // Size & Smart-Sizing
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
            .arg("/bpp:32")
            .arg("/log-level:DEBUG");

        // Pipe FreeRDP process output to temporary debug log for complete transparency
        let freerdp_log_path = std::env::temp_dir().join("freerdp_process.log");
        if let Ok(file) = std::fs::File::create(&freerdp_log_path) {
            if let Ok(file_err) = file.try_clone() {
                cmd.stdout(Stdio::from(file));
                cmd.stderr(Stdio::from(file_err));
            } else {
                cmd.stdout(Stdio::null()).stderr(Stdio::null());
            }
        } else {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
        cmd.stdin(Stdio::null());

        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW = 0x08000000 to prevent console flash
            cmd.creation_flags(0x08000000);
        }

        let child = cmd.spawn().context("Failed to spawn FreeRDP process")?;
        let child_pid = child.id().unwrap_or(0);
        log_freerdp(&format!("FreeRDP session started with PID: {}", child_pid));

        let child_hwnd = Arc::new(parking_lot::Mutex::new(0));
        let last_bounds = Arc::new(parking_lot::Mutex::new((config.x, config.y, config.width, config.height)));
        let is_connected = Arc::new(AtomicBool::new(true));

        #[cfg(windows)]
        {
            let hwnd_clone = child_hwnd.clone();
            let bounds_clone = last_bounds.clone();
            let is_conn_clone = is_connected.clone();
            let parent_hwnd = config.parent_window;

            tokio::spawn(async move {
                log_freerdp(&format!("Starting async discovery for PID {} under owner HWND 0x{:X}", child_pid, parent_hwnd));
                // FreeRDP creates its window only after completing RDP handshake (~1s).
                // Poll every 100ms for up to 10 seconds to discover and dock it.
                for attempt in 1..=100 {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    if !is_conn_clone.load(Ordering::SeqCst) {
                        log_freerdp("Session terminated, stopping window discovery");
                        break;
                    }
                    if *hwnd_clone.lock() != 0 {
                        break;
                    }
                    let discovered = find_freerdp_hwnd(parent_hwnd as HWND, child_pid);
                    if !discovered.is_null() {
                        let h = discovered as isize;
                        *hwnd_clone.lock() = h;
                        let (x, y, w, height) = *bounds_clone.lock();
                        log_freerdp(&format!("Discovered FreeRDP HWND: 0x{:X} on attempt {}", h, attempt));
                        unsafe {
                            apply_window_docking(
                                parent_hwnd as HWND,
                                discovered,
                                x,
                                y,
                                w,
                                height,
                            );
                        }
                        log_freerdp(&format!("Successfully docked and showed FreeRDP HWND 0x{:X}", h));
                        break;
                    }
                }
            });
        }

        Ok(Self {
            host: config.host,
            port: config.port,
            parent_hwnd: config.parent_window,
            child_pid,
            child_hwnd,
            last_bounds,
            is_connected,
            child_process: parking_lot::Mutex::new(Some(child)),
        })
    }

    /// Synchronize the FreeRDP window position on the monitor using saved bounds.
    pub fn sync_screen_position(&self) {
        let (x, y, w, h) = *self.last_bounds.lock();
        let _ = self.update_bounds(x, y, w, h);
    }

    /// Dynamically reposition and resize the embedded FreeRDP window to match the tab layout.
    pub fn update_bounds(&self, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
        *self.last_bounds.lock() = (x, y, width, height);
        #[cfg(windows)]
        {
            let mut hwnd = *self.child_hwnd.lock();
            if hwnd == 0 && self.child_pid != 0 {
                let discovered = find_freerdp_hwnd(self.parent_hwnd as HWND, self.child_pid);
                if !discovered.is_null() {
                    hwnd = discovered as isize;
                    *self.child_hwnd.lock() = hwnd;
                    log_freerdp(&format!("update_bounds discovered FreeRDP HWND: 0x{:X}", hwnd));
                }
            }
            if hwnd != 0 && self.parent_hwnd != 0 {
                unsafe {
                    apply_window_docking(
                        self.parent_hwnd as HWND,
                        hwnd as HWND,
                        x,
                        y,
                        width,
                        height,
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
            let hwnd = *self.child_hwnd.lock();
            if hwnd != 0 {
                unsafe {
                    if visible {
                        ShowWindow(hwnd as HWND, SW_SHOW);
                        self.sync_screen_position();
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
