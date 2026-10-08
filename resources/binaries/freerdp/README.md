# FreeRDP Prebuilt Binaries

This directory contains verified portable FreeRDP executables used by `rustius-rdp` for native hardware-accelerated remote desktop sessions.

- **`wfreerdp.exe`**: FreeRDP 3.28 portable Windows x64 binary. Bundled into Tauri release binaries via `tauri.conf.json` `bundle.resources`.
- **Integration**: Launched as a child process docked directly into a native Win32 container window (`/parent-window:<HWND>`) with dynamic resolution (`+dynamic-resolution`), Direct3D/AVC444 hardware acceleration, and clipboard sync.
