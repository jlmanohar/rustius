# Deep Research Report: RDP with Full GPU Acceleration + TLS (Rust Backend)
> Based on codebase analysis (`rustius-rdp`), `crates.io`, `freerdp` docs, and `IronRDP` source.

---

## 1. Your Current Approach (`rustius`)

**Architecture (verified from `freerdp.rs` + `client.rs`):**

| Layer | Implementation | GPU Path | TLS/Handshake |
|---|---|---|---|
| Protocol | `IronRDP` (`rustius-rdp-worker` binary) | `softbuffer` / `wgpu` via `ironrdp-egfx` | `rustls` / `native-tls` (`ironrdp-tls`) |
| Embedding | Spawns `wfreerdp` / `xfreerdp` binary; discovers `HWND`; reparents with `SetParent` | Native Direct3D / H.264 via FreeRDP executable | FreeRDP binary handles TLS + CredSSP internally |
| Communication | Base64-encoded `RGBA` frames over stdout/stdin (magic headers `RDPF`/`RDPC`) | Frame-by-frame CPU decode; no GPU compositing of full screen unless custom `wgpu` renderer added | None in your code — delegated to binary |

**Key code patterns found:**
- `FreeRdpConfig` passes `/v:`, `/u:`, `/p:`, `/gfx`, `/network:auto`, `/cert:ignore`, `/size:` to binary.
- `find_freerdp_hwnd()` uses `EnumChildWindows` + `GetWindowThreadProcessId` to locate the embedded window.
- Docking uses `SetParent` + `SetWindowLongW` (remove `WS_CAPTION`, `WS_THICKFRAME`) + `SetWindowPos`.
- The IronRDP worker uses `mpsc` channels (`frame_tx`) to stream base64 pixels to frontend.

---

## 2. Approach A: FreeRDP Binary Embedding (Your `freerdp.rs` Style) — Best Near-Native Performance

### How GPU Acceleration Works
- **FreeRDP binary (`wfreerdp.exe`)** uses the host GPU for **decoding** (NVDEC/VAAPI) and **encoding** (NVENC/VAAPI) of H.264/AVC444 streams.
- Command-line flags: `/gfx:h264`, `/gfx-avc444`, `/gdi:hw` (hardware GDI), `/cert:ignore`.
- You don't render pixels. The binary writes to its own `HWND`; you just dock/reparent it.

### TLS Handshake (Modern Windows / CredSSP)
- FreeRDP binary handles `X.224` → `TLS` (`SSL`) → `CredSSP` (`NTLM` / `Kerberos`) internally.
- Modern Windows 11 requires `TLS 1.2+` + `CredSSP` with `NLA`. FreeRDP supports `/sec:nla`, `/sec:tls`, `/cert-tofu` (TOFU), `/cert-ignore`.
- **No Rust TLS library needed** in this path — it's fully delegated to the binary.

### Pros / Cons
| Pros | Cons |
|---|---|
| True native GPU decode/render (Direct3D, H.264) | Binary dependency (`wfreerdp.exe` / `xfreerdp`) |
| Near-zero CPU for display (GPU handles decompression) | No fine-grained Rust-level protocol control |
| Mature TLS + CredSSP (tested against Win11) | HWND docking is Windows-specific; Linux needs X11 reparenting |
| Easy to bundle (`resources/binaries/freerdp/`) | Frame capture for your own canvas requires screenshot (`BitBlt`) or offscreen capture |

---

## 3. Approach B: Pure Rust Protocol + GPU Render (`IronRDP` Style)

### How GPU Acceleration Works
- `IronRDP` decodes graphics PDUs (`BitmapUpdate`, `RFX`, `ZGFX`) in pure Rust (`ironrdp-graphics`).
- For GPU rendering, upload `DecodedImage` pixels to a `wgpu` texture (`ImageCopyTexture`).
- `ironrdp-egfx` (Graphics Pipeline Extension) supports H.264 encoding/decoding via DVC, but requires server-side `MS-RDPEGFX` support.
- **Performance note**: No automatic hardware video decode by default; you must implement `wgpu` upload or use `softbuffer`. CPU usage is higher than FreeRDP binary for large 4K screens.

### TLS Handshake (`ironrdp-tls`)
- `rustls` (pure Rust, memory-safe) or `native-tls` (Windows `SChannel`, macOS `SecureTransport`, Linux `OpenSSL`).
- Explicit backend selection (`rustls` vs `native-tls`) via Cargo feature flags (`mutually exclusive`).
- Config: `.dangerous().with_custom_certificate_verifier(NoCertificateVerification).with_no_client_auth()` + `resumption = disabled()` (required for CredSSP).
- Extract `public_key` from TLS cert for RDP protocol validation (`extract_tls_server_public_key`).

### Pros / Cons
| Pros | Cons |
|---|---|
| Fully Rust — no binary dependency | Slower/performance bugs reported (`resize` bugs, single-threaded PDU parsing in web clients) |
| Fine protocol control (custom PDU handlers, custom codecs) | More bandwidth than FreeRDP (per user reports) |
| Cross-platform (Windows, Linux, WASM) | GPU acceleration is manual (`wgpu` texture upload); no automatic NVDEC for H.264 stream |
| `rustls` provides memory safety guarantees | Less mature TLS/Cert verification story than FreeRDP |

---

## 4. Approach C: `freerdp-sys` (Direct FFI Bindings) — Not Used in Your Project

- `freerdp-sys` crate (`crates.io/crates/freerdp-sys`) provides `bindgen`-generated FFI to FreeRDP 3.x C library.
- Would allow direct `wfreerdp` API calls from Rust (not spawning a binary).
- Enables true library-level GPU usage (`freerdp_peer` API) but requires linking against C library.
- **Not adopted by your project** — you use binary spawning instead.

---

## 5. Detailed Comparison: Your Approach vs Alternatives

| Feature | `rustius` (Hybrid) | FreeRDP Binary Only | `IronRDP` Only |
|---|---|---|---|
| **Rust Backend** | ✅ Yes (protocol in Rust) | ❌ Binary executed by Rust shell | ✅ Pure Rust |
| **GPU Decode** | ✅ Native (binary) | ✅ Native | ⚠️ Manual (`wgpu`) |
| **GPU Render** | ⚠️ Binary renders own window; you embed it | ⚠️ Binary renders own window | ✅ Custom GPU texture |
| **TLS 1.2/1.3** | ✅ Delegated to binary | ✅ Native FreeRDP TLS | ✅ `rustls` / `native-tls` |
| **CredSSP/NLA** | ✅ Delegated | ✅ Native | ✅ Implemented (`sspi` crate integration) |
| **HWND Embedding** | ✅ `EnumChildWindows` + `SetParent` | ✅ Same technique | N/A (no binary) |
| **Cross-Platform** | ⚠️ Windows (`HWND`); Linux needs X11 adaptation | ⚠️ Same | ✅ Linux/macOS/Windows/WASM |
| **Performance** | Near-native (best) | Near-native | Good but lower than FreeRDP (user reports) |
| **Maintainability** | Complex (Rust protocol + binary docking) | Simple (just spawn binary) | Complex (full protocol in Rust) |

---

## 6. Recommendations for Your Goal (Near-Native Performance + Rust Backend)

### If you want **maximum performance** with minimal Rust protocol work:
- **Keep FreeRDP binary embedding (`freerdp.rs` style).**
- Use `/gfx:h264`, `/gfx-avc444`, `/cert-ignore`, `/sec:nla` for modern Windows.
- Capture the screen for your app by reading the embedded `HWND` with `BitBlt` or `PrintWindow` if you need pixels in your own canvas; otherwise just let the binary render directly in your window (fastest).

### If you want **pure Rust control** with GPU rendering:
- Use `IronRDP` + `rustls` (`ironrdp-tls` feature `rustls`).
- Implement GPU upload in `ActiveStageOutput::GraphicsUpdate` handler (upload `image.data()` to `wgpu` texture).
- Note: This requires implementing or using `ironrdp-egfx` for H.264 GPU encoding/decoding; standard `IronRDP` uses CPU-decoded bitmaps (`BitmapData`).
- Use `Config { enable_tls: true, enable_credssp: true, ... }` for modern Windows TLS + NLA.

### TLS Best Practice for Modern Windows (11 / Server 2022):
- **Always use `CredSSP` (`/sec:nla`)** — modern Windows disables standard RDP security.
- **Always use TLS** (`/sec:tls` or `enable_tls: true`) — required for CredSSP.
- **Disable session resumption** (`resumption = disabled()`) — CredSSP doesn't support it (per MS-CSSP spec).
- For FreeRDP binary: `/cert:ignore` (or `/cert-tofu` for known_hosts-style verification) avoids TLS errors with self-signed server certs.

---

## 7. Key References from Research

- `rustius-rdp/src/freerdp.rs`: FreeRDP binary spawning + HWND docking.
- `rustius-rdp/src/client.rs`: IronRDP worker (`rustius-rdp-worker.exe`) with base64 `RDPF` frame streaming.
- `Cargo.toml` (`rustius-rdp`): `ironrdp = { version = "0.17" ... }`.
- `crates.io/crates/freerdp-sys`: Low-level FFI bindings (not used by project).
- `github.com/Devolutions/IronRDP`: `ironrdp-tls`, `ironrdp-connector`, `ironrdp-egfx`.
- `github.com/FreeRDP/FreeRDP`: Native binary GPU acceleration (`VAAPI`, `NVDEC`, `/gfx` flags).

---

*Report compiled from codebase inspection (`C:/Users/jlmanohar/Documents/Projects/rustius/`) + web sources (`exacr.io`, `github.com/FreeRDP/FreeRDP`, `github.com/Devolutions/IronRDP`, `devolutions-ironrdp.mintlify.app`).*
