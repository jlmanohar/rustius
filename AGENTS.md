# Rustius — Agent Guide

> **Rustius** is a cross-platform SSH, SFTP, and RDP desktop client built with
> **Tauri v2** (Rust backend) and **SvelteKit + Svelte 5** (frontend).
> It ships as a single native binary powered by the OS webview.

---

## 1. Project Architecture

### 1.1 High-Level Overview

```
┌──────────────────────────────────────────────────────────────┐
│                     Tauri v2 Shell                           │
│  ┌─────────────────────┐  IPC (invoke/events)  ┌──────────┐ │
│  │   Svelte 5 (SPA)    │◄─────────────────────►│  Rust    │ │
│  │   SvelteKit          │                       │  Backend │ │
│  │   @xterm/xterm       │                       │          │ │
│  │   Lucide Icons       │                       │          │ │
│  └─────────────────────┘                        └──────────┘ │
└──────────────────────────────────────────────────────────────┘
```

### 1.2 Directory Structure

```
rustius/
├── Cargo.toml                  # Workspace root (resolver = "2")
├── package.json                # Frontend deps (Svelte 5, xterm, Tauri API)
├── svelte.config.js            # SvelteKit with adapter-static (SPA mode)
├── vite.config.js              # Vite build config
├── tsconfig.json               # TypeScript config
├── DESIGN.md                   # Vercel Geist design system reference
│
├── src/                        # ── Frontend (Svelte 5 + SvelteKit) ──
│   ├── app.html                # HTML shell
│   ├── app.css                 # Global styles / CSS variables
│   ├── routes/
│   │   ├── +layout.svelte      # Root layout
│   │   ├── +layout.ts          # SSR disabled (ssr = false, prerender = false)
│   │   └── +page.svelte        # Main application page (all views)
│   └── lib/
│       ├── components/         # Svelte 5 components
│       │   ├── Sidebar.svelte          # Connection tree + search + env filter
│       │   ├── TabBar.svelte           # Tab strip with context menus
│       │   ├── TerminalView.svelte     # xterm.js terminal (SSH + local PTY)
│       │   ├── SftpView.svelte         # Dual-pane SFTP file manager
│       │   ├── RdpView.svelte          # Canvas-based RDP viewer
│       │   ├── ConnectionModal.svelte  # Add/edit connection dialog
│       │   └── SettingsModal.svelte    # Application settings dialog
│       ├── stores/
│       │   └── appState.svelte.ts      # Class-based reactive store (Svelte 5 runes)
│       ├── types/
│       │   └── index.ts                # Shared TypeScript interfaces
│       └── utils/
│           └── disposable.ts           # VS Code-style disposable resource management
│
├── src-tauri/                  # ── Tauri Backend (Rust) ──
│   ├── Cargo.toml              # Tauri app crate dependencies
│   ├── tauri.conf.json         # Tauri v2 window config, CSP, bundling
│   ├── build.rs                # Tauri build script
│   ├── capabilities/           # Tauri v2 capability permissions
│   └── src/
│       ├── main.rs             # Entry point (calls lib::run)
│       ├── lib.rs              # Tauri builder, plugin registration, command handlers
│       ├── state.rs            # AppState: session maps + config manager
│       └── commands/
│           ├── mod.rs          # Module declarations
│           ├── config.rs       # get_config, add_connection, add_folder, etc.
│           ├── ssh.rs          # ssh_connect, ssh_write, ssh_resize, ssh_disconnect
│           ├── sftp.rs         # sftp_connect, sftp_list_dir, sftp_upload/download, etc.
│           ├── rdp.rs          # rdp_connect, rdp_disconnect, rdp_key/mouse_event
│           └── terminal.rs     # pty_spawn, pty_write, pty_resize, pty_close
│
└── crates/                     # ── Rust Workspace Crates ──
    ├── rustius-core/           # Config management, connection models, storage
    │   └── src/
    │       ├── lib.rs
    │       ├── connection.rs   # Connection, AuthMethod, TreeNode, RustiusConfig
    │       ├── config.rs       # ConfigManager (Arc<RwLock<T>> based)
    │       ├── storage.rs      # ConfigStorage trait + PlaintextStorage
    │       └── error.rs        # RustiusError (thiserror)
    ├── rustius-ssh/            # SSH client (russh) + SFTP (russh-sftp)
    │   └── src/
    │       ├── lib.rs
    │       ├── client.rs       # SshSession: connect, write, resize, disconnect
    │       ├── sftp.rs         # SftpClient: list_dir, upload, download, mkdir, etc.
    │       └── auth.rs         # Auth helpers
    ├── rustius-rdp/            # RDP client (IronRDP + raw X.224/FastPath)
    │   └── src/
    │       ├── lib.rs
    │       └── client.rs       # RdpSession: X.224 negotiation, frame streaming
    └── rustius-terminal/       # Local PTY (portable-pty)
        └── src/
            ├── lib.rs
            └── pty.rs          # LocalTerminal: spawn, write, resize, close

```

### 1.3 Technology Stack

| Layer | Technology | Version |
|---|---|---|
| Desktop Shell | Tauri | v2 |
| Backend Language | Rust | Edition 2021 |
| Async Runtime | Tokio | 1.x (full features) |
| SSH | russh | 0.63 |
| SFTP | russh-sftp | 3.0 |
| RDP | IronRDP | 0.17 |
| Local Terminal | portable-pty | 0.9 |
| Frontend Framework | Svelte | 5.x (runes) |
| Meta-framework | SvelteKit | 2.x (adapter-static, SPA) |
| Build Tool | Vite | 8.x |
| Terminal Emulator | @xterm/xterm | 6.0 |
| Icons | Lucide (Svelte) | latest |
| Serialization | serde + serde_json + toml | latest |
| Error Handling | anyhow + thiserror | latest |
| Logging | tracing + tracing-subscriber | latest |

### 1.4 IPC Communication Pattern

The Rust backend exposes **Tauri commands** (`#[tauri::command]`) invoked from the frontend via `invoke()`. Long-lived sessions (SSH, RDP, PTY) stream data back to the frontend using **Tauri events** (`app.emit()`).

**Request/Response (Commands):**
```
Frontend ──invoke("ssh_connect", {tabId, connectionId})──► Rust
Frontend ◄───────────── Ok(()) / Err(String) ──────────── Rust
```

**Streaming (Events):**
```
Rust ──emit("terminal-data-{tabId}", bytes)──► Frontend (listener)
Rust ──emit("terminal-exit-{tabId}", code)──► Frontend (listener)
Rust ──emit("rdp-frame-{tabId}", frame)────► Frontend (listener)
```

### 1.5 State Management

**Backend (Rust):** `AppState` holds concurrent session maps using `Arc<RwLock<HashMap<String, Arc<T>>>>` for each session type (SSH, SFTP, RDP, local PTY). The `ConfigManager` uses `Arc<RwLock<RustiusConfig>>` for thread-safe config access.

**Frontend (Svelte 5):** A class-based `AppStore` using Svelte 5 runes (`$state`) is instantiated as a module-level singleton. It manages reactive config, tabs, modal states, and delegates persistence to the Rust backend via `invoke()`.

---

## 2. Rust Best Practices

*Based on [Microsoft Pragmatic Rust Guidelines](https://microsoft.github.io/rust-guidelines/) and this project's patterns.*

### 2.1 Error Handling

- **Use `thiserror` for library crate errors** — every crate in `crates/` should define a structured enum error type. See `rustius-core/src/error.rs` for the pattern.
- **Use `anyhow` for application-level code** — Tauri commands and top-level orchestration can use `anyhow::Result` for ergonomic error chaining with `.context()`.
- **Never use `.unwrap()` or `.expect()` in production paths.** These are only acceptable in tests or provably-infallible contexts. Use `?` propagation everywhere else.
- **Tauri commands return `Result<T, String>`** — convert internal errors to user-facing strings at the command boundary: `.map_err(|e| e.to_string())?`.
- **Attach context to errors** — use `anyhow::Context` to annotate failures with what operation was being attempted:
  ```rust
  let handle = russh::client::connect(config, addr, handler)
      .await
      .context(format!("Failed to connect to {}", addr))?;
  ```

### 2.2 Async & Concurrency

- **All Tauri commands are `async`** — never block the Tauri main thread. Offload long-running work with `tokio::spawn`.
- **Use `tokio::sync` primitives** (`RwLock`, `Mutex`, `mpsc`) for async-friendly synchronization. Prefer `RwLock` over `Mutex` when reads vastly outnumber writes.
- **Minimize lock hold times** — clone data out of the lock and drop it before performing I/O:
  ```rust
  let session = {
      let sessions = state.ssh_sessions.read().await;
      sessions.get(&tab_id).cloned()
  };
  // Lock is released here; now operate on the cloned Arc
  ```
- **Use `tokio::select!`** for multiplexing — see `ssh.rs` for the pattern of handling output data, exit signals, and channel closure simultaneously.
- **Use `std::sync::Mutex` (not `tokio::sync::Mutex`) for blocking code** — the PTY module uses `std::sync::Mutex` because `portable-pty` is not async. This is correct and intentional.

### 2.3 Memory & Ownership

- **Wrap shared session objects in `Arc`** — all active sessions (SSH, SFTP, RDP, PTY) are stored as `Arc<T>` so they can be cloned cheaply and shared between the session map and spawned tasks.
- **Implement `Drop` for cleanup** — `RdpSession` and `LocalTerminal` implement `Drop` to ensure resources are released even if the caller forgets to disconnect explicitly.
- **Avoid unnecessary cloning** — use references (`&str`, `&[u8]`) in function signatures where ownership transfer isn't needed. Clone only at boundaries.
- **Prefer `String::from()` or `.into()` over `.to_string()`** for type conversions where `From` is implemented.

### 2.4 Type Design

- **Use newtypes and enums liberally** — `ConnectionType`, `AuthMethod`, and `TreeNode` are tagged enums with serde discriminators. This is the correct pattern.
- **Derive standard traits** — always derive `Debug`, `Clone`, `Serialize`, `Deserialize` on data types. Add `PartialEq` when equality comparison is needed.
- **Use `#[serde(tag = "type", rename_all = "lowercase")]`** for tagged enum serialization that maps cleanly to TypeScript discriminated unions.
- **Use `#[serde(default)]`** on optional fields and `#[serde(alias = "...")]` for backward-compatible field renames.
- **Keep types in sync** — the Rust types in `connection.rs` and TypeScript types in `src/lib/types/index.ts` must mirror each other. When adding a field to a Rust struct, add it to the TypeScript interface too.

### 2.5 Workspace & Crate Organization

- **Workspace-level dependency management** — all shared dependencies are declared in the root `Cargo.toml` under `[workspace.dependencies]` and referenced in member crates with `{ workspace = true }`.
- **Separation of concerns** — each crate has a single responsibility:
  - `rustius-core`: data models, config, storage (no protocol logic)
  - `rustius-ssh`: SSH + SFTP protocol clients
  - `rustius-rdp`: RDP protocol client
  - `rustius-terminal`: local PTY management
  - `src-tauri`: Tauri app shell, commands, state — orchestrates the crates
- **New protocol support** → create a new crate under `crates/`, add it to the workspace, and wire it through `src-tauri/commands/`.

### 2.6 Build Profiles

The workspace defines optimized build profiles — **do not modify without reason**:

- **Dev profile**: `opt-level = 0`, `debug = 1` (line tables only), `codegen-units = 256` for maximum compile speed. External deps (`[profile.dev.package."*"]`) are built at `opt-level = 1` so crypto/SSH runs at reasonable speed.
- **Release profile**: `opt-level = 3`, `lto = "thin"`, `panic = "abort"`, `strip = true` for minimal binary size and maximum performance.

### 2.7 Logging & Tracing

- **Use `tracing` macros** — `info!`, `debug!`, `warn!`, `error!` — not `println!` or `eprintln!`.
- **Use structured fields** in log messages: `info!("SSH session for tab: {}", tab_id)`.
- **Default filter** is `info,rustius=debug` — workspace crates log at `debug` level, dependencies at `info`.
- **Log lifecycle events** — always log connect, disconnect, and error events for every session type.

### 2.8 Testing

- **Unit tests go in the same file** as the code under `#[cfg(test)] mod tests { ... }`.
- **Use `#[tokio::test]`** for async tests.
- **Test data isolation** — use unique temp directories per test (see `ConfigManager` tests using `Uuid::new_v4()` for test dir names).
- **Clean up after tests** — remove temp directories in tests. Use `let _ = std::fs::remove_dir_all(&dir);` at the end.
- **Run all tests**: `cargo test --workspace`.

### 2.9 Safety & Security

- **Never store passwords in plaintext in production** — `PlaintextStorage` is the current implementation; plan for encrypted storage (e.g., OS keychain integration).
- **Server key verification** — `check_server_key` currently returns `Ok(true)` unconditionally (TOFU). This must be improved with known_hosts support before production.
- **Sanitize user input** — connection names, hostnames, and paths received from the frontend should be validated before use.
- **Capability permissions** — keep the Tauri capability set minimal. Only expose IPC commands the frontend actually needs.

---

## 3. Svelte 5 Best Practices

*Based on [Svelte Best Practices](https://svelte.dev/docs/svelte/best-practices) and this project's patterns.*

### 3.1 Runes & Reactivity

- **Only use `$state` for reactive variables** — variables that drive UI updates. Everything else should be a normal `let`.
- **Use `$state.raw` for large, reassign-only objects** — API responses from the backend (e.g., file listings from SFTP) should use `$state.raw` to avoid deep proxy overhead.
- **Use `$derived` instead of `$effect` for computed values** — never compute derived state inside an `$effect`. Use `$derived` or `$derived.by` for complex derivations.
- **Treat props as immutable** — values derived from `$props()` should use `$derived` to stay in sync when props change.
- **Effects are an escape hatch** — `$effect` should only be used for DOM side effects (xterm initialization, Canvas rendering, event listener setup). Never update `$state` inside an `$effect` if `$derived` can do the job.

### 3.2 Component Architecture

- **One component per file** — each `.svelte` file should have a single, focused responsibility.
- **Props with `$props()`** — use destructured props with TypeScript types:
  ```svelte
  <script lang="ts">
    let { tabId, connectionId }: { tabId: string; connectionId: string } = $props();
  </script>
  ```
- **Event callbacks as props** — pass event handlers as props (functions) rather than using custom events:
  ```svelte
  let { onclose }: { onclose: () => void } = $props();
  ```
- **Use snippets (`{#snippet}`) over slots** — for passing rendered content to child components, prefer Svelte 5 snippets over legacy slots.

### 3.3 State Management Pattern

This project uses a **class-based singleton store** pattern (`AppStore` in `appState.svelte.ts`):

- All `$state` fields are class properties.
- Methods mutate state directly — no actions/reducers.
- The store delegates persistence to the Rust backend via `invoke()`.
- **Import the singleton**: `import { appState } from '$lib/stores/appState.svelte';`
- When adding new global state, add it as a `$state` property on `AppStore`.

### 3.4 Resource Cleanup with DisposableStore

Every component that creates event listeners, observers, xterm instances, or Tauri event subscriptions **must** use the `DisposableStore` pattern from `$lib/utils/disposable.ts`:

```svelte
<script lang="ts">
  import { DisposableStore } from '$lib/utils/disposable';
  import { onDestroy } from 'svelte';

  const disposables = new DisposableStore();

  // Track a Tauri event listener
  disposables.add(listen('terminal-data-' + tabId, handler));

  // Track a DOM event listener
  disposables.trackEventListener(window, 'resize', onResize);

  onDestroy(() => disposables.dispose());
</script>
```

**Never leave dangling listeners.** Every `addEventListener`, `listen()`, `ResizeObserver`, or `MutationObserver` must be tracked and disposed.

### 3.5 TypeScript Integration

- **All components use `<script lang="ts">`** — no exceptions.
- **Shared types live in `$lib/types/index.ts`** — import from there, never redeclare locally.
- **TypeScript types must mirror Rust types** — when a Rust struct changes, update the TypeScript interface. Key pairs:
  - `connection.rs::Connection` ↔ `index.ts::Connection`
  - `connection.rs::AuthMethod` ↔ `index.ts::AuthMethod`
  - `connection.rs::TreeNode` ↔ `index.ts::TreeNode`
  - `connection.rs::RustiusConfig` ↔ `index.ts::RustiusConfig`

### 3.6 Styling

- **Use vanilla CSS** — no Tailwind. Styles are scoped per-component in `<style>` blocks.
- **Global styles and CSS variables** live in `src/app.css`.
- **Follow the Geist design system** documented in `DESIGN.md`:
  - Canvas background: `#fafafa`, elevated surfaces: `#ffffff`
  - Ink text: `#171717`, body text: `#4d4d4d`, mute: `#8f8f8f`
  - Hairline borders: `#ebebeb`
  - Font: Geist Sans / JetBrains Mono, tight negative tracking on headings
  - Border radius: `6px` for controls, `12px` for cards, `100px` for pill buttons
- **Use CSS custom properties** for theming (dark/light mode).
- **Style child components** via CSS custom properties, not `:global()`.

### 3.7 Tauri IPC from Svelte

- **Always wrap `invoke()` in try/catch** — treat backend calls like network requests:
  ```typescript
  try {
    await invoke('ssh_connect', { tabId, connectionId });
  } catch (err) {
    console.error('SSH connect failed:', err);
    // Update tab status to 'error'
  }
  ```
- **Use `listen()` for streaming events** — always unlisten in cleanup:
  ```typescript
  const unlisten = await listen('terminal-data-' + tabId, (event) => {
    terminal.write(new Uint8Array(event.payload));
  });
  disposables.add(unlisten);
  ```
- **Tab ID as session key** — every session is identified by its `tabId` on both sides. Use this consistently.

### 3.8 Avoid Legacy Svelte Features

This project uses **Svelte 5**. Do not use:
- ❌ `on:click` directive → ✅ `onclick` attribute
- ❌ `export let` → ✅ `$props()`
- ❌ `$:` reactive statements → ✅ `$derived` / `$derived.by`
- ❌ `<slot>` → ✅ `{#snippet}` / `{@render}`
- ❌ `createEventDispatcher` → ✅ callback props
- ❌ `writable()` / `readable()` stores → ✅ `$state` in `.svelte.ts` files
- ❌ `afterUpdate` / `beforeUpdate` → ✅ `$effect`

---

## 4. Tauri v2 Guidelines

### 4.1 Command Design

- **Commands are thin orchestration** — they retrieve state, call into library crates, and return results. Business logic belongs in `crates/`, not in `src-tauri/commands/`.
- **Every command returns `Result<T, String>`** — Tauri requires `String` errors for IPC serialization.
- **Session lifecycle pattern** — connect commands store sessions in `AppState`, disconnect commands remove them:
  ```rust
  // Connect: insert into map
  sessions.insert(tab_id.clone(), Arc::new(session));

  // Disconnect: remove from map
  let session = sessions.remove(&tab_id);
  if let Some(s) = session { s.disconnect().await; }
  ```
- **Handle old sessions gracefully** — when connecting, check for and disconnect any existing session on the same tab ID.

### 4.2 Event Streaming

- **Use dynamic event names** — format with tab ID: `format!("terminal-data-{}", tab_id)`.
- **Spawn background tasks for streaming** — use `tokio::spawn` to forward output from session channels to the frontend via `app.emit()`.
- **Drain buffered data on exit** — when a session exits, drain any remaining buffered output before emitting the exit event (see `ssh.rs`).

### 4.3 Configuration

- **`tauri.conf.json`** — keep the window config, CSP, and bundle settings here. Current CSP is `null` (permissive) — tighten for production.
- **Capabilities** — define minimal IPC permissions in `src-tauri/capabilities/`.
- **SPA mode** — the frontend is built with `adapter-static` and `fallback: "index.html"`. No SSR.

---

## 5. Development Workflow

### 5.1 Commands

```bash
# Development (starts Vite + Tauri simultaneously)
cargo tauri dev

# Build release binary
cargo tauri build

# Run Rust tests
cargo test --workspace

# Svelte type checking
npm run check

# Frontend only (no Tauri)
npm run dev
```

### 5.2 Adding a New Feature

1. **Data model** → Add types to `rustius-core/src/connection.rs` + `src/lib/types/index.ts`
2. **Protocol logic** → Implement in the appropriate `crates/rustius-*` crate
3. **Tauri command** → Add to `src-tauri/src/commands/`, register in `lib.rs`
4. **Frontend component** → Create in `src/lib/components/`, wire to the store
5. **State** → Add session map to `AppState` if needed, add store state to `AppStore`

### 5.3 Adding a New Protocol

1. Create `crates/rustius-<protocol>/` with `Cargo.toml` and `src/lib.rs`
2. Add to `[workspace.members]` in root `Cargo.toml`
3. Implement session struct with `connect`, `disconnect`, and I/O methods
4. Add session map to `AppState` (`Arc<RwLock<HashMap<String, Arc<Session>>>>`)
5. Create commands in `src-tauri/src/commands/<protocol>.rs`
6. Register commands in `src-tauri/src/lib.rs`
7. Add frontend component and tab type

---

## 6. Code Style & Conventions

### 6.1 Rust

- **Formatting**: `rustfmt` defaults. Run `cargo fmt` before committing.
- **Linting**: `cargo clippy --workspace` must pass cleanly.
- **Naming**: `snake_case` for functions/variables, `PascalCase` for types, `SCREAMING_SNAKE_CASE` for constants.
- **Documentation**: `///` doc comments on all public items. Use `//` inline comments sparingly, only for non-obvious logic.
- **Imports**: group by (1) `std`, (2) external crates, (3) workspace crates, (4) `crate`/`super` — separated by blank lines.
- **Module structure**: prefer `mod.rs` for directories, inline modules for small groupings.
- **Prefer `impl` blocks over free functions** — attach methods to the relevant struct/enum.
- **Use `tracing` structured logging** — not `println!`.

### 6.2 Svelte / TypeScript

- **File naming**: `PascalCase.svelte` for components, `camelCase.svelte.ts` for stores, `camelCase.ts` for utilities.
- **Component script order**: `<script lang="ts">` → template → `<style>`.
- **Prefer `const` over `let`** for non-reactive variables.
- **Use template literals** over string concatenation.
- **Event handler naming**: `on` prefix — `onclick`, `onkeydown`, `onclose`.
- **CSS**: scoped styles per component. Use CSS variables for values shared with the design system.

### 6.3 Git

- **Commit messages**: imperative mood, present tense. E.g., "Add SFTP rename support", not "Added SFTP rename support".
- **One logical change per commit** — separate refactors from features.

---

## 7. Design System Reference

The UI follows a **Vercel Geist-inspired** design language documented in `DESIGN.md`. Key tokens:

### Colors (Dark Theme Active)

| Token | Value | Use |
|---|---|---|
| `ink` | `#171717` | Headings, primary CTAs, borders |
| `body` | `#4d4d4d` | Body text, nav links |
| `mute` | `#8f8f8f` | Captions, metadata |
| `faint` | `#a1a1a1` | Placeholders, disabled |
| `canvas` | `#fafafa` | Page background |
| `canvas-elevated` | `#ffffff` | Cards, inputs, panels |
| `hairline` | `#ebebeb` | 1px borders, dividers |
| `link` | `#0070f3` | Links, active states |
| `error` | `#ee0000` | Validation errors |

### Typography

| Use | Family | Size | Weight | Tracking |
|---|---|---|---|---|
| Display headline | Geist Sans | 48px | 600 | -2.4px |
| Section heading | Geist Sans | 32px | 600 | -1.28px |
| Card heading | Geist Sans | 20px | 600 | -0.4px |
| Body text | Geist Sans | 14px | 400 | 0 |
| Code / Terminal | Geist Mono / JetBrains Mono | 14px | 400 | 0 |
| Mono eyebrow | Geist Mono | 12px | 500 | 0 |

### Border Radius

| Use | Value |
|---|---|
| Controls, inputs | `6px` |
| Cards | `12px` |
| Large panels | `16px` |
| Pill buttons (CTA) | `100px` |
| Circular buttons | `9999px` |

### Elevation

- **Level 0 (Flat)**: 1px hairline border, no shadow — default for all cards and inputs.
- **Level 1 (Whisper)**: hairline + `0px 1px 1px rgba(0,0,0,0.04)` — slightly raised cards.
- **Level 2 (Floating)**: layered soft shadow — menus, modals, tooltips.

### Principles

- Near-zero chromatic chrome: ink on white, one gradient for decoration.
- Depth via 1px hairlines, not heavy shadows.
- Two button shapes: pills for CTAs, 6px squares for app controls.
- Tight negative letter-spacing on display headings.
- Consistent 4px-based spacing scale.

---

## 8. Common Patterns & Recipes

### 8.1 Session Lookup in Tauri Commands

```rust
// Clone the Arc out of the lock, then operate on it outside the lock scope
let session = {
    let sessions = state.ssh_sessions.read().await;
    sessions.get(&tab_id).cloned()
};

if let Some(s) = session {
    s.write(&data).await.map_err(|e| e.to_string())?;
    Ok(())
} else {
    Err(format!("Session not found for tab: {}", tab_id))
}
```

### 8.2 Spawning a Background Stream

```rust
let data_event = format!("terminal-data-{}", tab_id);
let exit_event = format!("terminal-exit-{}", tab_id);
let app_handle = app.clone();

tokio::spawn(async move {
    loop {
        tokio::select! {
            Some(bytes) = output_rx.recv() => {
                if let Err(e) = app_handle.emit(&data_event, bytes) {
                    error!("Failed to emit: {:?}", e);
                    break;
                }
            }
            Some(code) = exit_rx.recv() => {
                let _ = app_handle.emit(&exit_event, code);
                break;
            }
            else => break,
        }
    }
});
```

### 8.3 Svelte Component with Cleanup

```svelte
<script lang="ts">
  import { DisposableStore } from '$lib/utils/disposable';
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { onDestroy, onMount } from 'svelte';

  let { tabId, connectionId }: { tabId: string; connectionId: string } = $props();
  const disposables = new DisposableStore();

  onMount(async () => {
    const unlisten = await listen(`terminal-data-${tabId}`, (e) => { /* ... */ });
    disposables.add(unlisten);

    await invoke('ssh_connect', { tabId, connectionId });
  });

  onDestroy(() => disposables.dispose());
</script>
```

### 8.4 Adding a New Tauri Command

**Rust side:**
```rust
// In src-tauri/src/commands/my_feature.rs
#[tauri::command]
pub async fn my_command(
    tab_id: String,
    state: State<'_, AppState>,
) -> Result<MyResponse, String> {
    // Implementation
}
```

**Register in `lib.rs`:**
```rust
.invoke_handler(tauri::generate_handler![
    // ... existing commands ...
    my_command,
])
```

**Frontend call:**
```typescript
const result = await invoke<MyResponse>('my_command', { tabId });
```

---

## 9. Known Limitations & TODOs

- **Server key verification** — SSH `check_server_key` accepts all keys (no known_hosts). Needs TOFU implementation.
- **SSH agent auth** — `AuthMethod::Agent` returns a "not yet implemented" error.
- **Credential storage** — currently `PlaintextStorage`. Plan for OS keychain (e.g., `keyring` crate).
- **RDP** — X.224 negotiation is implemented; full MCS/RDPDR/graphics pipeline is in progress using IronRDP.
- **CSP** — `tauri.conf.json` has `"csp": null`. Tighten before production release.
- **Cross-platform testing** — local PTY uses `powershell.exe` on Windows, `$SHELL` on Unix. Ensure both paths are tested.
