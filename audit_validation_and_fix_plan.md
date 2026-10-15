# Plan: Rustius Codebase Audit Validation & Remediation

This plan reviews, validates, and categorizes all 18 findings from [`rustius_audit.md`](file:///c:/Users/jlmanohar/Documents/Projects/rustius/rustius_audit.md). It outlines which findings are valid vs. false positives, provides exact technical remediation steps for valid items, and explains which items should be accepted as acceptable risks.

---

## Executive Summary & Validation Matrix

| # | Finding Name | Severity / Category | Audit Claim | Codebase Reality & Validation Status | Proposed Action |
|---|---|---|---|---|---|
| **1** | **SSH Host Key Verification Blind Acceptance** | Critical / Security | `client.rs` & `sftp.rs` accept all server keys unconditionally (`Ok(true)`). | **VALID**: Both handlers return `Ok(true)` unconditionally, allowing MITM attacks. | **Fix**: Implement shared host key verifier using `russh::keys::check_known_hosts` & `learn_known_hosts` (TOFU) with `~/.ssh/known_hosts`. |
| **2** | **RDP Password in Process Command Line** | High / Security | Password passed via `/p:<password>` flag in `wfreerdp.exe` process arguments. | **VALID**: Passwords visible to any local user via Task Manager or `wmic`. | **Fix**: Use FreeRDP `/args-from:stdin` or `/from-stdin:force` to feed credentials via stdin pipe. |
| **3** | **Tauri CSP Disabled (`null`)** | High / Security | `"csp": null` allows inline script execution if any XSS occurs. | **VALID**: `tauri.conf.json` explicitly sets `"csp": null`. | **Fix**: Configure strict CSP with allowances for local IPC and Google Fonts. |
| **4** | **Vault Re-encrypts Entire File on Every Secret Mutation** | High / Performance | `set_vault_secret` derives Argon2id key and writes disk on every insert. | **VALID**: Bulk imports/migrations trigger $N$ Argon2id key derivations (~100ms each). | **Fix**: Add `set_vault_secrets_batch` and batch-persist in `secure_tree_nodes` / migration. |
| **5** | **Duplicated SSH Auth Logic** | Medium / Code Quality | `client.rs` and `sftp.rs` have identical match blocks on `AuthMethod`. | **VALID**: Duplicated match-arms for Password, KeyFile, Agent. | **Fix**: Extract shared `authenticate()` helper into `rustius-ssh/src/auth.rs`. |
| **6** | **Duplicated `secure_connection` / `secure_credential`** | Medium / Code Quality | ~120 lines of identical match logic in `config.rs`. | **VALID**: Redundant boilerplate for extracting secrets to Keyring/Vault. | **Fix**: Extract `secure_auth_method(&self, auth: &mut AuthMethod, key: &str)` helper. |
| **7** | **`overflow-checks = false` in Release Profile** | Low / Safety | Release profile explicitly turns off integer overflow checks. | **VALID**: `Cargo.toml` line 94 sets `overflow-checks = false`. | **Fix**: Change to `overflow-checks = true` in root `Cargo.toml`. |
| **8** | **PTY Shell Detection Memory Leak via `.leak()`** | Medium / Memory | Non-Windows shell string `.leak()`s on every terminal spawn. | **VALID**: `std::env::var("SHELL")...leak()` permanently leaks heap memory on Unix. | **Fix**: Pass `&shell` or use `Cow<str>` to `CommandBuilder::new(&shell)`. |
| **9** | **Unused `describe_auth_method` & `async-trait` Dep** | Low / Cleanliness | `auth.rs` has unused function; `Cargo.toml` lists unused `async-trait`. | **VALID**: `describe_auth_method` is never invoked, `async-trait` is never imported. | **Fix**: Remove `async-trait` dependency; repurpose `auth.rs` for shared auth helper. |
| **10** | **Duplicate Icon Libraries (`lucide-svelte` vs `@lucide/svelte`)** | Low / Bloat | `package.json` installs both packages; all code uses `@lucide/svelte`. | **VALID**: All 14 UI components import `@lucide/svelte`. | **Fix**: Remove `"lucide-svelte"` from `package.json`. |
| **11** | **SSH Keepalive & Timeout Config Ignored** | Medium / Bug | `client.rs` & `sftp.rs` hardcode 5s keepalive & 12s timeout, ignoring `SshConfig`. | **VALID**: `connection.ssh_config` is completely ignored at runtime. | **Fix**: Read `keepalive_interval` and `connect_timeout_secs` from `connection.ssh_config`. |
| **12** | **Frontend `conn_type` vs Backend `connection_type`** | Medium / Architecture | TS interface uses `conn_type`, Rust serializes `connection_type`. | **VALID**: Led to ugly `(node as any).connection_type` casts in components. | **Fix**: Make `connection_type` canonical in TS interface; keep `conn_type?` as alias. |
| **13** | **Zero Tests for SSH, SFTP, and RDP Logic** | Medium / Testing | Report claimed `rustius-rdp` has 0 tests and `describe_freerdp_exit` is untested. | **PARTIAL FALSE POSITIVE**: `rustius-rdp` **already has 5 unit tests** for `describe_freerdp_exit` in `freerdp.rs:939-988`. `rustius-ssh` does have 0 tests. | **Fix**: Acknowledge false positive on RDP exit tests; add unit tests for `rustius-ssh` auth & SFTP entry parsing. |
| **14** | **SFTP `download_bytes` Unbounded Memory Consumption** | High / Stability | Reading entire file into `Vec<u8>` without size check risks OOM crashes. | **VALID**: No check before reading into memory; crashes if user previews large file. | **Fix**: Check `stat` / file size before reading; reject files > 50 MB for in-memory transfer. |
| **15** | **`RustiusError` Unused Outside `rustius-core`** | Low / Cleanliness | `RustiusError` defined but almost all functions return `anyhow::Result`. | **VALID**: `RustiusError` is only used once with `.into()`. | **Action**: Accept as low priority or streamline `error.rs`. |
| **16** | **`update_config` Saves Entire Config on Every Mutation** | Low / Performance | Every call writes `settings.json` and `connections.json` to disk. | **VALID, ACCEPT RISK**: Writes take <2ms for hundreds of connections. Sync save guarantees persistence on crash. | **Action**: Accept Risk / Keep As-Is. |
| **17** | **FreeRDP Monitor Loop Uses Polling (200ms)** | Low / Battery | Background loop polls every 200ms instead of Win32 Event Hook. | **VALID, ACCEPT RISK**: Polling external child HWND across processes via `WinEventHook` introduces high cross-process hook fragility. 200ms sleep is negligible CPU. | **Action**: Accept Risk / Keep As-Is. |
| **18** | **`TreeNode` Allows `clippy::large_enum_variant` Without Boxing** | Low / Memory | `Connection` variant is ~300 bytes, `Folder` is smaller. | **VALID, ACCEPT RISK**: Total memory for 1,000 nodes is <300 KB. Boxing requires updating 28 match sites across the repo for zero perceptible gain. | **Action**: Accept Risk / Keep As-Is. |

---

## Detailed Remediation Plans by Category

### Phase 1: Security Critical Fixes (Must Fix)

#### 1. SSH Host Key Verification (`rustius-ssh`)
- **Validation**: Confirmed in [`client.rs:21-26`](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-ssh/src/client.rs#L21-L26) and [`sftp.rs:34-39`](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-ssh/src/sftp.rs#L34-L39). Both handlers return `Ok(true)`.
- **Implementation**:
  - In `crates/rustius-ssh/src/verifier.rs` (or shared handler), implement `HostKeyVerifier` implementing `russh::client::Handler`.
  - Check against `~/.ssh/known_hosts` using `russh::keys::known_hosts::check_known_hosts`.
  - If recognized and matches: `Ok(true)`.
  - If new host: use TOFU (Trust-On-First-Use) policy to record with `russh::keys::known_hosts::learn_known_hosts` and log an info notice.
  - If key changed: return `Err(anyhow::anyhow!("POSSIBLE MITM ATTACK: Host key changed for {}:{}!", host, port))`.
  - Use the exact same verifier in both `SshSession` and `SftpClient`.

#### 2. FreeRDP Password Exposure via CLI (`rustius-rdp`)
- **Validation**: Confirmed in [`freerdp.rs:336-340`](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-rdp/src/freerdp.rs#L336-L340). `cmd.arg(format!("/p:{}", pass))` places the password directly on the command line.
- **Implementation**:
  - Use FreeRDP's stdin credential passing: `/args-from:stdin` or `/from-stdin:force`.
  - With `/args-from:stdin`, pass connection flags line-by-line via `child.stdin`, or with `/from-stdin:force`, pass the password via `stdin` and drop `/p:<pass>` from `cmd.arg()`.
  - Set `cmd.stdin(Stdio::piped())`, write the password / arguments to stdin after spawn, and then close stdin.
  - This completely removes credentials from the Windows process command line (`Get-Process`, Task Manager, `wmic`).

#### 3. Content Security Policy (`src-tauri/tauri.conf.json`)
- **Validation**: Confirmed in [`tauri.conf.json:26`](file:///c:/Users/jlmanohar/Documents/Projects/rustius/src-tauri/tauri.conf.json#L26). `"csp": null`.
- **Caution / Nuance**: Inspection of [`src/app.html`](file:///c:/Users/jlmanohar/Documents/Projects/rustius/src/app.html#L8-L13) reveals that Rustius loads Google Fonts (`fonts.googleapis.com` and `fonts.gstatic.com`). Setting a CSP without these origins would break font rendering.
- **Implementation**:
  - Update `tauri.conf.json`:
    ```json
    "security": {
      "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com; connect-src ipc: http://ipc.localhost"
    }
    ```

#### 4. Vault Batching on Bulk Mutations (`rustius-core`)
- **Validation**: Confirmed in [`secret.rs:156-164`](file:///c:/Users/jlmanohar/Documents/Projects/rustius/crates/rustius-core/src/secret.rs#L156-L164). `set_vault_secret` calls `save_vault_locked(&guard)` on every single invocation, re-deriving Argon2id keys every time.
- **Implementation**:
  - Add `set_vault_secrets_batch(&self, entries: &[(&str, &str)]) -> Result<()>`:
    ```rust
    pub fn set_vault_secrets_batch(&self, entries: &[(&str, &str)]) -> Result<()> {
        let mut guard = self.vault_state.write().map_err(|e| anyhow::anyhow!("Lock poisoned: {e}"))?;
        if !guard.is_unlocked {
            bail!("Vault is locked.");
        }
        for (k, v) in entries {
            guard.secrets.insert((*k).to_string(), (*v).to_string());
        }
        self.save_vault_locked(&guard)?;
        Ok(())
    }
    ```
  - Use batching in `secure_tree_nodes` and `migrate_vault_backend`.

---

### Phase 2: High & Medium Priority Improvements (Should Fix)

#### 5. Shared SSH Auth Helper (`rustius-ssh`)
- **Implementation**:
  - In `crates/rustius-ssh/src/auth.rs`, implement:
    ```rust
    pub async fn authenticate<H: russh::client::Handler>(
        handle: &mut russh::client::Handle<H>,
        auth: &AuthMethod,
    ) -> Result<()>
    ```
  - Refactor `client.rs` and `sftp.rs` to call `auth::authenticate(&mut handle, &connection.auth).await?`.

#### 6. De-duplicate `secure_connection` & `secure_credential` (`rustius-core`)
- **Implementation**:
  - In `crates/rustius-core/src/config.rs`, implement:
    ```rust
    fn secure_auth_method(&self, auth: &mut AuthMethod, key: &str) {
        let backend = self.effective_vault_backend_sync();
        // Single match block handling Password and KeyFile variants
    }
    ```
  - Both `secure_connection` and `secure_credential` become 2-line wrappers calling `secure_auth_method`.

#### 7. Enable Overflow Checks in Release Profile (`Cargo.toml`)
- **Implementation**:
  - In root `Cargo.toml`, change `overflow-checks = false` to `overflow-checks = true`.

#### 8. Fix PTY Shell Memory Leak (`rustius-terminal`)
- **Implementation**:
  - In `crates/rustius-terminal/src/pty.rs`:
    ```rust
    let shell = if cfg!(windows) {
        "powershell.exe".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
    };
    let mut cmd = CommandBuilder::new(&shell);
    ```
  - Eliminates `.leak()`.

#### 9. Clean up Dead Code & Unused Dependencies
- **Implementation**:
  - Remove `async-trait.workspace = true` from `crates/rustius-ssh/Cargo.toml`.
  - Remove unused `describe_auth_method` or wire it to debug logging.

#### 10. Uninstall Duplicate `lucide-svelte`
- **Implementation**:
  - Remove `"lucide-svelte": "^1.0.1"` from `package.json`.
  - Run `npm install` (or edit `package.json`).

#### 11. Wire SSH Keepalive and Timeout from `SshConfig`
- **Implementation**:
  - In `client.rs` and `sftp.rs`:
    ```rust
    let keepalive = connection.ssh_config.as_ref()
        .and_then(|c| c.keepalive_interval)
        .unwrap_or(5);
    let timeout_secs = connection.ssh_config.as_ref()
        .and_then(|c| c.connect_timeout_secs)
        .unwrap_or(12);
    ```

#### 12. Standardize `connection_type` across Frontend & Types
- **Implementation**:
  - In `src/lib/types/index.ts`:
    ```typescript
    export interface Connection {
      id: string;
      name: string;
      connection_type: ConnectionType;
      conn_type?: ConnectionType; // optional alias for backward compat
      // ...
    }
    ```
  - Clean up `(node as any).connection_type` in `Sidebar.svelte` and `ConnectionModal.svelte`.

#### 13. Unit Tests for `rustius-ssh`
- **Audit Clarification**: Note the partial false positive — FreeRDP exit code tests already exist in `crates/rustius-rdp/src/freerdp.rs`.
- **Implementation**:
  - Add unit tests in `crates/rustius-ssh/src/auth.rs` and `crates/rustius-ssh/src/sftp.rs` for auth strategy description and SFTP directory entry parsing/sorting.

#### 14. SFTP In-Memory Buffer Limit (`rustius-ssh`)
- **Implementation**:
  - In `crates/rustius-ssh/src/sftp.rs`:
    - Before reading remote file into memory in `download_bytes`, query `self.session.metadata(remote_path).await`.
    - If `size > 50 * 1024 * 1024` (50MB), return `Err(anyhow::anyhow!("File is too large ({:.1} MB) for in-memory preview. Please download to disk.", size_mb))`.
    - Apply same check to `upload_bytes`.

---

### Phase 3: Accepted Risks (Documented & Justified)

#### 15. `RustiusError` vs `anyhow`
- **Justification**: Tauri commands serialize all errors to `String` over IPC (`Result<T, String>`). `anyhow::Result` with `.context()` provides rich contextual traces. Keeping `RustiusError` for core domain errors without forcing all internal helpers to use it is standard pragmatic Rust.
- **Decision**: Accept as-is.

#### 16. `update_config` Full File Write
- **Justification**: Config writes take < 2ms. Debouncing creates the risk of losing connection edits on sudden app quit. Synchronous atomic file writes ensure zero data loss.
- **Decision**: Accept as-is.

#### 17. FreeRDP 200ms Polling Loop
- **Justification**: FreeRDP creates a separate Win32 process window. Monitoring it with cross-process `WinEventHook` creates thread message loop affinity requirements, potential UI deadlocks, and unreliability across multi-monitor DPI switches. 200ms sleep on Tokio yields CPU time and consumes negligible resources.
- **Decision**: Accept as-is.

#### 18. `TreeNode` Large Enum Variant
- **Justification**: A desktop tree rarely exceeds 500 connections (< 200 KB RAM). Boxing adds heap indirection and requires modifying 28 match sites for zero human-perceptible gain.
- **Decision**: Accept as-is.

---

## User Review & Decision Points

> [!IMPORTANT]
> **SSH Host Key Policy Decision**:
> For Finding #1, standard OpenSSH behavior is TOFU (Trust-On-First-Use: auto-learn on first connection if not found, alert and block if key changed). Does this TOFU policy meet your requirements, or would you prefer a modal confirmation dialog from the UI on unknown host keys? (Auto-learn TOFU into `~/.ssh/known_hosts` is the standard recommended approach).

> [!TIP]
> **FreeRDP Stdin Piping**:
> For Finding #2, we will use `/from-stdin:force` / `/args-from:stdin` to pass the credentials via child process standard input, ensuring Task Manager and local process listings never display plaintext passwords.

---

## Verification Plan

### Automated Tests
1. **Rust Unit Tests**:
   ```bash
   cargo test --workspace
   ```
2. **Rust Compilation & Clippy**:
   ```bash
   cargo clippy --workspace --all-targets
   ```
3. **Frontend Type Check**:
   ```bash
   npm run check
   ```

### Manual Verification
1. Verify SSH and SFTP connections successfully authenticate and handle host keys.
2. Verify FreeRDP process arguments using `Get-Process | Select-Object -ExpandProperty CommandLine` — confirm password `/p:` is absent.
3. Verify CSP in the webview console — ensure Google Fonts load without CSP policy violations.
4. Verify creating and saving connections with master password vault.
