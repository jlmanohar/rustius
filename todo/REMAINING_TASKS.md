# Rustius — Remaining Tasks & Roadmap

> **Target Architecture:** Tauri v2 + Rust Workspace + Svelte 5 (Runes)  
> **Storage Mode:** Portable (`./rustius-data/config.toml` alongside executable)  
> **Current Status:** Phases 1–6 and Phase 7.1 (Scaffolding, Models, SSH, SFTP, FreeRDP, Geist UI, and Local PTY) are **100% Complete & Built**.

---

## 📊 Summary of Remaining Phases

| Priority | ID | Feature / Component | Scope & Target Files |
|:---:|:---|:---|:---|
| **P1** | **PHASE-7.2** | **Split-Pane Workspace** | Multi-session side-by-side & stacked layouts, draggable dividers, active pane tracking (`SplitPane.svelte`, `appState.svelte.ts`, `+page.svelte`). |
| **P1** | **PHASE-8.1** | **Encrypted Config Vault** | AES-256-GCM + Argon2id master password vault for connection credentials (`rustius-core/storage.rs`, `crypto.rs`, `MasterKeyModal.svelte`). |
| **P2** | **PHASE-8.2** | **SSH Key Management UI** | ED25519 & RSA key generator, fingerprint viewer, key manager dialog (`rustius-ssh/keys.rs`, `KeyManagerModal.svelte`). |
| **P2** | **PHASE-8.3** | **Visual Port Forwarding Manager** | Visual SSH tunnels (`-L`, `-R`, SOCKS5 `-D`), tunnel monitor panel (`rustius-ssh/tunnel.rs`, `TunnelManagerModal.svelte`). |
| **P3** | **PHASE-8.4** | **Terminal Session Logging** | Live session transcript recording to timestamped files on disk (`rustius-ssh`, `rustius-terminal`, `TerminalView.svelte`). |
| **P3** | **PHASE-8.5** | **Command Snippets Library** | Reusable command palette with variable substitution (`{{var}}`), quick-run drawer (`rustius-core`, `SnippetsDrawer.svelte`). |
| **P3** | **PHASE-8.6** | **Config Backup & Export / Import** | One-click JSON/TOML configuration backup, import with conflict resolution (`rustius-core/export.rs`, `SettingsModal.svelte`). |
| **P4** | **PHASE-8.7** | **Portable Packaging & Update Check** | Single-file portable packaging, release archive scripts, optional GitHub release update check (`src-tauri`). |

---

## 1. Phase 7.2: Split-Pane Workspace

### Goal
Allow developers to arrange multiple active tabs in split panes simultaneously (e.g. SSH terminal side-by-side with SFTP file manager, or dual PowerShell / SSH terminals).

### Key Requirements
- [ ] **Data Model for Pane Hierarchy**:
  - Replace single `activeTabId` in `appState.svelte.ts` with a flexible binary pane tree or multi-pane layout:
    ```typescript
    export type PaneOrientation = 'horizontal' | 'vertical';

    export type PaneNode =
      | { type: 'leaf'; id: string; tabId: string }
      | {
          type: 'split';
          id: string;
          orientation: PaneOrientation;
          ratio: number; // e.g. 0.5 (50/50 split)
          first: PaneNode;
          second: PaneNode;
        };
    ```
- [ ] **SplitPane Component (`src/lib/components/SplitPane.svelte`)**:
  - Responsive flexbox or grid layout supporting horizontal (`row`) and vertical (`column`) orientation.
  - Draggable gutter divider (`.split-divider`) with mouse drag event listeners.
  - Minimum size limits (e.g., minimum 200px or 20% width/height per pane).
  - Double-click divider to reset to equal 50/50 ratio.
- [ ] **Tab Bar Integration**:
  - Right-click tab context menu actions:
    - *Split Right* (horizontal split with selected or new session)
    - *Split Down* (vertical split with selected or new session)
    - *Unsplit / Maximize Pane*
  - Keyboard shortcuts: `Ctrl+Shift+D` (split right), `Ctrl+Shift+E` (split down), `Ctrl+W` (close focused pane).
- [ ] **Focus Management**:
  - Clicking inside any pane marks it as active in `appState`.
  - Terminal cursor / canvas focus routes directly to active pane.

---

## 2. Phase 8.1: Encrypted Config Vault (AES-256-GCM)

### Goal
Secure stored passwords, passphrases, and private keys with zero plaintext credential exposure on disk.

### Key Requirements
- [ ] **Core Cryptography (`crates/rustius-core/src/vault.rs`)**:
  - Key derivation: `Argon2id` (memory-hard, resistant to GPU attacks).
  - Cipher: `aes-gcm` (256-bit key, 96-bit random nonce, 128-bit authentication tag).
  - Format on disk:
    ```toml
    [vault]
    enabled = true
    salt = "base64-encoded-argon2-salt"
    iterations = 3
    memory_kib = 65536
    nonce = "base64-encoded-nonce"
    ciphertext = "base64-encoded-encrypted-payload"
    ```
- [ ] **Storage Trait Implementation**:
  - Implement `ConfigStorage` trait for `EncryptedVaultStorage`.
  - In-memory decrypted cache cleared on application lock/exit.
- [ ] **Master Password Modal (`src/lib/components/MasterKeyModal.svelte`)**:
  - Prompt user for master password on startup if vault is enabled.
  - Option to set/change master password in Settings.
  - Seamless migration tool: encrypt existing plaintext `config.toml` into vault.

---

## 3. Phase 8.2: SSH Key Management UI & Storage

### Goal
Provide a built-in SSH key manager to generate, import, inspect, and export SSH key pairs directly inside Rustius.

### Key Requirements
- [ ] **Backend Key Generation (`crates/rustius-ssh/src/keys.rs`)**:
  - Support ED25519 (recommended, fast, secure) and RSA (2048/4096-bit).
  - Support optional passphrase protection on generated private keys.
  - Generate OpenSSH formatted public/private key pairs.
  - Extract SHA-256 and MD5 fingerprints.
- [ ] **Tauri Commands (`src-tauri/src/commands/keys.rs`)**:
  - `generate_key_pair(algorithm: String, comment: String, passphrase: Option<String>) -> KeyPair`
  - `list_keys() -> Vec<KeyMetadata>`
  - `import_key_file(path: String) -> Result<KeyMetadata>`
  - `delete_key(key_id: String) -> Result<()>`
- [ ] **Frontend Modal (`src/lib/components/KeyManagerModal.svelte`)**:
  - View list of keys with icon, algorithm, bit length, fingerprint, and created date.
  - "Generate Key" dialog with algorithm selector and passphrase field.
  - One-click "Copy Public Key" button for easy `authorized_keys` pasting.
  - Direct selection in `ConnectionModal.svelte` under KeyFile authentication.

---

## 4. Phase 8.3: Visual Port Forwarding Manager (SSH Tunnels)

### Goal
Manage SSH tunnels (Local, Remote, and Dynamic SOCKS5) with a dedicated GUI dashboard showing active status and data throughput.

### Key Requirements
- [ ] **Backend Tunnel Orchestrator (`crates/rustius-ssh/src/tunnel.rs`)**:
  - Local Port Forwarding (`-L <local_port>:<remote_host>:<remote_port>`):
    - Tokio TCP listener on `127.0.0.1:<local_port>`.
    - Forward connection over `russh` direct-tcpip channel.
  - Remote Port Forwarding (`-R <remote_port>:<local_host>:<local_port>`):
    - Request remote port forward via `russh::client::tcpip_forward`.
  - Dynamic SOCKS5 Proxy (`-D <local_port>`):
    - Embedded Tokio SOCKS5 server forwarding requests over SSH channel.
- [ ] **Tauri Commands**:
  - `start_tunnel(connection_id, tunnel_config) -> Result<tunnel_id>`
  - `stop_tunnel(tunnel_id) -> Result<()>`
  - `list_active_tunnels() -> Vec<TunnelStatus>`
- [ ] **Frontend UI (`src/lib/components/TunnelManagerModal.svelte`)**:
  - Visual tunnel builder with diagram of Local Host ➔ SSH Host ➔ Remote Target.
  - Live status pills (Active 🟢, Connecting 🟡, Stopped ⚪).
  - Transferred byte counters (Tx / Rx).

---

## 5. Phase 8.4: Terminal Session Logging

### Goal
Record raw or ANSI-stripped terminal session streams directly to timestamped log files on disk for compliance and debugging.

### Key Requirements
- [ ] **Backend Logging Handler**:
  - Configurable log folder: `./rustius-data/logs/`.
  - Buffered async file writer attached to session output channels (`russh` output and `portable-pty` output).
  - Mode options: Plain Text (strip ANSI escape codes) or Raw ANSI (exact terminal reproduction).
- [ ] **Tauri Commands**:
  - `start_session_log(tab_id: String, raw_ansi: bool) -> Result<PathBuf>`
  - `stop_session_log(tab_id: String) -> Result<()>`
- [ ] **Frontend Controls**:
  - "Start Recording" button in `TerminalView.svelte` header.
  - Pulsing red "REC" indicator when logging is active.
  - Quick action to "Open Log Folder" in native file manager.

---

## 6. Phase 8.5: Command Snippets Library

### Goal
Provide a library of reusable server management scripts, shell snippets, and one-liners that can be executed or inserted into any active terminal.

### Key Requirements
- [ ] **Data Model (`crates/rustius-core/src/snippet.rs`)**:
  ```rust
  pub struct Snippet {
      pub id: String,
      pub name: String,
      pub description: Option<String>,
      pub command: String,
      pub tags: Vec<String>,
      pub category: String, // e.g. "Docker", "Nginx", "System"
  }
  ```
- [ ] **Variable Substitution**:
  - Support syntax like `{{server_name}}`, `{{port}}`, `{{container_id}}`.
  - Prompt user with quick input dialog for missing variables before pasting.
- [ ] **Frontend UI (`src/lib/components/SnippetsDrawer.svelte`)**:
  - Collapsible side drawer or modal searchable by name, tag, or command content.
  - Actions: "Insert into Terminal", "Run with Enter", "Copy to Clipboard", "Edit".

---

## 7. Phase 8.6: Config Backup, Export & Import

### Goal
Allow transferring connections and settings between machines or restoring from automatic backups.

### Key Requirements
- [ ] **Export & Import Engine (`crates/rustius-core/src/export.rs`)**:
  - Export entire connection tree or selected folders to `.json` or `.toml`.
  - Option to sanitize passwords/keys before export (safe sharing mode).
  - Import parser with preview and conflict resolution:
    - *Overwrite existing*
    - *Create duplicates with new IDs*
    - *Skip conflicting connections*
- [ ] **Automatic Rolling Backups**:
  - Create backup snapshot in `./rustius-data/backups/config-<timestamp>.bak` before every write operation.
  - Retain last 10 snapshots automatically.
- [ ] **UI Integration**:
  - Dedicated "Backup & Sync" tab in `SettingsModal.svelte`.

---

## 8. Phase 8.7: Portable Packaging & Release Assets

### Goal
Ensure seamless single-directory distribution without registry or system dependencies.

### Key Requirements
- [ ] **Release Packaging Script (`scripts/package-portable.ps1`)**:
  - Compiles release binary with `cargo build --profile release -p rustius`.
  - Packages `rustius.exe`, default `rustius-data/config.toml`, and `README.md` into a single standalone `.zip`.
- [ ] **Update Check Utility**:
  - Optional query to GitHub Releases API (`https://api.github.com/repos/.../releases/latest`) to notify the user when a newer version is available.
