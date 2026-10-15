# Rustius — Full Codebase Audit

**What this repo does:** Rustius is a cross-platform desktop SSH, SFTP, and RDP client built with Tauri v2 (Rust backend) and SvelteKit + Svelte 5 (frontend). It stores connections in a tree hierarchy, manages credentials through OS keyring or an encrypted vault (AES-256-GCM + Argon2id), and embeds FreeRDP as a child window for hardware-accelerated RDP.

**Assumed load:** One user running locally — a desktop app, not a server. Concurrent access is only between Tauri commands on the async runtime (multiple tabs sending data simultaneously).

---

## Must Fix

### 1. **SSH host key verification accepts all servers blindly** ([client.rs:L21-26](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-ssh/src/client.rs#L21-L26), [sftp.rs:L34-39](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-ssh/src/sftp.rs#L34-L39))
- **What this is:** When connecting over SSH (both terminal and SFTP), russh calls `check_server_key` to verify the remote server's identity.
- **Problem:** Both `SshClientHandler` and `SftpClientHandler` unconditionally return `Ok(true)`. This means Rustius silently accepts *any* server key, including from a man-in-the-middle attacker sitting between the user and the real server. The attacker would intercept all data, including credentials typed into the terminal.
- **Fix:** Implement a `known_hosts` file (e.g. `~/.ssh/known_hosts` or Rustius's own `known_hosts.json`). On first connect, show a TOFU prompt ("This host is not in your known hosts. Fingerprint: SHA256:xxx. Trust?"). On subsequent connects, verify the key matches. Extract the shared handler into a common struct in `rustius-ssh` so both `client.rs` and `sftp.rs` use the same verifier — that eliminates the duplicated handler too.
- **If we skip it:** Any network attacker (corporate proxy, café WiFi, DNS hijack) can silently intercept all SSH traffic, capturing passwords, private key passphrases, and all data transmitted over the session.

### 2. **RDP password passed as a command-line argument visible to other processes** ([freerdp.rs:L338](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-rdp/src/freerdp.rs#L336-L340))
- **What this is:** FreeRDP is launched as a child process with the connection password passed via `/p:<password>` CLI argument.
- **Problem:** On Windows, any local user or process can enumerate command lines of running processes via `wmic process get commandline`, Task Manager, or `Get-Process | Select-Object CommandLine`. The plaintext password is visible for the entire duration of the RDP session.
- **Fix:** FreeRDP 3.x supports reading credentials from stdin using `/from-stdin`. Pipe the password through `stdin` instead of the command line. This removes it from the process argument list entirely.
- **If we skip it:** Any process on the same machine (malware, monitoring agents, other users on shared workstations) can harvest RDP passwords from the process list.

### 3. **CSP is disabled — XSS in any frontend path leads to full system compromise** ([tauri.conf.json:L26](file:///c:/Users/jlmanohar/Documents/Projects/rustius/src-tauri/tauri.conf.json#L25-L27))
- **What this is:** Content Security Policy (CSP) controls which scripts, styles, and connections the webview is allowed to execute.
- **Problem:** `"csp": null` means the webview has no CSP at all. If an attacker manages to inject HTML/JS into any rendered content (e.g. a malicious connection name, a crafted SFTP filename, a markdown note with embedded script), they gain full access to `invoke()` — meaning they can execute arbitrary Tauri commands, connect to any server, exfiltrate credentials, and read/write local files.
- **Fix:** Set a strict CSP: `"csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src ipc: http://ipc.localhost"`. This prevents inline script injection. Adjust as needed for any CDN fonts.
- **If we skip it:** A single XSS vector anywhere in the app gives an attacker full control of the desktop application and all stored credentials.

### 4. **Vault re-encrypts and writes the entire file on every single secret mutation** ([secret.rs:L156-164](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-core/src/secret.rs#L156-L164))
- **What this is:** `set_vault_secret` acquires the RwLock, inserts the secret, then calls `save_vault_locked` which re-derives the Argon2 key and re-encrypts all secrets to disk.
- **Problem:** During `secure_connection` or `migrate_vault_backend`, secrets are stored one at a time. If you have 50 connections, that's 50 Argon2id derivations (CPU-heavy) + 50 full file writes. Argon2id with default params takes ~100ms per invocation, so importing or migrating 50 connections would take ~5 seconds of blocking CPU work on the write-lock, freezing all credential reads.
- **Fix:** Add a `set_vault_secret_batch` method or a `save_vault` flag so callers can batch mutations and persist once. `secure_tree_nodes` should collect all secrets, batch-insert them, then save once.
- **If we skip it:** Importing a large number of connections or migrating backends will freeze the app for many seconds.

---

## Should Fix

### 5. **Duplicated SSH authentication logic in `client.rs` and `sftp.rs`** ([client.rs:L72-99](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-ssh/src/client.rs#L72-L99), [sftp.rs:L71-98](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-ssh/src/sftp.rs#L71-L98))
- **What this is:** Both SSH terminal and SFTP connections have the exact same authentication block — password, key file, and agent auth.
- **Problem:** Any bug fix or new auth method (e.g. implementing SSH agent support) must be applied in two places. If one is updated and the other forgotten, users will get inconsistent behavior between terminal and SFTP.
- **Fix:** Extract authentication into a shared function in `auth.rs`:
  ```rust
  pub async fn authenticate(handle: &mut Handle, auth: &AuthMethod) -> Result<()>
  ```
  Both `SshSession::connect` and `SftpClient::connect` call it. This also makes `auth.rs` useful — currently it only contains an unused `describe_auth_method` function.
- **If we skip it:** Divergent auth behavior between SSH and SFTP as the codebase evolves.

### 6. **Duplicated `secure_connection` / `secure_credential` code pattern** ([config.rs:L306-367](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-core/src/config.rs#L306-L367), [config.rs:L371-432](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-core/src/config.rs#L371-L432))
- **What this is:** `secure_connection` and `secure_credential` contain nearly identical match-arm logic for extracting passwords/passphrases from `AuthMethod` and storing them into the appropriate backend.
- **Problem:** ~120 lines of near-identical code. The same pattern is repeated again in `migrate_vault_backend` (~200 more lines of match-arm duplication across Vault→Keyring and Keyring→Vault branches).
- **Fix:** Extract a generic helper: `fn secure_auth_method(&self, auth: &mut AuthMethod, key_prefix: &str)` that handles the Password/KeyFile pattern once. The migration functions should use a separate `fn migrate_secret_ref(sm: &SecretManager, secret_ref: &mut Option<SecretRef>, from: Backend, to: Backend)` helper.
- **If we skip it:** ~300 lines of boilerplate that must be kept in sync across 4+ call sites.

### 7. **`overflow-checks = false` in release profile disables integer overflow safety** ([Cargo.toml:L94](file:///c:/Users/jlmanohar/Documents/Projects/rustius/Cargo.toml#L94))
- **What this is:** Rust's release mode disables overflow checks by default, but `overflow-checks = false` explicitly confirms this.
- **Problem:** While the performance gain from disabling overflow checks is negligible (integer math is not a hot path in a UI app), it removes a safety net. An uncaught integer overflow in SFTP file size calculations, port parsing, or RDP dimension math could produce silently wrong results instead of a clean panic. This is a desktop app, not a game engine — correctness matters more than saving nanoseconds.
- **Fix:** Set `overflow-checks = true` in the release profile. The performance cost is unmeasurable for this type of application.
- **If we skip it:** Silent integer wrapping bugs in release builds that panic in debug builds — the most dangerous kind of bug to debug.

### 8. **Memory leak via `.leak()` in PTY shell detection** ([pty.rs:L42](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-terminal/src/pty.rs#L36-L43))
- **What this is:** On non-Windows platforms, the `$SHELL` environment variable value is `.leak()`-ed to get a `&'static str`.
- **Problem:** `.leak()` permanently leaks the allocated `String`. Every call to `LocalTerminal::spawn()` leaks a new copy. Over many terminal tab open/close cycles, this accumulates leaked memory that is never freed.
- **Fix:** Use a `once_cell::sync::Lazy<String>` or `std::sync::LazyLock<String>` to cache the shell path once. Then borrow from it. Or simply keep it as a `String` and pass `shell.as_str()` to `CommandBuilder::new()`.
- **If we skip it:** ~50 bytes leaked per terminal spawn. Over hundreds of tabs in a long session, it adds up but won't crash the app. Still wrong.

### 9. **`describe_auth_method` function in `auth.rs` is completely unused** ([auth.rs:L1-16](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-ssh/src/auth.rs#L1-L16))
- **What this is:** The entire `auth.rs` module contains one function that maps `AuthMethod` variants to string labels.
- **Problem:** No code in the entire codebase calls it. It was likely intended for logging but was never wired up. The `rustius-ssh` Cargo.toml also lists `async-trait` as a dependency, which is also never imported or used anywhere in the crate.
- **Fix:** Either wire `describe_auth_method` into the SSH connect logging, or delete the function. Remove `async-trait` from `rustius-ssh/Cargo.toml`. When you extract shared auth logic into `auth.rs` (finding #5), this module becomes the right place for it.
- **If we skip it:** Dead code and an unused dependency bloating compile time.

### 10. **`lucide-svelte` and `@lucide/svelte` — two icon libraries for the same icons** ([package.json:L17,L24](file:///c:/Users/jlmanohar/Documents/Projects/rustius/package.json#L16-L25))
- **What this is:** `package.json` lists both `@lucide/svelte` (the new scoped package) and `lucide-svelte` (the old unscoped package).
- **Problem:** Both packages ship the same icons. All actual imports in the codebase use `@lucide/svelte`. `lucide-svelte` is dead weight — it adds ~2MB to `node_modules` and slows `npm install` for no reason.
- **Fix:** `npm uninstall lucide-svelte`.
- **If we skip it:** Wasted disk space and install time. No runtime impact.

### 11. **No SSH keepalive/timeout settings are read from `SshConfig`** ([client.rs:L49-55](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-ssh/src/client.rs#L49-L55))
- **What this is:** `Connection` has an `ssh_config` field with `keepalive_interval`, `connect_timeout_secs`, `legacy_ciphers`, and `legacy_kex` — all user-configurable per connection.
- **Problem:** `SshSession::connect` hardcodes keepalive to 5 seconds, timeout to 12 seconds, and uses default crypto algorithms. The per-connection `SshConfig` struct is completely ignored. A user who configures a 60-second keepalive or enables legacy algorithms for old network switches will see no effect.
- **Fix:** Accept `&SshConfig` in `SshSession::connect` and `SftpClient::connect`. Use `ssh_config.keepalive_interval` (falling back to 5s), `ssh_config.connect_timeout_secs` (falling back to 12s), and configure legacy ciphers/kex when the flags are set.
- **If we skip it:** The SSH settings UI exists but does nothing — users will file bugs.

### 12. **Type discrepancy: frontend uses `conn_type`, Rust uses `connection_type`** ([types/index.ts:L84](file:///c:/Users/jlmanohar/Documents/Projects/rustius/src/lib/types/index.ts#L84), [connection.rs:L211](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-core/src/connection.rs#L211))
- **What this is:** The Rust `Connection` struct serializes as `connection_type`. The TypeScript `Connection` interface has `conn_type` as the primary field, with `connection_type` as an optional alias.
- **Problem:** The `ConnectionRaw` deserialization workaround handles both fields, but this creates ongoing confusion. Frontend code constructs objects with `conn_type`, which is then silently converted. Any new developer will be confused about which name is canonical.
- **Fix:** Pick one name and use it everywhere. Since the Rust side already migrated to `connection_type`, update the TypeScript interface to use `connection_type` as the primary field, remove the `conn_type` alias from frontend code, and keep `ConnectionRaw` for backward-compatible file loading only.
- **If we skip it:** Continued confusion and a ticking time bomb for serialization bugs.

### 13. **No tests for SSH, SFTP, or RDP crate logic** (all crates under [crates/rustius-ssh/](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-ssh/src), [crates/rustius-rdp/](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-rdp/src))
- **What this is:** `rustius-ssh` and `rustius-rdp` have zero unit or integration tests. The only tested crates are `rustius-core` (good coverage) and `rustius-terminal` (basic spawn test).
- **Problem:** The SSH auth flow, SFTP file listing logic, RDP exit diagnostic parsing, and FreeRDP executable discovery are all untested. The `describe_freerdp_exit` function (~300 lines of match logic) is the kind of code most likely to regress without tests.
- **Fix:** Priority test targets: (1) `describe_freerdp_exit` — pure function, trivially testable with known exit codes and log lines, (2) `find_freerdp_executable` — testable with temp directories, (3) SFTP `list_dir` sorting — can be unit tested with mock `FileEntry` data.
- **If we skip it:** Regressions in RDP error reporting or SSH auth will only be caught by manual testing.

### 14. **SFTP `download_bytes` loads entire remote file into memory without size limit** ([sftp.rs:L210-214](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-ssh/src/sftp.rs#L210-L214))
- **What this is:** `download_bytes` reads the entire remote file into a `Vec<u8>` for IPC transfer to the frontend.
- **Problem:** If a user accidentally triggers a preview on a 2GB log file, the app allocates 2GB of RAM and then tries to serialize it over Tauri IPC (which would also allocate another copy). The app will likely OOM-crash.
- **Fix:** Add a size check via `stat` before reading. If the file is larger than a reasonable limit (e.g. 50MB), return an error telling the user to download to disk instead. Apply the same limit to `sftp_upload_bytes`.
- **If we skip it:** Users accidentally previewing large files will crash the entire application.

---

## Nice to Have

### 15. **`RustiusError` enum is defined but never used outside `rustius-core`** ([error.rs:L1-27](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-core/src/error.rs#L1-L27))
- **What this is:** `RustiusError` defines variants for Config, ConnectionNotFound, FolderNotFound, IO, Serialization, and Storage errors.
- **Problem:** Almost all functions in `config.rs` return `anyhow::Result`, not `crate::error::Result`. The typed error is only used in `get_connection` (returning `ConnectionNotFound`). The Tauri command layer converts everything to `String` anyway.
- **Fix:** Either commit to using `RustiusError` throughout `config.rs` (replacing `anyhow::anyhow!()` calls with proper variants), or delete `error.rs` and use `anyhow` consistently. The current half-and-half approach gives you none of the benefits of either pattern.
- **If we skip it:** Slightly confusing code but no functional impact.

### 16. **`update_config` saves the entire config to disk on every mutation** ([config.rs:L599-608](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-core/src/config.rs#L599-L608))
- **What this is:** Every call to `update_config` acquires the write lock, runs the closure, then calls `save()` which serializes both `settings.json` and `connections.json` and writes them to disk.
- **Problem:** During `import_tree`, this means all connections are written to disk once at the end — that's fine. But each `add_connection`, `add_folder`, or `save_connection_note` call triggers a full re-serialize and write of the entire config. For a tree with hundreds of connections, each write re-serializes the whole thing.
- **Fix:** Consider a debounced save or a dirty flag + periodic flush (e.g. save at most once per second). This is a "nice to have" because at desktop scale, the writes are fast enough to not be noticed unless the user imports hundreds of connections one by one.
- **If we skip it:** Slightly more disk I/O than necessary; not noticeable for typical usage patterns.

### 17. **FreeRDP monitor loop uses polling instead of events** ([freerdp.rs:L437-438](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-rdp/src/freerdp.rs#L437-L438))
- **What this is:** A `tokio::spawn` loop polls every 200ms to discover the FreeRDP HWND and enforce window positioning.
- **Problem:** Polling at 200ms is fast enough for smooth docking, but it's inherently wasteful — the loop runs even when the window hasn't moved. On battery-powered laptops, hundreds of unnecessary Win32 API calls per minute drain power.
- **Fix:** Use `WinEventHook` (`SetWinEventHook` with `EVENT_OBJECT_LOCATIONCHANGE`) to react to window moves rather than polling. This is a non-trivial win32 change but eliminates the polling entirely.
- **If we skip it:** Negligible battery impact for most users. The current approach works correctly.

### 18. **`TreeNode` allows `clippy::large_enum_variant` without boxing** ([connection.rs:L346](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-core/src/connection.rs#L346))
- **What this is:** `TreeNode::Connection(Connection)` inlines the full ~300-byte `Connection` struct, while `TreeNode::Folder(Folder)` is much smaller.
- **Problem:** Every `TreeNode` in memory is as large as its largest variant. With hundreds of nodes, mostly folders, each folder node wastes ~200 bytes of padding. The `#[allow(clippy::large_enum_variant)]` suppresses the warning instead of addressing it.
- **Fix:** `Box<Connection>` inside the enum variant: `Connection(Box<Connection>)`. Reduces enum size to ~80 bytes. Only worth doing if the tree regularly exceeds 500+ nodes.
- **If we skip it:** A few KB of wasted memory. Negligible for desktop use.

---

## Verdict

**Healthy architecture, serious security gaps.** The crate separation, config management, credential vault, and IPC patterns are well-designed and production-grade. The critical gaps are all security-related: blind host key acceptance (#1), password on the RDP command line (#2), and no CSP (#3). Fix those three before shipping to real users.

**Lean: ~120 lines deletable** (unused `auth.rs` function, dead `lucide-svelte` dependency, `async-trait` dep), **~300 lines mergeable** (duplicated auth logic + duplicated secure_connection/secure_credential patterns).

**Not checked:** Frontend Svelte components beyond the store and types (14 `.svelte` files in `src/lib/components/`), the importer modules (`mremoteng.rs`, `mobaxterm.rs`), GitHub Actions CI configuration, and the `DESIGN.md` design system implementation fidelity.
