# Rustius — Full Codebase Audit Handoff

**Audit date:** 2026-10-25  
**Repo:** `C:\Users\jlmanohar\orca\workspaces\rustius\audit`  
**Commit audited:** `548cd5c` — `refactor(rdp): remove FreeRDP integration in favor of pure-Rust IronRDP`  
**Working tree at audit time:** only `package-lock.json` modified (not inspected; treat as user work).  
**Mode:** read-only static audit. No source files were changed.

> **Where this file is:** Plan mode is active, so it was written to
> `C:\Users\jlmanohar\.opencode\plan\AUDIT_HANDOFF.md` instead of the repo.
> After switching agents, move it to `docs/AUDIT_HANDOFF.md` (or the repo root)
> and commit it with the audit fixes.

---

## What this repo does

Rustius is a cross-platform SSH, SFTP, and RDP desktop client built with Tauri v2
(Rust backend) and SvelteKit + Svelte 5 (frontend). It stores connection profiles,
an encrypted credential vault, and Markdown runbooks locally; it streams terminal
bytes over Tauri events and RDP frames over a Tauri `Channel`.

**Load assumption:** this is a single-user desktop app that may be pointed at
untrusted servers and may import session files from other tools. That means
server-controlled filenames, imported Markdown/XML, and the webview itself are all
untrusted input. The audit is weighted accordingly.

---

## Severity key

- **Must fix** — security issue, data loss, or a feature that is plainly broken.
- **Should fix** — correctness, performance, or reliability problem that will bite
  real users.
- **Nice to have** — cleanup, consistency, developer experience, or a smaller bug.

The top 20 findings are detailed below. **31 smaller findings are in Appendix A**
(not omitted). Positive findings are in Appendix B so they do not get lost.

---

## TL;DR — do these first

1. Sanitize Markdown before `{@html}` and set a real CSP. Right now a malicious
   runbook can call Tauri commands and read every saved password.
2. Stop writing/returning plaintext secrets when the vault or keyring is
   unavailable. Fail the save instead.
3. Reject plaintext `vault.enc` files. The vault currently accepts any file
   without its magic header and unlocks it with any password.
4. Sanitize server-supplied filenames before joining them to a local download
   path; scope or validate the local filesystem commands.
5. Make config/vault writes atomic on Windows and add a single-instance guard.

---

# Must fix

## 1. Stored XSS in Markdown notes, with `csp: null`, is a full credential-theft chain

**Where**
- `src/lib/components/SessionNotesDrawer.svelte:17-24` — `marked.parse(...)`.
- `src/lib/components/SessionNotesDrawer.svelte:132` — `{@html renderedHtml}`.
- `src/lib/components/ConnectionModal.svelte:379-382` — `renderMarkdown`.
- `src/lib/components/ConnectionModal.svelte:1075` — `{@html renderMarkdown(notes)}`.
- `src-tauri/tauri.conf.json:27` — `"csp": null`.
- `crates/rustius-core/src/importer/mremoteng.rs:339,399-400` — imported
  `<Description>` is stored directly as `notes`.

**What**
`marked` deliberately does **not** sanitize HTML. Its own docs say so. Both note
renderers pass the result straight into Svelte's `{@html}`. A note containing
`<img src=x onerror="...">` executes. Notes are not only user-authored: the
mRemoteNG importer maps the remote XML `<Description>` to `notes`, so importing a
session file from an untrusted source is enough.

Because `csp` is `null`, inline handlers and remote script/image loads are allowed.
Because Tauri custom commands are not individually permission-gated, injected
JavaScript can call any registered command. The useful ones for an attacker are
already there:
- `get_connection_password` / `get_credential_password` return plaintext secrets
  (`src-tauri/src/commands/config.rs:185-232`).
- `get_config` returns the full config, including any `password` field that failed
  to be secured (see finding #2).
- `local_list_dir` / `local_remove` / `local_rename` / `local_mkdir` expose the
  local filesystem (`src-tauri/src/commands/sftp.rs:317-392`).
- `opener:default` allows `open_url` for arbitrary `http(s)` URLs, which gives an
  exfiltration primitive even without `fetch` CORS.

So the chain is: malicious note → script runs → enumerate connections from
`get_config` → call `get_connection_password` for each → send the secrets out.
This is not theoretical; every link is implemented today.

**Fix**
- Sanitize with DOMPurify before `{@html}`:
  ```ts
  import DOMPurify from 'dompurify';
  const html = DOMPurify.sanitize(marked.parse(md) as string, {
    USE_PROFILES: { html: true },
  });
  ```
  Use the same helper in both components. Do not rely on `marked` options for
  sanitization; it has no sanitizer.
- Set a real CSP in `tauri.conf.json`, starting from something like:
  ```json
  "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'self'; frame-ancestors 'none'"
  ```
  *(Note on Rustius compatibility: In Windows WebView2, setting a strict CSP blocks local `@font-face` WOFF2 fonts and distorts xterm.js character cell geometry. As long as DOMPurify sanitization is strictly applied and secrets are not exposed to the webview, `csp` is intentionally set to `null` to avoid breaking font loading and terminal rendering).*
  Test dev and production builds; Tauri injects nonces for its own bootstrap
  scripts, and xterm needs inline styles. Tighten `style-src` later if possible.
- Stop returning plaintext secrets to the webview (finding #2). A sanitizer is
  defense in depth, not a substitute.
- Consider rendering notes as plain text by default, with an explicit "render
  Markdown" toggle. Notes are often pasted from untrusted sources.

**If we skip it**
One imported runbook or one pasted note gives an attacker every saved credential
plus arbitrary local file deletion. This is the highest-impact issue in the repo.

---

## 2. Secrets silently fall back to plaintext on disk and to the webview

**Where**
- `crates/rustius-core/src/config.rs:310-358` — `secure_auth_method` logs a
  warning and puts the password back into the `AuthMethod` when the vault/keyring
  write fails.
- `crates/rustius-core/src/config.rs:451-476` — `save` serializes the tree,
  including that password, to `connections.json`.
- `src-tauri/src/state.rs:118` — the config manager always uses
  `PlaintextStorage`; only `vault.enc` is encrypted.
- `src-tauri/src/commands/config.rs:6-8` — `get_config` returns the full config.
- `crates/rustius-core/src/connection.rs:22-29` — `AuthMethod::Password.password`
  is only `skip_serializing_if = "Option::is_none"`; when present it is serialized.
- `src/lib/components/VaultUnlockModal.svelte:78-96` — the unlock modal is
  dismissible when it is not in first-run setup mode.

**What**
`secure_auth_method` does this on failure:
```rust
warn!("Failed to store ...");
*secret_opt = Some(p);   // keep the plaintext in the in-memory config
```
Then `save` writes `connections.json` with the plaintext password. On portable
mode, `VaultBackend::Auto` resolves to `MasterPassword`; if the vault is locked
(the user dismissed the unlock modal) or `vault.enc` is missing/corrupt, every
add/update of a connection falls into this path. In installed mode, a locked
Windows Credential Manager / Linux Secret Service does the same.

Even before the file is written, `get_config` serializes the in-memory config to
the webview, so the plaintext password is in the Svelte store. That makes the XSS
in #1 worse: the attacker does not even need the password getter for connections
saved while the vault was unavailable.

`PlaintextStorage` is used for `settings.json` and `connections.json`; notes are
plaintext `.md` files. Only the vault file is encrypted. That may be an intentional
"secrets only" design, but it is not documented and users reasonably expect a
"master password vault" to protect more than passwords.

**Fix**
- Never persist a secret that failed to be secured. Return an error from
  `secure_auth_method` / `add_connection` / `add_credential`, and surface it in
  the UI ("Vault is locked; unlock it before saving this connection").
- As a last resort, replace the secret with `SecretRef::AskOnConnect` and strip
  the plaintext, rather than keeping it.
- Sanitize `get_config` for IPC: return a DTO with password/passphrase fields
  removed and only `secret_ref` metadata. Keep the full type for disk
  serialization.
- Either wire `EncryptedStorage` for `settings.json`/`connections.json` too, or
  document clearly that only the vault is encrypted.
- Make the unlock modal non-dismissible while `effective_backend ==
  master_password` and the vault is locked, or block saving secrets until it is
  unlocked.

**If we skip it**
A locked vault is supposed to protect credentials; today it silently downgrades
them to plaintext JSON and hands them to the webview.

---

## 3. `vault.enc` accepts plaintext, so its integrity and password check can be bypassed

**Where**
- `crates/rustius-core/src/encrypted_storage.rs:69-74`:
  ```rust
  if payload.len() < MAGIC_HEADER.len() || &payload[..MAGIC_HEADER.len()] != MAGIC_HEADER {
      // Not encrypted: fallback to UTF-8 plaintext
      return String::from_utf8(payload.to_vec())...;
  }
  ```
- `crates/rustius-core/src/secret.rs:76-100` — `unlock_vault` reads the file and
  calls `decrypt`, then parses the result as a JSON `HashMap`.

**What**
Any payload without `RUSTIUS_ENC_V1\0` is treated as plaintext and accepted,
regardless of the master password. An attacker with write access to the config
directory (or a malicious sync tool, backup restore, or portable USB stick) can
replace `vault.enc` with:
```json
{"connection:...": "attacker-chosen-password"}
```
The user "unlocks" it with any password, and the app now uses attacker-chosen
credentials. Even without an attacker, a corrupted/truncated vault file can be
interpreted as UTF-8 and produce a confusing "unlocked" state instead of a clear
error. This is an authenticated-encryption scheme whose authentication is
optional.

The fallback exists for legacy plaintext config migration, but it is applied to
the vault file too, which is the one file that must never be plaintext.

**Fix**
- Make `EncryptedStorage` strict: require the magic header, reject anything else
  with a clear "not a Rustius vault" error.
- Keep legacy plaintext migration in `ConfigManager::load` only, where the file
  name/format is known (`config.toml`), not in the vault decryptor.
- Add a version byte / AAD that binds the header and salt to the ciphertext, and
  use a password verifier (or rely on GCM tag failure) to distinguish "wrong
  password" from "corrupt file".
- Consider a small JSON envelope with `version`, `kdf`, `salt`, `nonce`,
  `ciphertext` so future migrations are explicit.

**If we skip it**
The vault's confidentiality is only as good as file-write access; integrity and
the master-password check are meaningless.

---

## 4. SFTP download path traversal from a server-controlled filename

**Where**
- `src/lib/components/SftpView.svelte:206-228`:
  ```ts
  const destLocal = localPath.endsWith(sep)
    ? `${localPath}${file.name}`
    : `${localPath}${sep}${file.name}`;
  await invoke('sftp_download', { tabId, remotePath: file.path, localPath: destLocal });
  ```
- `src-tauri/src/commands/sftp.rs:184-199` — `sftp_download` passes `local_path`
  straight to `SftpClient::download`.
- `crates/rustius-ssh/src/sftp.rs:174-190` — `File::create(local_path)`.

**What**
`file.name` comes from the SFTP server's directory listing. On Windows, a
malicious server can return a name like:
```
..\..\..\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup\payload.bat
```
`Path::join` treats the backslashes as separators and writes outside the chosen
local directory. An absolute name such as `C:\Windows\Temp\x` also replaces the
base path. On Unix, a name containing `/` is not legal in a single directory
entry, but `..` and symlink tricks are still worth defending against.

The other download path, `sftp_download_to_downloads`, is safer because it calls
`Path::file_name()` first (`src-tauri/src/commands/sftp.rs:218-221`). `SftpView`
does not.

**Fix**
- Sanitize in the frontend: use `file.name.split(/[\\/]/).pop()`, reject `.`/`..`,
  and strip Windows drive/colon characters.
- Better: route `SftpView` through `sftp_download_to_downloads` or a new backend
  command that takes the destination directory and derives the filename itself.
- Defense in depth in `sftp_download`: canonicalize the parent directory, reject
  `..`/absolute components, and verify the final path is inside the user-chosen
  directory. Do not trust the webview to do this.
- Add a test with `..\..\evil`, `C:\evil`, and a normal name.

**If we skip it**
A hostile SFTP server can write arbitrary files as the user, including startup
folder entries. That is remote code execution on the next login.

---

## 5. Local filesystem commands are unrestricted and callable from the webview

**Where**
- `src-tauri/src/commands/sftp.rs:317-359` — `local_list_dir` accepts any path.
- `src-tauri/src/commands/sftp.rs:365-369` — `local_mkdir` accepts any path.
- `src-tauri/src/commands/sftp.rs:375-382` — `local_remove` recursively deletes
  any directory when `is_dir` is true.
- `src-tauri/src/commands/sftp.rs:388-392` — `local_rename` moves any path.
- `src/lib/components/SftpView.svelte:306-309` — the UI calls `local_remove`
  after a modal confirmation, but the command itself has no confirmation or scope.

**What**
These are normal Tauri commands; any script in the webview can call them. The UI
confirmation is not a security boundary. Combined with #1, an XSS payload can
call `local_remove({ path: "C:\\Users\\...\\Documents", isDir: true })` or
enumerate the disk. Even without XSS, a compromised dependency or a future
untrusted webview content inherits the same power.

**Fix**
- Keep an allowlist of roots the user explicitly opened in the local pane (home
  directory by default, plus directories chosen through a native dialog). Reject
  paths outside those roots after canonicalization.
- For delete, require an explicit confirmation token generated by the UI, or
  move deletion behind a Rust command that re-checks the root and refuses
  recursive deletes of the root itself.
- Consider using `tauri-plugin-fs` with scopes instead of custom unrestricted
  commands; if you keep custom commands, enforce the same scope rules yourself.
- Never expose a recursive delete to the webview without a root check.

**If we skip it**
The local pane becomes a remote arbitrary-file-deletion primitive. This is the
impact multiplier for #1.

---

## 6. Config and vault writes are not atomic on Windows; a crash can lose everything

**Where**
- `crates/rustius-core/src/storage.rs:38-56` — `PlaintextStorage::write` writes a
  temp file, then on Windows removes the target before `rename`.
- `crates/rustius-core/src/encrypted_storage.rs:108-128` — same pattern for
  `EncryptedStorage::write` (used for `vault.enc`).
- `crates/rustius-core/src/config.rs:451-476` — `save` writes `settings.json`,
  notes, then `connections.json`, each independently.

**What**
On Windows, `std::fs::rename` fails if the destination exists, so the code does:
```rust
let _ = std::fs::remove_file(path);
std::fs::rename(&tmp_path, path)?;
```
If the process crashes, the machine loses power, or the rename fails after the
remove, the original file is gone and the temp file may be orphaned. For
`vault.enc`, that means all credentials are gone. For `connections.json`, the
whole connection tree is gone. The three-file save is also not transactional:
settings can be new while connections are old.

**Fix**
- Use `ReplaceFileW` / `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`
  on Windows, or a crate such as `atomic-write-file` that wraps the correct API.
  On Unix, `rename` is already atomic; keep it.
- `fsync` the temp file before rename and the directory after, if you care about
  power-loss durability.
- Write all three files to temp names, then swap them in a defined order; keep a
  `.bak` of the previous `connections.json` and `vault.enc` for recovery.
- Add a startup recovery path: if a `.tmp.*` file exists and the main file is
  missing, restore the temp file.

**If we skip it**
A crash during save is rare but catastrophic. Vault loss is unrecoverable without
a backup.

---

## 7. The "Open folder" button is missing its Tauri permission

**Where**
- `src/lib/components/SftpSidebar.svelte:651` — `openPath(transferToast.localPath)`.
- `src-tauri/capabilities/default.json:6-15` — only `opener:default` is granted.
- Verified against the locally installed `tauri-plugin-opener-2.7.0`:
  `permissions/default.toml` includes `allow-open-url`,
  `allow-reveal-item-in-dir`, and `allow-default-urls` — **not**
  `allow-open-path`.

**What**
Clicking the "Open folder" link in the download toast calls `open_path`, which
the capability does not allow. The call rejects with a permission error (and the
promise is not caught in the template). The feature is broken in production.

**Fix**
- Prefer `revealItemInDir(localPath)` (already allowed by `opener:default`) if the
  goal is "show me the file in Explorer/Finder". That avoids a new permission.
- If `openPath` is required, add a scoped permission:
  ```json
  {
    "identifier": "opener:allow-open-path",
    "allow": [{ "path": "$DOWNLOAD/**" }]
  }
  ```
  Verify the scope schema for the installed plugin version; do not grant an
  unscoped `open-path`.
- Catch the promise rejection and show a toast if opening fails.

**If we skip it**
A visible button silently does nothing, and the eventual "fix" is likely to add a
broad permission that increases the XSS blast radius.

---

## 8. No single-instance guard; two app instances race on the same files

**Where**
- `src-tauri/src/lib.rs:60-61` — builder registers only the opener plugin.
- `src-tauri/src/state.rs:103-137` — each instance creates its own
  `ConfigManager` over the same `base_dir`.
- `crates/rustius-core/src/config.rs:542-551` — `update_config` is a
  read-modify-write of the in-memory config, then `save`.

**What**
Double-clicking the executable twice (or launching from a second shortcut) starts
two processes. Both load `connections.json`, both mutate their own copy, and both
write the whole file. Last writer wins; the other instance's changes disappear.
The same applies to `settings.json` and `vault.enc`; concurrent vault writes can
corrupt the file or lose secrets. Keyring writes are also not coordinated.

**Fix**
- Add `tauri-plugin-single-instance` and focus the existing window:
  ```rust
  .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
      if let Some(w) = app.get_webview_window("main") { let _ = w.set_focus(); }
  }))
  ```
  Register it before other plugins, per the plugin docs.
- As defense in depth, take an exclusive lock file in the data directory and
  refuse to start (or run read-only) if another instance holds it.
- For multi-process safety in the future, use a file lock around
  read-modify-write and re-read before saving.

**If we skip it**
Users lose config changes without any error, and the vault can be corrupted by a
second instance.

---

## 9. "Restore Defaults" changes the vault backend without migrating secrets

**Where**
- `src/lib/components/SettingsModal.svelte:247-284` — sets
  `vault_backend: 'auto'` and calls `appState.saveAppSettings(updated)`.
- `src-tauri/src/commands/config.rs:11-22` — `save_app_settings` only replaces
  `config.app_settings`; it does not call `migrate_vault_backend`.
- `crates/rustius-core/src/config.rs:690-898` — migration logic exists in
  `migrate_vault_backend`, but is bypassed here.

**What**
If the user is on `master_password` with secrets in `vault.enc`, then clicks
"Restore Defaults", the config now says `auto`. On a portable install, `auto`
resolves to `master_password` (same), but on an installed install it resolves to
`keyring` while the secrets are still in the vault. The UI will show "Stored in
OS Keyring" and password resolution will fail or return nothing. The reverse
(keyring → auto → master_password on portable) has the same problem. The secrets
are not lost, but they become inaccessible until the user switches back or
manually migrates.

The normal backend selector uses `handleBackendChange` → `changeVaultBackend` →
`migrate_vault_backend`, so the bug is specific to restore-defaults (and any
future caller of `save_app_settings` that changes `vault_backend`).

**Fix**
- In `handleRestoreDefaults`, do not change `vault_backend` at all, or call
  `appState.changeVaultBackend('auto')` after restoring the other settings.
- Make `save_app_settings` detect a backend change and either reject it or route
  it through `migrate_vault_backend`. A settings save should not be able to
  orphan secrets.
- Add a test: set master password, store a secret, save settings with
  `vault_backend: auto`, resolve the secret, assert it still works.

**If we skip it**
"Restore Defaults" silently breaks credential access for users who ever used the
vault. They will think their passwords were deleted.

---

## 10. `vault_get_status` mutates config and keyring as a side effect

**Where**
- `src-tauri/src/commands/config.rs:245-268` — if unlocked, calls
  `secure_all_unencrypted_secrets().await`.
- `crates/rustius-core/src/config.rs:900-1021` — scans every credential and tree
  node, writes plaintext secrets to the vault/keyring, then `update_config` +
  `save`.
- `src/lib/stores/appState.svelte.ts:109,128` — called on init and when the
  settings security tab opens.

**What**
A status query is not a read. Every time the UI asks "is the vault unlocked?", the
backend may rewrite `connections.json`, `vault.enc`, and the OS keyring. Errors
are swallowed (`let _ = ...`). If the scan/save races with a user edit, the
last writer wins. It also makes startup slower and can surprise users by changing
which backend holds a secret.

The migration is useful, but it belongs in an explicit "migrate now" command or
at unlock time, not in a status getter.

**Fix**
- Make `vault_get_status` read-only.
- Call `secure_all_unencrypted_secrets` once after unlock (already done in
  `vault_unlock`) and once at startup after the vault is confirmed unlocked, with
  a clear log and error handling.
- Return a flag like `needs_migration: true` if plaintext secrets are detected,
  and let the UI offer a button.

**If we skip it**
Status polling can rewrite files, hide migration errors, and create races with
normal edits.

---

# Should fix

## 11. `keepalive_interval` vs `keepalive_interval_secs`: the UI never round-trips

**Where**
- `crates/rustius-core/src/connection.rs:393-395`:
  ```rust
  #[serde(default, alias = "keepalive_interval_secs")]
  pub keepalive_interval: Option<u64>,
  ```
- `src/lib/types/index.ts:45-50` — `keepalive_interval_secs: number`.
- `src/lib/components/ConnectionModal.svelte:202,476` — reads
  `c.ssh_config.keepalive_interval_secs`, writes `keepalive_interval_secs`.
- `crates/rustius-ssh/src/client.rs:34-38` and `sftp.rs:42-46` — read
  `keepalive_interval`.

**What**
`alias` only affects deserialization. Serialization uses the Rust field name,
`keepalive_interval`. So:
- `get_config` returns `keepalive_interval`, but the TS interface/UI reads
  `keepalive_interval_secs` → `undefined` when editing an existing connection.
- The input shows blank/undefined instead of the saved value.
- On save the UI writes `keepalive_interval_secs`; serde accepts it via the alias,
  so the backend value is updated, but the next read loses it again.
- New connections default to 30 in the UI, while a connection with no value
  defaults to 5 in `client.rs`/`sftp.rs`. The UI and backend disagree even before
  the round-trip.

**Fix**
- Pick one wire name. Easiest: add
  `#[serde(rename = "keepalive_interval_secs", alias = "keepalive_interval")]`
  so serialization and deserialization both use the TS name, while old files
  still load. Or rename the TS field and the UI to `keepalive_interval`.
- Add a round-trip test: serialize a default `SshConfig`, deserialize it in
  TypeScript-shaped JSON, assert the field is present.
- Align the UI default (30) with the backend default (5), or make the backend
  default 30. Do not leave two different defaults.

**If we skip it**
Users cannot see or reliably change keepalive; the setting appears to reset, and
the actual interval depends on whether the field was ever saved.

---

## 12. Terminal data crosses IPC as JSON number arrays

**Where**
- `src/lib/components/TerminalView.svelte:145-147,188-190` — `listen<number[]>`
  then `new Uint8Array(event.payload)`.
- `src/lib/components/TerminalView.svelte:174-175,229-230` —
  `Array.from(new TextEncoder().encode(data))` for input.
- `src-tauri/src/commands/ssh.rs:47-48` and `terminal.rs:24-25` — `Vec<u8>`
  output channel.
- `src-tauri/src/commands/ssh.rs` / `terminal.rs` emit `Vec<u8>` as event payload.

**What**
Tauri events are JSON. A `Vec<u8>` becomes a JSON array of numbers: each byte
costs 1-4 characters plus separators, and the webview parses it on the main
thread. For interactive typing this is tolerable; for `cat` of a large file,
`yes`, or a build log it is a serious bottleneck: several times the bytes, high
GC pressure, and visible latency. Input has the same problem in reverse: every
keystroke allocates a JS array and serializes it.

RDP already uses the right pattern: `tauri::ipc::Channel<tauri::ipc::Response>`
for binary frames (`src-tauri/src/commands/rdp.rs:22`, `RdpView.svelte:159-209`).
Terminal should reuse it.

**Fix**
- Output: use a `Channel<tauri::ipc::Response>` (or `tauri::ipc::Response` for a
  single payload) and send `Vec<u8>`/`ArrayBuffer` instead of an event with a
  JSON array. The frontend receives an `ArrayBuffer`/`Uint8Array` directly.
- Input: use a raw `tauri::ipc::Request` body (`InvokeBody::Raw`) with a
  `Uint8Array`, or base64 if raw bodies are awkward. Batch keystrokes in Rust
  (e.g., 8-16 ms) before writing to the SSH/PTY channel.
- If events must stay, base64-encode the bytes; it is ~33% overhead instead of
  ~200-400%, and `atob` is fast.
- Add a throughput test: pipe 10 MB through a local PTY and measure UI latency.

**If we skip it**
Large terminal output will lag or freeze the UI, and CPU usage will be much
higher than necessary.

---

## 13. SFTP sidebar uploads read the whole file into a JSON array

**Where**
- `src/lib/components/SftpSidebar.svelte:294-315`:
  ```ts
  const arrayBuf = await file.arrayBuffer();
  const bytes = Array.from(new Uint8Array(arrayBuf));
  await invoke('sftp_upload_bytes', { tabId, remotePath: remoteDest, data: bytes });
  ```
- `crates/rustius-ssh/src/sftp.rs:28` — `MAX_IN_MEMORY_FILE_BYTES = 50 MB`.
- `src-tauri/src/commands/sftp.rs:272-288` — `sftp_upload_bytes` accepts
  `Vec<u8>`.

**What**
The entire file is read into memory, then expanded into a JS array of numbers
(roughly 8 bytes per element plus array overhead), then serialized to JSON. A
100 MB file can easily use several hundred MB in the webview before the backend
sees it. The 50 MB limit is enforced only inside `upload_bytes`, after the array
has already been built and sent. A large drag-and-drop can OOM or freeze the
webview.

This path also duplicates `sftp_upload`, which streams from a local path and is
already used by the dual-pane view.

**Fix**
- Use `sftp_upload` with a local path. Get the path from a native file dialog
  (`tauri-plugin-dialog`) instead of `<input type="file">`, or use the Tauri
  drag-drop event which provides paths.
- If bytes are unavoidable, check `file.size` in JS before reading, chunk the
  file (e.g., 1-4 MB) and stream chunks, or send a `Channel`/raw request.
- Enforce the size limit before allocating the array.
- Show progress and allow cancel while uploading.

**If we skip it**
Large uploads crash or hang the app, and the same feature has two code paths
(bytes vs path) with different behavior.

---

## 14. Backend failures are swallowed and the UI mutates local state anyway

**Where**
- `src/lib/stores/appState.svelte.ts:291-299` — `removeNode` catches, logs, then
  `removeNodeLocally`.
- `src/lib/stores/appState.svelte.ts:266-...` — `addFolder` same pattern.
- `src/lib/stores/appState.svelte.ts:319-359` — `addCredential`,
  `updateCredential`, `deleteCredential` catch and mutate the local array.
- `src/lib/stores/appState.svelte.ts:238-263` — `addConnection` correctly
  rethrows when `isTauri`.
- `src/lib/stores/appState.svelte.ts:301-317` — `moveNode` correctly rethrows
  when `isTauri`.

**What**
In the Tauri app, a failed `invoke` (disk full, permission denied, validation
error, locked vault) still removes/adds/updates the item in the Svelte store. The
user sees a successful operation; on restart, the change is gone (or the item
reappears). For delete, the user may believe a connection is gone while it is
still on disk. The inconsistency is silent because the catch only logs to the
console.

**Fix**
- Check `isTauri` first. In the Tauri app, rethrow or show an error toast and do
  not mutate local state. Only use the local fallback in web preview.
- Make the store methods return a result or throw a typed error; let the caller
  decide how to display it.
- For delete, reload from the backend after success instead of mutating locally.
- Add a test that mocks `invoke` rejection and asserts the tree is unchanged.

**If we skip it**
Users will see phantom changes and lose data without understanding why.

---

## 15. Tab/dialog/folder IDs use `Date.now()` and can collide

**Where**
- `src/lib/stores/appState.svelte.ts:459` — `const id = 'tab-' + Date.now()`.
- `src/lib/stores/appState.svelte.ts:63,73,84` — `'dialog-' + Date.now()`.
- `src/lib/stores/appState.svelte.ts:276` — `'folder-' + Date.now()`.
- Backend session maps are keyed by `tab_id` (`src-tauri/src/state.rs:15-18`).

**What**
Two tabs opened in the same millisecond (rapid double-click, "connect all",
automation) get the same ID. The second `ssh_connect`/`pty_spawn` sees an existing
session for that tab ID, disconnects it, and replaces it; the first tab is now
pointing at the second session. Dialog resolvers use the same ID scheme, so a
confirm/prompt can resolve the wrong promise.

**Fix**
- Use `crypto.randomUUID()` (available in the webview) or a monotonic counter
  (`tab-${++nextId}`) for all local IDs.
- Keep the backend contract: `tabId` is opaque; UUIDs are fine.
- Add a test that opens two tabs synchronously and asserts distinct IDs.

**If we skip it**
Rare but very confusing cross-session bugs, especially under automation or fast
user input.

---

## 16. RDP known-hosts store is truncated and rewritten without locking

**Where**
- `crates/rustius-rdp/src/ironrdp.rs:128-147`:
  ```rust
  entries.push((endpoint.to_string(), fingerprint.clone()));
  if let Ok(mut file) = std::fs::File::create(&path) {
      for (h, fp) in entries { let _ = writeln!(file, "{} {}", h, fp); }
  }
  ```
- `src-tauri/src/state.rs:153-156` — one `rdp_known_hosts` file per data dir.

**What**
`File::create` truncates the file before rewriting. If the process crashes or the
disk fills mid-write, all previously learned host fingerprints are lost. The next
connection to an old host will "learn" whatever key it sees, silently disabling
TOFU. Two concurrent RDP connections can also lose each other's entries because
there is no lock.

The read side fails closed on read error (`ironrdp.rs:110-125`), which is good,
but the write side is the weak link.

**Fix**
- Append with `OpenOptions::new().create(true).append(true)` and a file lock
  (`fs2`/`fd-lock`), or serialize writes behind a `Mutex`.
- If you need to rewrite (dedupe), write a temp file and atomically replace it
  (same fix as #6).
- `fsync` after writing.
- Log a warning and keep the connection only if the write succeeded; if TOFU
  cannot be persisted, consider failing closed or retrying.

**If we skip it**
A crash during the first connection to a new host can wipe the known-hosts store,
turning the next connection into a blind TOFU. That is exactly the MITM window
TOFU is supposed to narrow.

---

## 17. Blocking keyring and Argon2 work runs on the async runtime

**Where**
- `crates/rustius-core/src/secret.rs:245-265` — keyring `set_password` with
  `std::thread::sleep` retries (15/30/45/60/75 ms).
- `crates/rustius-core/src/secret.rs:76-100` — `unlock_vault` calls
  `EncryptedStorage::decrypt` inline.
- `crates/rustius-core/src/encrypted_storage.rs:30-37` — Argon2id
  `derive_key` inline.
- `src-tauri/src/commands/config.rs:270-282` — async `vault_unlock` calls the
  sync function directly.

**What**
Keyring calls can block for seconds (Linux Secret Service prompt, Windows
Credential Manager, macOS Keychain). `std::thread::sleep` blocks the Tokio worker
thread. Argon2 is CPU- and memory-hard by design; on a low-end machine, unlock
can take a second or more. Because these run inline in async commands, they stall
other tasks on the same worker and make the UI feel frozen. With a multi-thread
runtime it is not a full deadlock, but it is avoidable.

**Fix**
- Wrap keyring and Argon2 calls in `tokio::task::spawn_blocking`.
- Replace `std::thread::sleep` with `tokio::time::sleep` if the retry loop stays
  async, or keep the whole loop inside `spawn_blocking`.
- Consider Argon2 params: default is memory-hard; document the chosen cost and
  show a progress indicator during unlock/save.
- Make `vault_unlock` and `vault_set_master_password` explicitly async-friendly.

**If we skip it**
Vault unlock and credential saves can freeze the app on slow/headless systems,
especially Linux without a running Secret Service.

---

## 18. Terminal output channels are unbounded

**Where**
- `src-tauri/src/commands/ssh.rs:47` — `mpsc::unbounded_channel::<Vec<u8>>()`.
- `src-tauri/src/commands/terminal.rs:24` — same for local PTY.
- `src-tauri/src/commands/ssh.rs` / `terminal.rs` — reader task sends every chunk;
  emitter task consumes and emits.

**What**
If the frontend cannot keep up (large output, slow IPC, background tab, blocked
main thread), the channel grows without limit. A `yes` or a large `cat` can
consume hundreds of MB. RDP already bounds its output channel to 16 frames
(`crates/rustius-rdp/src/ironrdp.rs:363-365`); terminal should have a similar
policy.

**Fix**
- Use a bounded channel (e.g., 256 KB-1 MB total) and apply backpressure to the
  reader, or drop oldest chunks with a visible "output truncated" marker.
- Coalesce small chunks in Rust before emitting (e.g., 8-16 ms batching).
- For PTY/SSH, backpressure can block the remote shell; that is usually better
  than unbounded memory. Alternatively, keep a bounded ring buffer and drop with
  a counter.
- Add a stress test with a command that emits 100 MB.

**If we skip it**
A single large output can OOM the app or make it unresponsive.

---

## 19. No CI, and one test requires a real OS keyring

**Where**
- No `.github/workflows/` directory (verified).
- `crates/rustius-core/src/secret.rs:417-430` — `test_secret_manager_roundtrip`
  uses `SecretManager::default()` and `set_secret`, i.e. the real OS keyring.
- `package.json:11-12` — `check` exists but is not enforced anywhere.
- `AGENTS.md` §5.1 says to run `cargo test --workspace`, `npm run check`, and
  `cargo clippy`, but nothing automates it.

**What**
There is no automated gate for formatting, clippy, Rust tests, Svelte type
checking, or the production build. The keyring test will fail or hang on a
headless CI runner without a Secret Service. It does clean up its entry, but it
still touches the developer's real keyring on every run. The TypeScript/Rust
drift in #11 and Appendix A would have been caught by a contract test.

**Fix**
- Add `.github/workflows/ci.yml`:
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace` (with the keyring test ignored or mocked)
  - `npm ci`
  - `npm run check`
  - `npm run build`
  - optionally `cargo tauri build` on one OS (Tauri Linux needs webkit2gtk
    packages).
- Make the keyring test hermetic: use a mock keyring backend or mark it
  `#[ignore]` with a separate `--ignored` job that has a keyring. Clean up the
  test entry.
- Add `cargo audit`/`cargo deny` and `npm audit` as non-blocking or scheduled
  jobs.
- Add a serialization contract test: Rust serializes `SshConfig`/`AppSettings`,
  a small TS test validates the JSON shape (or generate TS with `ts-rs`).

**If we skip it**
Regressions land silently, and the first CI run will be painful. The keyring test
is already non-hermetic.

---

## 20. Dead settings, dead fields, and unused dependencies

**Where**
- `crates/rustius-core/src/connection.rs:590-595` — `AppSettings.theme`,
  `show_sidebar`, `sidebar_width`, `default_terminal_type` are defined and saved.
- `src/lib/components/SettingsModal.svelte:27-29,231-233,409,429` — the UI edits
  `show_sidebar` and `default_terminal_type`, but nothing reads them.
  `theme` and `sidebar_width` are never edited.
- `crates/rustius-core/src/connection.rs:386-407` — `allow_legacy_algorithms`,
  `terminal_type`, `legacy_ciphers`, `legacy_kex`, `port_forwards` are never used
  by `russh::client::Config` or `request_pty` (`crates/rustius-ssh/src/client.rs`).
- `crates/rustius-core/src/connection.rs:426-441` — `RdpConfig.width`, `height`,
  `color_depth`, `fullscreen` are not read by `ironrdp_connect` (it takes
  `width`/`height` arguments and uses `domain`, `scale_factor`, `egfx`,
  `clipboard_sharing`).
- `crates/rustius-core/src/secret.rs:27` — `SecretRef::External` is never
  produced; `resolve` returns an error for it.
- `crates/rustius-core/src/connection.rs:412` — `PortForward` is only used by the
  dead `port_forwards` field.
- `crates/rustius-ssh/src/auth.rs:12-18` — `describe_auth_method` is only used by
  its own test.
- `src-tauri/src/commands/sftp.rs:251-266` — `sftp_download_bytes` is registered
  but never invoked (verified by cross-referencing all `invoke()` calls).
- `src/lib/components/SessionNotesDrawer.svelte:11` — `ExternalLink` imported,
  never used.
- Unused deps: `hmac` in `crates/rustius-core/Cargo.toml:27`;
  `thiserror` in `rustius-ssh`, `rustius-terminal`, `rustius-rdp`;
  `serde_json` in `rustius-rdp` is test-only (move to `[dev-dependencies]`);
  `uuid` and `serde_json` in `src-tauri/Cargo.toml:24,29` appear unused;
  `@xterm/addon-search` and `@xterm/addon-web-links` in `package.json:20-22` are
  never imported (`@xterm/addon-fit` is used).

**What**
This is a mix of "UI promises a setting that does nothing" and "dependency/code
weight with no behavior". The dead settings are worse than dead code: a user
toggles "Show sidebar" or "Legacy ciphers" and expects an effect. The legacy
toggles are security-relevant: a user might enable them for an old switch and
still fail, with no indication that the toggle is ignored.

**Fix**
- Either implement each setting or remove it from the type, UI, and defaults.
  Recommended:
  - Implement `terminal_type` by passing it to `request_pty`; implement
    `allow_legacy_algorithms` by configuring `russh` preferred algorithms (with
    a warning); implement `port_forwards` or remove it.
  - Implement `show_sidebar`/`sidebar_width` in `+page.svelte`/`Sidebar.svelte`,
    or remove them.
  - Remove `theme`/`default_terminal_type` if they are not going to be used.
  - Remove `RdpConfig.width/height/color_depth/fullscreen` or use them as
    fallbacks when the frontend does not provide dimensions.
- Delete `SecretRef::External` or implement a real provider (1Password, etc.).
- Delete `PortForward` if port forwarding is out of scope.
- Delete `describe_auth_method` (or use it in logs).
- Delete `sftp_download_bytes` and `SftpClient::download_bytes` unless a preview
  feature is planned.
- Remove unused deps; run `cargo machete`/`cargo udeps` and `depcheck` in CI.

**If we skip it**
Users cannot trust the settings UI, the security toggles are misleading, and the
build carries dependencies and IPC surface for no reason.

---

# Appendix A — 31 smaller findings

These are real but lower impact; each has a one-line fix.

1. **RDP `Pause` sends NumLock.** `crates/rustius-rdp/src/ironrdp.rs:1004` maps
   `"Pause"` to `(false, 0x45)`, the same scancode as `"NumLock"` (line 986).
   Pause requires the E1-prefixed sequence; if the input API cannot express it,
   return `None` instead of sending the wrong key.
2. **RDP frame tiles are not bounds-checked.** `RdpView.svelte:183-204` validates
   the buffer length but not `x + width <= desktopWidth`, `y + height <=
   desktopHeight`, or `dataLen === width * height * 4`. A malformed frame throws
   in `new ImageData`/`texSubImage2D` and aborts the rest of the batch. Validate
   and drop bad tiles, with a rate-limited log.
3. **WebGL context restore does not clean up old handles.**
   `src/lib/utils/rdpWebGlRenderer.ts:265-271` calls `setupPipeline()` after a
   context restore without clearing `program`/`vao`/`vbo`/`texture`. Old handles
   are overwritten; repeated restores can orphan JS-side objects and a failed
   `setupPipeline` leaves partial state. Null the fields and guard the failure
   path.
4. **IronRDP library default enables EGFX.** `crates/rustius-rdp/src/ironrdp.rs:285`
   uses `config.egfx.unwrap_or(true)`, while the app passes `default_rdp_egfx`
   (false) and `AGENTS.md` documents EGFX as disabled due to a decoder bug. Change
   the library default to `false`; re-enable only when upstream fixes progressive
   tile decoding.
5. **`sftp_download_to_downloads` silently overwrites.** `sftp.rs:218-244` joins
   `Downloads` + `file_name` and calls `download`, which uses `File::create`.
   Downloading `report.pdf` twice replaces the first copy. Add `report (1).pdf`
   collision suffixes or prompt.
6. **SFTP download leaves partial files on failure.**
   `crates/rustius-ssh/src/sftp.rs:174-190` writes directly to the destination.
   A network error leaves a truncated file that looks complete. Download to
   `.part`, then rename on success.
7. **Logs go to `%TEMP%` and grow without bound.** `src-tauri/src/lib.rs:31-40`
   opens `%TEMP%/rustius/rustius.log` in append mode, with `ironrdp=debug`, and
   no rotation. RDP debug can be huge; temp dirs may be world-readable. Use
   `tracing-appender` daily rolling files in the app data dir, cap retention, and
   drop `ironrdp=debug` in release.
8. **Corrupt `settings.json` silently resets to defaults.**
   `crates/rustius-core/src/config.rs:70-73` uses `unwrap_or_default()`. A parse
   error loses all settings without a warning. Log the error and back up the
   corrupt file; consider failing with a recovery UI.
9. **`ConfigManager::save` holds a read lock during synchronous file I/O.**
   `config.rs:451-476` keeps `self.config.read().await` while writing settings,
   notes, and connections. This blocks writers (and a Tokio worker) for the whole
   save. Clone the data under the lock, drop it, then write; use
   `spawn_blocking` for the filesystem work.
10. **TypeScript/Rust drift beyond keepalive.**
    - `SecretRef.external` TS has `provider`, `vault_id?`, `item_id`
      (`src/lib/types/index.ts:8`); Rust has `provider`, `id`
      (`crates/rustius-core/src/secret.rs:27`). A round-trip fails.
    - `Connection.last_connected` exists only in TS (`types/index.ts:108,121`);
      Rust has no such field. The UI will always see `undefined`.
    - `SshConfig` TS lacks `allow_legacy_algorithms` and `terminal_type`
      (`types/index.ts:45-50` vs `connection.rs:386-407`).
    Fix by generating TS from Rust (`ts-rs`/`specta`) or adding a contract test.
11. **`AGENTS.md` is stale.** It says config is `config.toml` (the code uses
    `settings.json` + `connections.json` + `notes/*.md` with legacy TOML
    migration), says `check_server_key` accepts all keys (the handler now
    implements TOFU in `crates/rustius-ssh/src/handler.rs`), and says EGFX is
    disabled by default (the library default is true; the app passes false).
    Update the guide so the next agent does not "fix" the wrong thing.
12. **`ConnectionModal`/`CredentialsModal` have no-op ternaries.**
    `ConnectionModal.svelte:446,453` and `CredentialsModal.svelte:278,285` use
    `password ? password : (existingSecretRef ? undefined : undefined)`, which is
    just `password ? password : undefined`. Simplify; the current form hides the
    intent.
13. **Secret placeholders only recognize `keyring`.**
    `ConnectionModal.svelte:791,850` and `CredentialsModal.svelte:591,644,707`
    check `existingSecretRef?.type === 'keyring'`; in portable/master-password
    mode the type is `vault`, so the placeholder says "••••••••" instead of
    "Stored in vault (leave empty to keep)". Check both.
14. **Copy buttons use `navigator.clipboard` with no fallback.**
    `ConnectionModal.svelte:139` (and the passphrase variant) call
    `navigator.clipboard.writeText` without a try/catch. If the webview denies
    clipboard access, the click rejects silently. Use
    `tauri-plugin-clipboard-manager` or catch and show an error.
15. **`SftpView` falls back to `C:\` on every OS.**
    `SftpView.svelte:341-347` calls `local_get_home_dir`, then
    `loadLocalDir('C:\\')` on failure. On Linux/macOS that is a relative path
    named `C:\`, not the filesystem root. Use `/` on Unix or show an error.
16. **`SftpSidebar` toast timer is not cleaned up.**
    `showToast` uses `setTimeout` to clear `transferToast`; if the component
    unmounts first, it writes to state after unmount. Track the timer in the
    `DisposableStore` and clear it on destroy.
17. **RDP clipboard staging overwrites same-named files.**
    `crates/rustius-rdp/src/clipboard/backend.rs:293-301` sanitizes the filename
    (good) but joins it to the staging dir and `std::fs::write`s; two remote
    files with the same name overwrite each other. Add a collision suffix or a
    per-transfer subdirectory.
18. **RDP known-hosts write failure is ignored.**
    `crates/rustius-rdp/src/ironrdp.rs:139-147` still returns `true` if the file
    cannot be written. The connection proceeds without persistence; the next
    connection re-learns. Log at `warn` (already) and consider failing closed or
    retrying; at minimum surface a UI warning for portable users.
19. **`AppState::new` failure panics with no UI.**
    `src-tauri/src/lib.rs:52-57` prints to stderr and `panic!`s if the data dir
    or config cannot be initialized. On Windows a GUI app may show nothing. Show
    a native error dialog or a minimal window explaining the failure.
20. **IronRDP git dependencies are not pinned to a revision.**
    `crates/rustius-rdp/Cargo.toml:20-27` uses `git = ...` without `rev`/`tag`.
    `Cargo.lock` pins the current commit, but `cargo update` can drift into an
    incompatible IronRDP. Pin `rev = "<sha>"` for reproducibility, especially
    with the local `[patch]` entries.
21. **`storage.rs` docs still describe TOML.**
    `crates/rustius-core/src/storage.rs:6-7` says "plain TOML files" and "Phase 2
    future: EncryptedStorage"; the code now writes JSON and uses
    `EncryptedStorage` for the vault. Update the doc so the abstraction's intent
    is clear.
22. **`ConnectionModal` resets `connect_timeout_secs` to 15 on every edit.**
    `ConnectionModal.svelte:475` hardcodes `15` and never loads
    `c.ssh_config.connect_timeout_secs` (line 202 only loads keepalive). A user
    who changed the timeout loses it when editing the name. Load and preserve the
    field.
23. **`RdpConfig.width`/`height` are hardcoded on save.**
    `ConnectionModal.svelte:485-486` writes 1920x1080 unless the override toggle
    is on; `RdpView` computes the real size from the viewport. The config values
    are misleading and unused. Remove them or use them as a fallback.
24. **`get_config` clones the entire config including notes.**
    `ConfigManager::get_config` (`config.rs:446-448`) clones all notes on every
    call; the frontend calls it after every mutation. For large runbooks this is
    unnecessary. Return a shallow summary for list views and fetch notes on
    demand (`get_connection_note` already exists).
25. **Startup reads every note file.**
    `config.rs:1229-1251` reads `notes/{id}.md` for every connection during
    load. With hundreds of connections this is many small blocking reads.
    Consider lazy loading notes when a connection is opened.
26. **`secure_all_unencrypted_secrets` is O(n) and runs on status.**
    Already covered in #10; if it stays, at least debounce it and do not run it
    on every `vault_get_status`.
27. **`sftp_download_bytes` is dead IPC surface.**
    Registered in `src-tauri/src/lib.rs:17`, never invoked. Remove it and
    `SftpClient::download_bytes`, or wire it to a preview feature with a size
    limit.
28. **`describe_auth_method` is test-only.**
    `crates/rustius-ssh/src/auth.rs:12-18`; use it in connection logs or remove
    it.
29. **`SecretManager` does not zeroize secrets.**
    `VaultState.secrets` is a `HashMap<String, String>`; `lock_vault` clears it
    but does not zero the memory. `EncryptedStorage.master_password` is a
    `String`. Consider `zeroize`/`secrecy` for master passwords and vault values.
30. **`RdpView` sends one `invoke` per key event and per mouse move.**
    Mouse moves are rAF-coalesced (`RdpView.svelte:280-292`), but key events are
    not (`RdpView.svelte:375-400`). Rapid typing/repeat generates many IPC calls.
    Batch key events per frame or use a `Channel`/raw request.
31. **`SftpSidebar` uploads sequentially with no progress/cancel.**
    `uploadFileList` awaits each file; the toast is indeterminate. For large
    files, users cannot cancel. Add progress events from Rust and a cancel
    token.

---

# Appendix B — positive findings (keep these)

- **SSH TOFU is implemented properly.** `crates/rustius-ssh/src/handler.rs`
  checks known hosts, learns on first use, and rejects changed keys. This
  contradicts the stale `AGENTS.md` note.
- **RDP TOFU is implemented** with SHA-256 fingerprints and fails closed on read
  errors (`crates/rustius-rdp/src/ironrdp.rs:45-147`). The write path needs #16.
- **RDP clipboard filenames are sanitized** before writing:
  `crates/rustius-rdp/src/clipboard/backend.rs:293-296` uses `file_name()`,
  rejects `.`/`..`, and rejects `:`. This is the pattern SFTP downloads should
  copy.
- **RDP output is bounded** to 16 frames (`ironrdp.rs:363-365`) and frames use a
  binary `Channel`, not JSON arrays. Terminal should copy this.
- **`rustius-core` has meaningful tests** for config, vault round-trip, storage,
  importers, and connection serialization. The main gap is hermetic keyring
  testing and frontend tests.
- **TypeScript is strict** (`tsconfig.json:11`), and the project uses Svelte 5
  runes correctly in the store and components.
- **The IronRDP patches are wired** via `[patch."https://github.com/Devolutions/IronRDP"]`
  in the root `Cargo.toml:106-108`; the vendored crates are not orphaned.
- **The build profiles are deliberate** (fast dev, optimized release) and the
  `cargo tauri build` warning in `AGENTS.md` is correct.

---

# Suggested fix order

1. **Security chain (day 1-2):** #1 (sanitize + CSP), #2 (no plaintext fallback,
   sanitize `get_config`), #3 (strict vault), #4/#5 (path traversal + local FS
   scope), #7 (openPath permission).
2. **Data integrity (day 2-3):** #6 (atomic writes), #8 (single instance), #9
   (restore-defaults migration), #16 (RDP known-hosts atomic write).
3. **Correctness/UX (day 3-4):** #11 (keepalive), #14 (error handling), #15
   (IDs), #10 (status side effect), #19 (CI + hermetic tests).
4. **Performance (day 4-5):** #12 (terminal IPC), #13 (upload), #17 (blocking
   keyring/Argon2), #18 (unbounded channels).
5. **Cleanup (ongoing):** #20 (dead settings/deps), Appendix A, `AGENTS.md`
   update.

Do not do #20 before #1/#2: removing settings is easy, but the security fixes
change the same files and should land first.

---

# Verification notes

Checked against upstream docs and the locally installed crates:

- Tauri v2 **command return values are JSON-serialized**; `tauri::ipc::Response`
  is required for `ArrayBuffer`/binary returns. This is why the RDP `Channel` is
  correct and the terminal `Vec<u8>` event is not.
  <https://v2.tauri.app/develop/calling-rust/>
- Tauri v2 **events only support JSON payloads**; `Channel`/`Response` is the
  binary path.
  <https://v2.tauri.app/develop/calling-frontend/>
- Tauri v2 **raw request bodies** are available through `tauri::ipc::Request`
  (`InvokeBody::Raw`), useful for terminal input/upload.
- `tauri-plugin-opener` **2.7.0** default permission set was inspected locally:
  `allow-open-url`, `allow-reveal-item-in-dir`, `allow-default-urls`. It does
  **not** include `allow-open-path`, so `SftpSidebar.svelte:651` is broken.
  <https://v2.tauri.app/plugin/opener/>
- `marked` documentation states it **does not sanitize** output; use a sanitizer
  such as DOMPurify.
  <https://marked.js.org/#security>
- Tauri v2 **CSP** guidance:
  <https://v2.tauri.app/security/csp/>
- Tauri v2 **single-instance** plugin:
  <https://v2.tauri.app/plugin/single-instance/>

No live SSH/SFTP/RDP server was contacted; protocol behavior was reviewed
statically. No fuzzing was performed.

---

# Not checked

- Did not run `cargo test --workspace`, `cargo clippy`, `cargo fmt --check`,
  `npm run check`, or `cargo tauri build`. The keyring test alone makes a clean
  run environment-dependent.
- Did not inspect the uncommitted `package-lock.json` diff.
- Did not audit `patches/ironrdp-graphics` and `patches/ironrdp-egfx`
  line-by-line; they are vendored upstream code. The `[patch]` wiring was
  checked.
- Did not test on macOS or Linux; Windows-specific paths (atomic rename,
  `USERPROFILE`, `C:\`) were reviewed from source.
- Did not test the OS keyring on any platform, including locked/headless Linux.
- Did not fuzz the mRemoteNG/MobaXterm importers or the SFTP directory parser.
- Did not review every CSS rule, animation, icon, or accessibility attribute.
- Did not verify the "~11 MB executable" claim in the landing page.
- Did not review generated schemas/icons or the Tauri bundler output.
- Did not test RDP graphics/EGFX against a real Windows host; the EGFX default
  finding is based on code and the documented upstream limitation.

---

# Verdict

Rustius has good bones: a clean crate split, a real encrypted vault with
Argon2id + AES-GCM, working SSH/RDP TOFU, a bounded RDP frame channel, strict
TypeScript, and meaningful core tests. It is **not production-safe yet**. The
combination of unsanitized Markdown, `csp: null`, plaintext secret fallbacks, and
unrestricted local filesystem commands is a credible credential-theft and
file-destruction chain. Config writes can lose the vault on a crash, and several
settings in the UI are decorative.

Fix the Must-fix group before shipping to anyone who stores real credentials.
Then add CI and the IPC/upload performance work; those are the changes that will
keep the app pleasant to use under real load.

**Lean:** about **-9 dependencies** and **-400 to -700 lines** are possible
without losing features: remove `hmac`, the three unused `thiserror` deps, the
test-only/misplaced `serde_json` deps, `uuid` in `src-tauri`, the two unused
xterm addons, dead settings/fields (`theme`, `show_sidebar`, `sidebar_width`,
`default_terminal_type`, legacy SSH toggles, `port_forwards`, `RdpConfig`
width/height/color_depth/fullscreen, `SecretRef::External`, `PortForward`,
`describe_auth_method`, `sftp_download_bytes`), and the no-op ternaries/unused
imports. The bigger win is fewer promises the UI cannot keep.

---

*End of audit. Move this file into the repo after switching out of Plan mode.*
