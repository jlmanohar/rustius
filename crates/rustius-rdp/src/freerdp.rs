// FreeRDP Session Management with Native Container Window Embedding.
// Hosts FreeRDP (wfreerdp on Windows / xfreerdp on Linux) in a native child
// container window for hardware-accelerated Direct3D / H.264 / 60+ FPS performance.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tracing::info;

#[cfg(windows)]
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
#[cfg(windows)]
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, EnumChildWindows, EnumWindows, GetClientRect, GetParent,
    GetWindowLongW, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetParent,
    SetWindowLongPtrW, SetWindowLongW, SetWindowPos, ShowWindow, GWL_EXSTYLE, GWL_STYLE,
    GWLP_HWNDPARENT, SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW, WS_CAPTION, WS_CHILD, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_MAXIMIZEBOX,
    WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME, WS_VISIBLE,
};

/// Diagnostic information explaining why a FreeRDP session ended or failed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdpExitDiagnostic {
    pub code: i32,
    pub title: String,
    pub message: String,
    pub severity: String, // "security", "auth", "error", "info"
    pub raw_error: Option<String>,
}

/// File-based and console diagnostics helper for FreeRDP embedding
pub fn log_freerdp(msg: &str) {
    if msg.contains("[ERROR]") {
        tracing::error!(target: "rustius_rdp", "{}", msg);
    } else if msg.contains("[WARN]") {
        tracing::warn!(target: "rustius_rdp", "{}", msg);
    } else {
        tracing::info!(target: "rustius_rdp", "{}", msg);
    }

    let log_path = std::env::temp_dir().join("rustius_freerdp.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log_path) {
        use std::io::Write;
        let _ = writeln!(f, "[FreeRDP] {}", msg);
    }

    let proc_log_path = std::env::temp_dir().join("freerdp_process.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(proc_log_path) {
        use std::io::Write;
        let _ = writeln!(f, "{}", msg);
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
    #[serde(default)]
    pub desktop_width: Option<u32>,
    #[serde(default)]
    pub desktop_height: Option<u32>,
    #[serde(default)]
    pub scale_desktop: Option<u32>,
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
    is_docked: Arc<AtomicBool>,
    pub is_visible: Arc<AtomicBool>,
    child_process: parking_lot::Mutex<Option<Child>>,
    recent_logs: Arc<parking_lot::Mutex<VecDeque<String>>>,
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
        // Accept windows with valid size
        if visible || (width > 100 && height > 100) {
            target.1 = hwnd;
            return 0; // stop enumeration: display window found
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
    docked: bool,
) {
    if hwnd.is_null() || parent_hwnd.is_null() {
        return;
    }

    if !docked {
        // Detached / Popout Mode: Restore standard native window with titlebar, minimize, maximize, close 'X'
        let cur_parent = GetParent(hwnd);
        if cur_parent != 0 as HWND {
            SetParent(hwnd, 0 as HWND);
        }
        SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, 0);

        let cur_style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        let new_style = (cur_style & !WS_CHILD)
            | WS_POPUP
            | WS_CAPTION
            | WS_THICKFRAME
            | WS_MINIMIZEBOX
            | WS_MAXIMIZEBOX
            | WS_SYSMENU
            | WS_VISIBLE;
        SetWindowLongW(hwnd, GWL_STYLE, new_style as i32);

        let cur_ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let new_ex = (cur_ex & !WS_EX_TOOLWINDOW) | WS_EX_APPWINDOW;
        SetWindowLongW(hwnd, GWL_EXSTYLE, new_ex as i32);

        SetWindowPos(
            hwnd,
            0 as HWND,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
        );
        ShowWindow(hwnd, SW_SHOW);
        return;
    }

    // Embedded Docked Mode: Constrain strictly within tab viewport boundaries
    if IsIconic(parent_hwnd) != 0 {
        if IsWindowVisible(hwnd) != 0 {
            ShowWindow(hwnd, SW_HIDE);
        }
        return;
    }

    let mut client_rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetClientRect(parent_hwnd, &mut client_rect);
    let parent_w = client_rect.right - client_rect.left;
    let parent_h = client_rect.bottom - client_rect.top;

    if parent_w <= 0 || parent_h <= 0 || x >= parent_w || y >= parent_h {
        if IsWindowVisible(hwnd) != 0 {
            ShowWindow(hwnd, SW_HIDE);
        }
        return;
    }

    let clamped_w = width.min(parent_w - x).max(0);
    let clamped_h = height.min(parent_h - y).max(0);

    if clamped_w <= 10 || clamped_h <= 10 {
        if IsWindowVisible(hwnd) != 0 {
            ShowWindow(hwnd, SW_HIDE);
        }
        return;
    }

    let cur_parent = GetParent(hwnd);
    if cur_parent != 0 as HWND {
        SetParent(hwnd, 0 as HWND);
    }

    // Style as borderless popup, but KEEP WS_SYSMENU so OS shortcut handling remains intact
    let cur_style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    let new_style = (cur_style & !(WS_CAPTION | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_CHILD))
        | WS_POPUP
        | WS_VISIBLE
        | WS_SYSMENU;
    SetWindowLongW(hwnd, GWL_STYLE, new_style as i32);

    let cur_ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    let new_ex = (cur_ex & !WS_EX_APPWINDOW) | WS_EX_TOOLWINDOW;
    SetWindowLongW(hwnd, GWL_EXSTYLE, new_ex as i32);

    // Set owner window to parent_hwnd so it moves and minimizes with parent
    SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, parent_hwnd as isize);

    let mut pt = POINT { x, y };
    ClientToScreen(parent_hwnd, &mut pt);

    log_freerdp(&format!(
        "Docking HWND 0x{:X} over parent 0x{:X}: client ({},{}) {}x{} -> screen ({},{}) clamped {}x{}",
        hwnd as usize, parent_hwnd as usize, x, y, width, height, pt.x, pt.y, clamped_w, clamped_h
    ));

    SetWindowPos(
        hwnd,
        0 as HWND,
        pt.x,
        pt.y,
        clamped_w,
        clamped_h,
        SWP_SHOWWINDOW | SWP_FRAMECHANGED | SWP_NOACTIVATE,
    );
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

        // Size & Smart-Sizing: scale remote desktop dynamically to window size
        let session_w = config.desktop_width.unwrap_or(config.width.max(100) as u32).max(100);
        let session_h = config.desktop_height.unwrap_or(config.height.max(100) as u32).max(100);
        cmd.arg(format!("/size:{}x{}", session_w, session_h));

        // Performance, Scaling, and Hardware Acceleration Flags
        if let Some(scale) = config.scale_desktop {
            if (100..=500).contains(&scale) {
                cmd.arg(format!("/scale-desktop:{}", scale));
            }
        }

        cmd.arg("/smart-sizing")
            .arg("-toggle-fullscreen")
            .arg("/cert:ignore")
            .arg("+clipboard")
            .arg("/sound")
            .arg("/gfx")
            .arg("/network:auto")
            .arg("/bpp:32")
            .arg("/log-level:INFO");

        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW = 0x08000000 to prevent console flash
            cmd.creation_flags(0x08000000);
        }

        let mut child = cmd.spawn().context("Failed to spawn FreeRDP process")?;
        let child_pid = child.id().unwrap_or(0);
        log_freerdp(&format!("FreeRDP session started with PID: {}", child_pid));

        let recent_logs = Arc::new(parking_lot::Mutex::new(VecDeque::with_capacity(100)));
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        if let Some(stdout) = stdout {
            let recent = recent_logs.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    log_freerdp(&line);
                    let mut buf = recent.lock();
                    if buf.len() >= 100 {
                        buf.pop_front();
                    }
                    buf.push_back(line);
                }
            });
        }

        if let Some(stderr) = stderr {
            let recent = recent_logs.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    log_freerdp(&line);
                    let mut buf = recent.lock();
                    if buf.len() >= 100 {
                        buf.pop_front();
                    }
                    buf.push_back(line);
                }
            });
        }

        let child_hwnd = Arc::new(parking_lot::Mutex::new(0));
        let last_bounds = Arc::new(parking_lot::Mutex::new((config.x, config.y, config.width, config.height)));
        let is_connected = Arc::new(AtomicBool::new(true));
        let is_docked = Arc::new(AtomicBool::new(true));
        let is_visible = Arc::new(AtomicBool::new(true));

        #[cfg(windows)]
        {
            let hwnd_clone = child_hwnd.clone();
            let bounds_clone = last_bounds.clone();
            let is_conn_clone = is_connected.clone();
            let is_docked_clone = is_docked.clone();
            let is_visible_clone = is_visible.clone();
            let parent_hwnd = config.parent_window;

            // Continuous monitor loop: discovers FreeRDP HWND and enforces boundary containment
            tokio::spawn(async move {
                log_freerdp(&format!("Starting async monitor for PID {} under owner HWND 0x{:X}", child_pid, parent_hwnd));
                let mut last_applied_rect = (0i32, 0i32, 0i32, 0i32);

                while is_conn_clone.load(Ordering::SeqCst) {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

                    let mut current_hwnd = *hwnd_clone.lock();
                    if current_hwnd == 0 {
                        let discovered = find_freerdp_hwnd(parent_hwnd as HWND, child_pid);
                        if !discovered.is_null() {
                            current_hwnd = discovered as isize;
                            *hwnd_clone.lock() = current_hwnd;
                            log_freerdp(&format!("Discovered FreeRDP HWND: 0x{:X}", current_hwnd));
                        }
                    }

                    if current_hwnd != 0 && parent_hwnd != 0 {
                        unsafe {
                            if IsIconic(parent_hwnd as HWND) != 0 {
                                continue;
                            }
                        }

                        let visible = is_visible_clone.load(Ordering::SeqCst);
                        if !visible {
                            unsafe {
                                if IsWindowVisible(current_hwnd as HWND) != 0 {
                                    ShowWindow(current_hwnd as HWND, SW_HIDE);
                                }
                            }
                            continue;
                        }

                        let docked = is_docked_clone.load(Ordering::SeqCst);
                        if docked {
                            let (x, y, w, h) = *bounds_clone.lock();
                            let mut cur_rect: RECT = unsafe { std::mem::zeroed() };
                            unsafe { GetWindowRect(current_hwnd as HWND, &mut cur_rect) };

                            let mut target_pt = POINT { x, y };
                            unsafe { ClientToScreen(parent_hwnd as HWND, &mut target_pt) };

                            let mut client_rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                            unsafe { GetClientRect(parent_hwnd as HWND, &mut client_rect) };
                            let parent_w = client_rect.right - client_rect.left;
                            let parent_h = client_rect.bottom - client_rect.top;
                            if parent_w <= 10 || parent_h <= 10 {
                                continue;
                            }
                            let expected_w = w.min(parent_w - x).max(0);
                            let expected_h = h.min(parent_h - y).max(0);

                            // Detect if FreeRDP moved to (0,0) or changed size internally upon connection
                            let pos_mismatch = (cur_rect.left - target_pt.x).abs() > 4
                                || (cur_rect.top - target_pt.y).abs() > 4
                                || ((cur_rect.right - cur_rect.left) - expected_w).abs() > 4
                                || ((cur_rect.bottom - cur_rect.top) - expected_h).abs() > 4;

                            if pos_mismatch || (x, y, w, h) != last_applied_rect {
                                last_applied_rect = (x, y, w, h);
                                unsafe {
                                    apply_window_docking(
                                        parent_hwnd as HWND,
                                        current_hwnd as HWND,
                                        x,
                                        y,
                                        w,
                                        h,
                                        true,
                                    );
                                }
                            }
                        }
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
            is_docked,
            is_visible,
            child_process: parking_lot::Mutex::new(Some(child)),
            recent_logs,
        })
    }

    /// Toggle between embedded docked mode and detached native window mode.
    pub fn set_docked(&self, docked: bool) -> Result<()> {
        self.is_docked.store(docked, Ordering::SeqCst);
        #[cfg(windows)]
        {
            let hwnd = *self.child_hwnd.lock();
            if hwnd != 0 && self.parent_hwnd != 0 {
                let (x, y, w, h) = *self.last_bounds.lock();
                unsafe {
                    apply_window_docking(
                        self.parent_hwnd as HWND,
                        hwnd as HWND,
                        x,
                        y,
                        w,
                        h,
                        docked,
                    );
                }
            }
        }
        Ok(())
    }

    /// Synchronize the FreeRDP window position on the monitor using saved bounds.
    pub fn sync_screen_position(&self) {
        #[cfg(windows)]
        {
            let hwnd = *self.child_hwnd.lock();
            if hwnd != 0 && !self.is_visible.load(Ordering::SeqCst) {
                unsafe {
                    ShowWindow(hwnd as HWND, SW_HIDE);
                }
                return;
            }
        }
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
                if !self.is_visible.load(Ordering::SeqCst) {
                    unsafe {
                        ShowWindow(hwnd as HWND, SW_HIDE);
                    }
                    return Ok(());
                }

                if self.is_docked.load(Ordering::SeqCst) {
                    unsafe {
                        apply_window_docking(
                            self.parent_hwnd as HWND,
                            hwnd as HWND,
                            x,
                            y,
                            width,
                            height,
                            true,
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Show or hide the native embedded window when switching tabs.
    pub fn set_visibility(&self, visible: bool) -> Result<()> {
        self.is_visible.store(visible, Ordering::SeqCst);
        #[cfg(windows)]
        {
            let hwnd = *self.child_hwnd.lock();
            if hwnd != 0 {
                unsafe {
                    if visible {
                        if IsWindowVisible(hwnd as HWND) == 0 {
                            ShowWindow(hwnd as HWND, SW_SHOWNOACTIVATE);
                        }
                    } else {
                        if IsWindowVisible(hwnd as HWND) != 0 {
                            ShowWindow(hwnd as HWND, SW_HIDE);
                        }
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

    /// Analyze the exit status and buffered process output to produce a user-facing diagnostic.
    pub fn get_exit_diagnostic(&self, code: i32) -> RdpExitDiagnostic {
        let logs: Vec<String> = self.recent_logs.lock().iter().cloned().collect();
        describe_freerdp_exit(code, &logs)
    }
}

/// Translate FreeRDP exit code and captured process stderr/stdout into a friendly diagnostic.
pub fn describe_freerdp_exit(code: i32, logs: &[String]) -> RdpExitDiagnostic {
    if code == 0 {
        return RdpExitDiagnostic {
            code: 0,
            title: "Session Disconnected".to_string(),
            message: "Remote Desktop session ended normally by user or server.".to_string(),
            severity: "info".to_string(),
            raw_error: None,
        };
    }

    // Extract the most descriptive error line from recent logs
    let raw_error = logs.iter().rev().find(|l| {
        l.contains("[ERROR]")
            || l.contains("ERRCONNECT_")
            || l.contains("STATUS_")
            || l.contains("failure")
            || l.contains("denied")
            || l.contains("rejected")
    }).map(|s| s.trim().to_string());

    let logs_joined = logs.iter().rev().take(40).cloned().collect::<Vec<_>>().join("\n").to_lowercase();
    let ucode = code as u32;

    // 1. Account Locked Out
    if ucode == 0x00020017
        || code == 131095
        || logs_joined.contains("status_account_locked_out")
        || logs_joined.contains("0xc0000234")
        || logs_joined.contains("errconnect_account_locked_out")
        || logs_joined.contains("account locked out")
        || logs_joined.contains("locked out for security")
        || logs_joined.contains("account is locked")
    {
        return RdpExitDiagnostic {
            code,
            title: "Account Locked Out".to_string(),
            message: "The user account is locked out for security reasons and cannot log in. Please contact your system administrator to unlock it.".to_string(),
            severity: "security".to_string(),
            raw_error,
        };
    }

    // 2. Logon Failure / Bad Credentials
    if ucode == 0x00020014
        || code == 131092
        || logs_joined.contains("status_logon_failure")
        || logs_joined.contains("0xc000006a")
        || logs_joined.contains("errconnect_logon_failure")
        || logs_joined.contains("authentication failure, check credentials")
        || logs_joined.contains("logon failure")
    {
        return RdpExitDiagnostic {
            code,
            title: "Logon Failed (Invalid Credentials)".to_string(),
            message: "Incorrect username or password. Please verify your credentials and domain, then try again.".to_string(),
            severity: "auth".to_string(),
            raw_error,
        };
    }

    // 3. Password Expired
    if ucode == 0x00020015
        || code == 131093
        || logs_joined.contains("status_password_expired")
        || logs_joined.contains("0xc0000071")
        || logs_joined.contains("errconnect_password_expired")
        || logs_joined.contains("password has expired")
    {
        return RdpExitDiagnostic {
            code,
            title: "Password Expired".to_string(),
            message: "The user account password has expired and must be updated before connecting.".to_string(),
            severity: "auth".to_string(),
            raw_error,
        };
    }

    // 4. Password Must Change
    if ucode == 0x00020018
        || code == 131096
        || logs_joined.contains("status_password_must_change")
        || logs_joined.contains("0xc0000224")
        || logs_joined.contains("errconnect_password_must_change")
        || logs_joined.contains("must change password")
    {
        return RdpExitDiagnostic {
            code,
            title: "Password Must Be Changed".to_string(),
            message: "The user is required to change their password at next logon before establishing a session.".to_string(),
            severity: "auth".to_string(),
            raw_error,
        };
    }

    // 5. Account Disabled
    if ucode == 0x00020016
        || code == 131094
        || logs_joined.contains("status_account_disabled")
        || logs_joined.contains("0xc0000072")
        || logs_joined.contains("errconnect_account_disabled")
        || logs_joined.contains("account is disabled")
    {
        return RdpExitDiagnostic {
            code,
            title: "Account Disabled".to_string(),
            message: "The user account is currently disabled on the remote host or domain.".to_string(),
            severity: "security".to_string(),
            raw_error,
        };
    }

    // 6. Logon Type Not Granted (Remote Desktop Users membership)
    if ucode == 0x0002001B
        || code == 131099
        || logs_joined.contains("status_logon_type_not_granted")
        || logs_joined.contains("0xc000015b")
        || logs_joined.contains("errconnect_logon_type_not_granted")
        || logs_joined.contains("logon type not granted")
    {
        return RdpExitDiagnostic {
            code,
            title: "Remote Logon Not Granted".to_string(),
            message: "The user account is not permitted to log on remotely. Ensure the user belongs to the 'Remote Desktop Users' or 'Administrators' group.".to_string(),
            severity: "security".to_string(),
            raw_error,
        };
    }

    // 7. Account Restriction (Hours / Workstation)
    if ucode == 0x00020019
        || code == 131097
        || logs_joined.contains("status_account_restriction")
        || logs_joined.contains("0xc000006e")
        || logs_joined.contains("errconnect_account_restriction")
    {
        return RdpExitDiagnostic {
            code,
            title: "Account Restricted".to_string(),
            message: "Logon rejected due to user account policy restrictions (such as unauthorized logon hours or restricted workstation).".to_string(),
            severity: "security".to_string(),
            raw_error,
        };
    }

    // 8. Account Expired
    if ucode == 0x0002001A
        || code == 131098
        || logs_joined.contains("status_account_expired")
        || logs_joined.contains("0xc0000193")
        || logs_joined.contains("errconnect_account_expired")
    {
        return RdpExitDiagnostic {
            code,
            title: "Account Expired".to_string(),
            message: "The user account has reached its expiration date and cannot be used.".to_string(),
            severity: "security".to_string(),
            raw_error,
        };
    }

    // 9. Access Denied
    if ucode == 0x00020020
        || code == 131104
        || logs_joined.contains("status_access_denied")
        || logs_joined.contains("0xc0000022")
        || logs_joined.contains("errconnect_access_denied")
    {
        return RdpExitDiagnostic {
            code,
            title: "Access Denied".to_string(),
            message: "Access was denied by the remote computer or network policy.".to_string(),
            severity: "security".to_string(),
            raw_error,
        };
    }

    // 10. Security Negotiation / NLA Failure
    if ucode == 0x0002000D
        || ucode == 0x0002000C
        || code == 131085
        || code == 131084
        || logs_joined.contains("errconnect_security_nego_connect_failed")
        || logs_joined.contains("security negotiation failure")
        || logs_joined.contains("security negotiation failed")
    {
        return RdpExitDiagnostic {
            code,
            title: "Security Negotiation Failed".to_string(),
            message: "Failed to negotiate security with remote host. Network Level Authentication (NLA) or TLS negotiation was rejected.".to_string(),
            severity: "error".to_string(),
            raw_error,
        };
    }

    // 11. TLS / SSL Handshake Failure
    if ucode == 0x00020006
        || ucode == 0x0002000F
        || code == 131078
        || code == 131087
        || logs_joined.contains("tls_connect_failed")
        || logs_joined.contains("ssl_do_handshake")
    {
        return RdpExitDiagnostic {
            code,
            title: "TLS Connection Failed".to_string(),
            message: "Failed to establish an encrypted TLS/SSL session with the remote host.".to_string(),
            severity: "error".to_string(),
            raw_error,
        };
    }

    // 12. Host Unreachable / DNS / Connection Refused / Timeout
    if ucode == 0x00020004
        || ucode == 0x00020009
        || ucode == 0x0002000A
        || ucode == 0x00020002
        || ucode == 0x00020003
        || code == 131081
        || code == 131082
        || code == 131076
        || logs_joined.contains("getaddrinfo")
        || logs_joined.contains("host not found")
        || logs_joined.contains("connection refused")
        || logs_joined.contains("wsaeconnrefused")
        || logs_joined.contains("wsaetimedout")
        || logs_joined.contains("timed out")
        || logs_joined.contains("connect_transport_failed")
    {
        return RdpExitDiagnostic {
            code,
            title: "Connection Failed".to_string(),
            message: "Could not connect to remote host. Verify the hostname/IP is correct, port 3389 is open, and Remote Desktop is enabled on the server.".to_string(),
            severity: "error".to_string(),
            raw_error,
        };
    }

    // 13. Licensing
    if ucode == 0x0002001C
        || code == 131100
        || logs_joined.contains("errconnect_no_or_revoked_license")
    {
        return RdpExitDiagnostic {
            code,
            title: "Licensing Error".to_string(),
            message: "Remote Desktop Services licensing error (no client access license or license was revoked).".to_string(),
            severity: "error".to_string(),
            raw_error,
        };
    }

    // 14. Fallback
    RdpExitDiagnostic {
        code,
        title: "Remote Desktop Error".to_string(),
        message: format!("FreeRDP exited with code {} (0x{:08X}).", code, ucode),
        severity: "error".to_string(),
        raw_error,
    }
}

impl Drop for FreeRdpSession {
    fn drop(&mut self) {
        self.disconnect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_describe_freerdp_clean_exit() {
        let diag = describe_freerdp_exit(0, &[]);
        assert_eq!(diag.code, 0);
        assert_eq!(diag.severity, "info");
    }

    #[test]
    fn test_describe_freerdp_account_locked_by_code() {
        let diag = describe_freerdp_exit(131095, &[]);
        assert_eq!(diag.title, "Account Locked Out");
        assert_eq!(diag.severity, "security");
        assert!(diag.message.contains("locked out for security"));
    }

    #[test]
    fn test_describe_freerdp_account_locked_by_log() {
        let logs = vec![
            "[ERROR][com.freerdp.core.nego] - Security Negotiation failed (NTSTATUS 0xc0000234 - STATUS_ACCOUNT_LOCKED_OUT)".to_string(),
        ];
        let diag = describe_freerdp_exit(1, &logs);
        assert_eq!(diag.title, "Account Locked Out");
        assert_eq!(diag.severity, "security");
        assert_eq!(diag.raw_error, Some(logs[0].clone()));
    }

    #[test]
    fn test_describe_freerdp_logon_failure() {
        let logs = vec![
            "[ERROR][com.freerdp.core] - freerdp_set_last_error_ex: ERRCONNECT_LOGON_FAILURE [0x00020014]".to_string(),
        ];
        let diag = describe_freerdp_exit(131092, &logs);
        assert_eq!(diag.title, "Logon Failed (Invalid Credentials)");
        assert_eq!(diag.severity, "auth");
    }

    #[test]
    fn test_describe_freerdp_connection_refused() {
        let logs = vec![
            "[ERROR][com.freerdp.core] - connect: connection refused to 192.168.1.100:3389".to_string(),
        ];
        let diag = describe_freerdp_exit(131081, &logs);
        assert_eq!(diag.title, "Connection Failed");
        assert_eq!(diag.severity, "error");
    }
}

