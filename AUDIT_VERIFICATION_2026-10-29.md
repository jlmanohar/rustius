# Rustius Audit Verification — 2026-10-29

**Subject:** Verification of the fixes in commit `50bbd75` (`fix(audit): remediate all findings from 2026-10-28 codebase audit`), plus a regression scan of that commit.

**Reference:** `C:/Users/jlmanohar/.opencode/plan/rustius-audit-2026-10-28.md`

**Method:** Read the full remediation diff, re-read every changed source path, cross-checked the pre-fix implementation with `git show 50bbd75^`, and ran the project checks. No application code was modified during this audit.

**Checks run:**

- `cargo test --workspace` — all suites pass (38 `rustius-core`, 9 `rustius_lib`, 8 `rustius-rdp`, 2 `rustius-ssh`, 1 `rustius-terminal`; 1 keyring test ignored).
- `npm run check` — 0 errors, 0 warnings.

**Bottom line:** All 13 findings and all 8 low/hardening items were addressed, but **finding 4 and L5 are only partially fixed**, and the remediation introduced **one high-severity credential regression**, one medium startup/shutdown issue, and several smaller regressions.

---

## 1. Verification of the 2026-10-28 findings

| # | Finding | Status |
|---|---|---|
| 1 | Local "New Folder" always fails | ✅ Fixed — canonicalizes the nearest existing ancestor and appends the leaf; regression tests added |
| 2 | `migrate_vault_backend` loses secrets on abort | ✅ Fixed — proper two-phase commit; old secrets deleted only after the config save succeeds |
| 3 | `remove_node` / `delete_credential` delete before save | ✅ Fixed — config is persisted before secrets/notes are purged |
| 4 | Corrupt config panics / silent reset | ⚠️ **Partial** — no panic, timestamped `.corrupt.<ts>` backups are written, and a fallback state exists. But recovery is silent: corrupt `connections.json` starts the app with an empty tree and no restore UI; corrupt `settings.json` resets `vault_backend` to `Auto` and can hide the vault-unlock panel (see N4) |
| 5 | RDP session eviction race | ✅ Fixed — per-session UUID and identity check on eviction |
| 6 | Duplicate SFTP handshake on reconnect | ✅ Fixed, but introduced a two-click reconnect regression (N3) |
| 7 | Vault backend silently falls back to `Auto` | ✅ Fixed — `AtomicU8` cache, no `try_read()` guessing |
| 8 | Argon2id/AES-GCM blocks the async runtime | ✅ Fixed — `run_blocking_or_direct` uses `block_in_place` only on multi-thread runtimes; safe inside `spawn_blocking` (Tokio handles the nested case) |
| 9 | Settings save failure swallowed | ✅ Fixed — error propagates, UI state is not optimistically kept |
| 10 | Auth-type switch reuses wrong secret | ✅ Fixed — `existingSecretRef` is cleared/rederived per auth tab |
| 11 | Download path check over/under-strict | ✅ Fixed — nearest-ancestor resolution, only real `..` components rejected |
| 12 | Download deletes destination before rename | ✅ Fixed — `atomic_replace_file` with `ReplaceFileW`; minor temp-file edge case in N7 |
| 13 | SSH cleanup evicts newer SFTP session | ✅ Fixed — identity guard before removing the SFTP session |

### Low / hardening items

- **L1** mRemoteNG folder popping — fixed; explicit-close nodes handled and tested.
- **L2** Unbounded clipboard staging — fixed; per-session directory and cleanup on disconnect/drop. Only leaks an empty directory if connect fails (N6).
- **L3** TS/Rust type drift — mostly fixed; one cosmetic leftover: TS `SecretRef.external` still carries optional `vault_id` / `item_id` that Rust never emits (`src/lib/types/index.ts:3-8`).
- **L4** IPC sanitization leaves `SecretRef::Plaintext` — fixed.
- **L5** `save()` not transactional — ⚠️ **Partial**. Only an in-memory rollback was added in `update_config` (`crates/rustius-core/src/config.rs:595-614`). `save()` still writes settings → notes → connections non-transactionally (`:503-535`), `move_node` still mutates memory then saves without rollback (`:726-752`), and the rollback can clobber a concurrent update on I/O failure (N8).
- **L6** Windows path-guard false positives — fixed; checks are root-anchored.
- **L7** No way to clear a saved password — fixed.
- **L8** Onboarding forces a master password — ⚠️ **Partial**. A "Skip" button now exists (`VaultUnlockModal.svelte:201-218`), but in portable mode the effective backend remains `MasterPassword`; skipping leaves the vault locked, so saving any credential fails, and the modal reopens on every launch (`src/lib/stores/appState.svelte.ts:107-121`).

---

## 2. New issues

### N1 — HIGH: `secure_all_unencrypted_secrets` rewrites secrets it never copied

**Location:** `crates/rustius-core/src/config.rs:1114-1275`

The collector only gathers inline plaintext or `SecretRef::Plaintext` (`:1127-1163`), but the rewriter unconditionally changes **every** Password/KeyFile `secret_ref` to the active backend (`:1216-1249`), including existing `Keyring`, `Vault`, `External`, `AskOnConnect`, and entries with no secret at all. The old implementation only rewrote refs for entries it actually secured.

Real trigger: portable mode with `vault_backend: auto` resolves to `MasterPassword`. If the config carries `Keyring` refs (config copied from an installed setup, a `.portable` marker added later, or corrupt `settings.json` resetting the backend to `auto`), unlocking the vault rewrites all of them to `SecretRef::Vault { key }` — but nothing is copied from the keyring. `get_vault_secret` then returns `Ok(None)` for the missing key, so every saved password silently stops working; the real secrets remain orphaned in the OS keyring. The reverse combination (installed/Auto → Keyring with `Vault` refs, e.g. via the Settings "Set Master Password" flow in `vault_set_master_password`, `src-tauri/src/commands/config.rs:291-302`) loses vault credentials the same way.

Secondary effects: "ask on connect" entries lose that behavior, `External` refs are silently converted to an unimplemented backend, and no-secret connections gain dangling refs.

**Fix direction:** return the set of keys actually secured from the collector and rewrite only those; preserve any existing non-plaintext ref, or migrate it explicitly the way `migrate_vault_backend` does. Add a test with mixed `Keyring`/`Vault`/no-secret entries in both portable and installed modes.

### N2 — MEDIUM: `tokio::spawn` inside the window-destroy handler

**Location:** `src-tauri/src/lib.rs:111-150`

`on_window_event` runs on the Tao main thread, which is not inside a Tokio runtime context. `tokio::spawn` calls `Handle::current()` and panics ("there is no reactor running"). With the release profile's `panic = "abort"`, closing the window can abort the process, and the session-cleanup task never runs. Use `tauri::async_runtime::spawn` (it enters the runtime explicitly) or do the cleanup synchronously. This path is untested.

### N3 — MEDIUM: SFTP Reconnect takes two clicks after a terminal reconnect

**Location:** `src/lib/components/SftpSidebar.svelte:109-126`

`handleReconnect` sets `wasDisconnected = false` before calling `onreconnect`. The `$effect` only calls `initSession()` when `isTerminalDisconnected` flips false **and** `wasDisconnected` is still true. Because both flags were already cleared by the time the effect runs, the first click reconnects the SSH terminal only; the SFTP pane stays disconnected until a second click (which takes the `else` branch). Let the effect own the reconnect, or don't reset `wasDisconnected` in `handleReconnect`.

### N4 — MEDIUM: Corrupt-settings recovery silently changes security state

**Location:** `crates/rustius-core/src/config.rs:88-135`, `src-tauri/src/lib.rs:52-79`

A parse failure in `settings.json` is backed up (good) but the app starts with defaults and **no user-facing notice**. `vault_backend` becomes `auto`; in installed mode this hides the vault-unlock panel, so `Vault`-ref connections fail with "Vault is locked" until the user manually reselects Master Password and unlocks. In portable mode it flips to `MasterPassword` and can trigger N1 on the next unlock. Corrupt `connections.json` similarly yields a silent empty tree. The fallback state also uses `is_portable: true` with `%TEMP%\rustius-recovery`, so credentials saved during "recovery" go into an ephemeral vault that disappears.

**Recommendation:** show a recovery banner with the backup path and a restore action; when `vault.enc` exists, infer `MasterPassword` instead of `Auto`; don't present portable setup UX for the fallback state.

### N5 — LOW: RDP mouse release sent twice

**Location:** `src/lib/components/RdpView.svelte:763-767` and `:1023`

The new window-level `mouseup` listener and the canvas `onmouseup` both call `handleMouseUp`, issuing two release RPCs per click. The release is idempotent, so this is harmless but noisy; keep the window listener (for releases outside the canvas) and drop the canvas one.

### N6 — LOW: RDP staging directory leaks on failed connect

**Location:** `crates/rustius-rdp/src/ironrdp.rs:385-389`

The staging directory is created before `RdpClient::connect`. If connect fails, no session object exists, so `Drop` never removes it. Create it after a successful connect or clean it up on the error path.

### N7 — LOW: `atomic_replace_file` deletes the temp even when replacement fails

**Location:** `crates/rustius-core/src/storage.rs:44-66`

On Windows, `std::fs::remove_file(src)` runs before checking `ReplaceFileW`'s result. If the replace fails (e.g. destination locked by another process), the downloaded `.part` is deleted and the error is returned. The old destination survives, so there is no data loss, but the completed download is thrown away. Move the cleanup into the success branch.

### N8 — LOW: `update_config` rollback can discard a concurrent mutation

**Location:** `crates/rustius-core/src/config.rs:595-614`

On save failure the pre-closure snapshot is restored. If another command mutated the config between snapshot and failure, that newer change is rolled back too and may be persisted by the next save. Only reachable on I/O failure, but a save mutex or per-update diff would make the rollback safe.

### Note: CSP is `null` again

`src-tauri/tauri.conf.json` was changed back to `"csp": null`. `AGENTS.md` documents this as intentional for WebView2 font/xterm issues, and both `{@html}` sites (`SessionNotesDrawer.svelte`, `ConnectionModal.svelte`) sanitize with DOMPurify. It is still a loss of defense-in-depth: any future raw HTML sink is directly exploitable. If the font problem can be scoped, a CSP with `font-src`/`style-src` exceptions would be safer; otherwise consider a lint rule banning unsanitized `{@html}`.

---

## 3. Suggested tests to close the gaps

1. `secure_all_unencrypted_secrets` with mixed `Keyring` / `Vault` / no-secret / `Plaintext` entries, in both installed and portable modes — assert only plaintext entries change.
2. Corrupt `settings.json` and `connections.json` startup: backup created, `vault_backend` preserved when `vault.enc` exists, and a recovery notice is surfaced.
3. Window-destroy cleanup: assert no panic and that all four session maps are drained (manual or integration).
4. SFTP reconnect sequence: disconnect terminal → click SFTP Reconnect → both reconnect on the first click.
5. `atomic_replace_file` with a locked destination: temp handling and destination integrity.
6. RDP mouse press/release: exactly one release RPC per button.

---

## 4. Summary of severity

| Severity | Count | Items |
|---|---|---|
| High | 1 | N1 — credential refs rewritten without copying |
| Medium | 3 | N2 shutdown panic, N3 SFTP two-click reconnect, N4 silent security-state reset |
| Low | 4 | N5 duplicate RDP mouseup, N6 staging-dir leak, N7 temp deletion on failed replace, N8 rollback clobber |
| Note | 1 | CSP `null` trade-off |

**Recommended order:** N1 first (credential-loss risk), then N2 (crash on close), N3/N4 (user-facing regressions), then the low items. L5's transactionality should be finished alongside N8.

---

*Audit only. No repository files were modified.*
