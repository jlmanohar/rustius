<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { appState } from '../stores/appState.svelte';
  import { DisposableStore } from '../utils/disposable';
  import { RotateCcw, X, FolderTree } from '@lucide/svelte';
  import SftpSidebar from './SftpSidebar.svelte';
  import '@xterm/xterm/css/xterm.css';
  import type { TabSession } from '../types';

  let { session = $bindable() }: { session: TabSession } = $props();

  let shellElement: HTMLDivElement | null = $state(null);
  let terminalContainer: HTMLDivElement | null = $state(null);
  let term = $state<Terminal | null>(null);
  let fitAddon = $state<FitAddon | null>(null);
  let isConnecting = false;
  let isTauriLive = false;
  let isDisconnected = $state(false);

  // MobaXterm-style SFTP side file navigator state
  let isSftpOpen = $state(true);
  let sftpWidth = $state(260);
  let isDraggingSplitter = $state(false);
  let terminalCwd = $state<string | null>(null);

  // Helper to extract working directory from OSC 7 escape sequence (e.g. file://hostname/path)
  function extractCwdFromOsc7(data: string): string | null {
    if (!data) return null;
    const raw = data.trim();
    if (raw.startsWith('file://')) {
      try {
        const url = new URL(raw);
        return decodeURIComponent(url.pathname);
      } catch {
        const match = /^file:\/\/[^/]*(\/.*)$/.exec(raw);
        if (match) return decodeURIComponent(match[1]);
      }
    } else if (raw.startsWith('/') || raw.startsWith('~')) {
      return raw;
    }
    return null;
  }

  // Helper to extract working directory from terminal title (OSC 0 / OSC 2 - default in Linux distributions)
  function extractCwdFromTitle(title: string): string | null {
    if (!title) return null;
    const t = title.trim();

    // Pattern 1: user@host: /path or user@host:/path or user@host: ~ or user@host:~
    const userHostMatch = /^(?:[a-zA-Z0-9_.+-]+@)?[a-zA-Z0-9_.+-]+:\s*([/~].*)$/.exec(t);
    if (userHostMatch && userHostMatch[1]) {
      return userHostMatch[1].trim();
    }

    // Pattern 2: [user@host /path] or [user@host: /path] or (user@host)-[/path]
    const bracketMatch = /^[\[(](?:[a-zA-Z0-9_.+-]+@)?[a-zA-Z0-9_.+-]+:?\s+([/~].*?)[\])]$/.exec(t);
    if (bracketMatch && bracketMatch[1]) {
      return bracketMatch[1].trim();
    }

    // Pattern 3: user@host - /path or host - /path
    const dashMatch = /^(?:[a-zA-Z0-9_.+-]+@)?[a-zA-Z0-9_.+-]+\s*[-–—]\s*([/~].*)$/.exec(t);
    if (dashMatch && dashMatch[1]) {
      return dashMatch[1].trim();
    }

    // Pattern 4: Terminal title is simply the directory itself: "/var/log" or "~" or "~/projects"
    if (t.startsWith('/') || t.startsWith('~')) {
      if (!t.includes(' ') && !t.includes('\n')) {
        return t;
      }
    }

    return null;
  }

  function toggleSftp() {
    isSftpOpen = !isSftpOpen;
    setTimeout(() => {
      fitAddon?.fit();
    }, 50);
  }

  function startResizeSplitter(e: MouseEvent) {
    e.preventDefault();
    isDraggingSplitter = true;

    const onMouseMove = (moveEvent: MouseEvent) => {
      if (!shellElement) return;
      const rect = shellElement.getBoundingClientRect();
      const newWidth = moveEvent.clientX - rect.left;
      sftpWidth = Math.max(180, Math.min(500, newWidth));
      fitAddon?.fit();
    };

    const onMouseUp = () => {
      isDraggingSplitter = false;
      window.removeEventListener('mousemove', onMouseMove);
      window.removeEventListener('mouseup', onMouseUp);
      setTimeout(() => fitAddon?.fit(), 20);
    };

    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', onMouseUp);
  }

  // Disposable stores:
  // componentDisposables manages the lifetime of DOM listeners, observers, and the Terminal instance.
  // sessionDisposables manages the lifetime of data streams (Tauri data events, onData handlers) for a single connection attempt.
  const componentDisposables = new DisposableStore();
  const sessionDisposables = new DisposableStore();
  let sessionGeneration = 0;

  $effect(() => {
    if (appState.activeTabId === session.id && term && !componentDisposables.isDisposed) {
      setTimeout(() => {
        if (componentDisposables.isDisposed) return;
        try {
          fitAddon?.fit();
          term?.refresh(0, term.rows - 1);
          term?.focus();
        } catch {}
      }, 30);
    }
  });

  async function startSession() {
    if (isConnecting || componentDisposables.isDisposed) return;
    isConnecting = true;
    const currentGen = ++sessionGeneration;

    try {
      isDisconnected = false;
      session.status = 'connecting';

      // Always clear any previous session listeners (xterm onData, tauri terminal-data)
      sessionDisposables.clear();

      if (session.type === 'local_terminal') {
        try {
          const unlisten = await listen<number[]>(`terminal-data-${session.id}`, (event) => {
            if (term && event.payload) {
              term.write(new Uint8Array(event.payload));
            }
          });

          // Check if session was superseded or component unmounted while awaiting listen
          if (currentGen !== sessionGeneration || componentDisposables.isDisposed) {
            unlisten();
            return;
          }
          sessionDisposables.add(unlisten);

          await invoke('pty_spawn', {
            tabId: session.id,
            cols: term?.cols || 80,
            rows: term?.rows || 24,
          });

          if (currentGen !== sessionGeneration || componentDisposables.isDisposed) {
            invoke('pty_close', { tabId: session.id }).catch(() => {});
            return;
          }

          isTauriLive = true;
          session.status = 'connected';

          const onDataDisposable = term?.onData((data) => {
            if (isDisconnected) return;
            const encoded = Array.from(new TextEncoder().encode(data));
            invoke('pty_write', { tabId: session.id, data: encoded }).catch((err) => {
              console.error('Failed to write to PTY:', err);
            });
          });
          if (onDataDisposable) {
            sessionDisposables.add(onDataDisposable);
          }
        } catch (e) {
          console.warn('Tauri PTY unavailable (running in web preview mode):', e);
          setupFallbackEcho();
        }
      } else if (session.type === 'ssh' && session.connectionId) {
        try {
          term?.write(`\x1b[90mConnecting to remote SSH host...\x1b[0m\r`);

          const unlisten = await listen<number[]>(`terminal-data-${session.id}`, (event) => {
            if (term && event.payload) {
              term.write(new Uint8Array(event.payload));
            }
          });

          if (currentGen !== sessionGeneration || componentDisposables.isDisposed) {
            unlisten();
            return;
          }
          sessionDisposables.add(unlisten);

          await invoke('ssh_connect', {
            tabId: session.id,
            connectionId: session.connectionId,
          });

          if (currentGen !== sessionGeneration || componentDisposables.isDisposed) {
            invoke('ssh_disconnect', { tabId: session.id }).catch(() => {});
            return;
          }

          // Clear connecting line so remote prompt/banner begins cleanly
          term?.write('\x1b[2K\r');
          session.status = 'connected';
          isTauriLive = true;
          setTimeout(() => {
            try {
              fitAddon?.fit();
              term?.focus();
            } catch {}
          }, 30);

          const onDataDisposable = term?.onData((data) => {
            if (isDisconnected) return;
            const encoded = Array.from(new TextEncoder().encode(data));
            invoke('ssh_write', { tabId: session.id, data: encoded }).catch((err) => {
              console.error('Failed to write to SSH session:', err);
              handleSessionExit(255);
            });
          });
          if (onDataDisposable) {
            sessionDisposables.add(onDataDisposable);
          }
        } catch (e: any) {
          console.warn('SSH connection failed or running in web preview mode:', e);
          const isTauri = typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
          if (term) {
            term.write('\x1b[2K\r');
            term.writeln(`\x1b[31;1mConnection failed:\x1b[0m \x1b[90m${e?.message || e}\x1b[0m\r\n`);
          }
          if (!isTauri) {
            if (term) {
              term.writeln(`\x1b[90mFalling back to demo terminal emulator:\x1b[0m\r\n`);
            }
            setupFallbackEcho();
          } else {
            handleSessionExit(1);
          }
        }
      } else {
        setupFallbackEcho();
      }
    } finally {
      isConnecting = false;
    }
  }

  function handleSessionExit(exitCode: number = 0) {
    if (isDisconnected) return;
    isDisconnected = true;
    isTauriLive = false;
    session.status = exitCode === 0 ? 'disconnected' : 'error';

    if (appState.config.app_settings?.close_tab_on_exit) {
      appState.closeTab(session.id);
      return;
    }

    if (term) {
      term.writeln(`\r\n\x1b[90m──────────────────────────────────────────────\x1b[0m`);
      if (exitCode === 255) {
        term.writeln(`\x1b[31;1m[Connection lost]\x1b[0m \x1b[90mRemote host terminated the connection or is offline.\x1b[0m`);
      } else {
        term.writeln(`\x1b[33;1m[Session closed]\x1b[0m \x1b[90mConnection closed (exit code: ${exitCode}).\x1b[0m`);
      }
      term.writeln(`\x1b[90mPress \x1b[37;1m[Enter]\x1b[0m\x1b[90m or click \x1b[37;1mReconnect\x1b[0m\x1b[90m below to restart session.\x1b[0m\r\n`);
    }
  }

  async function reconnect() {
    if (!term || isConnecting || componentDisposables.isDisposed) return;
    isDisconnected = false;
    session.status = 'connecting';
    term.reset();
    await startSession();
  }

  onMount(async () => {
    if (!terminalContainer) return;

    term = new Terminal({
      cursorBlink: true,
      cursorStyle: 'bar',
      fontSize: 13,
      fontFamily: "'Geist Mono', 'JetBrains Mono', monospace",
      lineHeight: 1.25,
      theme: {
        background: '#000000',
        foreground: '#ededed',
        cursor: '#ffffff',
        cursorAccent: '#000000',
        selectionBackground: 'rgba(255, 255, 255, 0.25)',
        black: '#000000',
        red: '#ee0000',
        green: '#0070f3',
        yellow: '#f5a623',
        blue: '#0070f3',
        magenta: '#7928ca',
        cyan: '#50e3c2',
        white: '#ededed',
        brightBlack: '#707070',
        brightRed: '#ff4d4d',
        brightGreen: '#50e3c2',
        brightYellow: '#f9cb28',
        brightBlue: '#00dfd8',
        brightMagenta: '#eb367f',
        brightCyan: '#79ffe1',
        brightWhite: '#ffffff',
      },
      allowTransparency: false,
      scrollback: 5000,
    });
    componentDisposables.add(term);

    fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    componentDisposables.add(fitAddon);

    term.open(terminalContainer);
    fitAddon.fit();
    setTimeout(() => {
      try {
        term?.focus();
      } catch {}
    }, 30);

    // Custom keyboard handler for copy / paste and Enter-to-reconnect
    term.attachCustomKeyEventHandler((event: KeyboardEvent) => {
      // Enter key when disconnected -> Reconnect
      if (isDisconnected && event.key === 'Enter') {
        if (event.type === 'keydown') {
          reconnect();
        }
        return false;
      }

      // Ctrl+C with text selected -> Copy to clipboard (prevent SIGINT)
      if (event.ctrlKey && (event.key === 'c' || event.key === 'C') && term?.hasSelection()) {
        if (event.type === 'keydown') {
          const selected = term.getSelection();
          if (selected) {
            navigator.clipboard.writeText(selected).catch((err) => {
              console.error('Failed to copy to clipboard:', err);
            });
          }
        }
        return false;
      }

      // Ctrl+Shift+C -> Copy to clipboard
      if (event.ctrlKey && event.shiftKey && (event.key === 'c' || event.key === 'C')) {
        if (event.type === 'keydown' && term?.hasSelection()) {
          const selected = term.getSelection();
          if (selected) {
            navigator.clipboard.writeText(selected).catch(() => {});
          }
        }
        return false;
      }

      // Ctrl+V or Ctrl+Shift+V -> Paste from clipboard
      if (
        (event.ctrlKey && (event.key === 'v' || event.key === 'V')) ||
        (event.ctrlKey && event.shiftKey && (event.key === 'v' || event.key === 'V'))
      ) {
        if (event.type === 'keydown') {
          navigator.clipboard
            .readText()
            .then((text) => {
              if (text && term) {
                term.paste(text);
              }
            })
            .catch((err) => {
              console.error('Failed to paste from clipboard:', err);
            });
        }
        return false;
      }

      // Ctrl+Shift+F -> Toggle SFTP side file navigator
      if (event.ctrlKey && event.shiftKey && (event.key === 'f' || event.key === 'F')) {
        if (event.type === 'keydown' && session.type === 'ssh') {
          toggleSftp();
        }
        return false;
      }

      return true;
    });

    // Right-click support: copy if text is selected, or paste if nothing selected
    const handleContextMenu = async (e: MouseEvent) => {
      e.preventDefault();
      if (term?.hasSelection()) {
        const text = term.getSelection();
        if (text) {
          try {
            await navigator.clipboard.writeText(text);
          } catch (err) {
            console.error('Copy on right-click failed:', err);
          }
          term.clearSelection();
        }
      } else {
        try {
          const text = await navigator.clipboard.readText();
          if (text && term) {
            term.paste(text);
          }
        } catch (err) {
          console.error('Paste on right-click failed:', err);
        }
      }
    };

    // Track DOM listener with auto-cleanup
    componentDisposables.trackEventListener(terminalContainer, 'contextmenu', handleContextMenu);

    // Track working directory via OSC 7 (modern shells) and Title changes (default in Linux distributions)
    try {
      const osc7Disposable = term.parser.registerOscHandler(7, (data: string) => {
        try {
          const path = extractCwdFromOsc7(data);
          if (path && path !== terminalCwd) {
            terminalCwd = path;
          }
        } catch {}
        return true;
      });
      componentDisposables.add(osc7Disposable);
    } catch (e) {
      console.warn('Failed to register OSC 7 handler:', e);
    }

    const titleDisposable = term.onTitleChange((title: string) => {
      try {
        const path = extractCwdFromTitle(title);
        if (path && path !== terminalCwd) {
          terminalCwd = path;
        }
      } catch {}
    });
    componentDisposables.add(titleDisposable);

    // Listen for terminal exit event from Rust backend
    const unlistenExit = await listen<number>(`terminal-exit-${session.id}`, (event) => {
      handleSessionExit(event.payload ?? 0);
    });
    if (componentDisposables.isDisposed) {
      unlistenExit();
      return;
    }
    componentDisposables.add(unlistenExit);

    await startSession();

    setTimeout(() => {
      try {
        fitAddon?.fit();
        term?.focus();
      } catch {}
    }, 50);

    // Resize observer to keep xterm fitting parent container
    const resizeObserver = new ResizeObserver((entries) => {
      try {
        const entry = entries[0];
        // Ignore collapsed/zero dimensions when hidden or transitioning
        if (!entry || entry.contentRect.width < 50 || entry.contentRect.height < 50) {
          return;
        }
        fitAddon?.fit();
        if (isTauriLive && term && term.cols >= 10 && term.rows >= 2) {
          const command = session.type === 'ssh' ? 'ssh_resize' : 'pty_resize';
          invoke(command, {
            tabId: session.id,
            cols: term.cols,
            rows: term.rows,
          }).catch(() => {});
        }
      } catch {}
    });
    resizeObserver.observe(terminalContainer);
    componentDisposables.add(resizeObserver);
  });

  function setupFallbackEcho() {
    if (!term || componentDisposables.isDisposed) return;
    sessionDisposables.clear();

    term.writeln(`\x1b[90m┌─ \x1b[37;1mrustius\x1b[0m \x1b[90m${session.title} (${session.badge || session.type})\x1b[0m`);
    term.writeln(`\x1b[90m└─ Status: \x1b[32mconnected (web preview mode)\x1b[0m\r\n`);
    term.write('\x1b[90mps\x1b[0m \x1b[37m>\x1b[0m ');

    const onData = term.onData((data) => {
      if (data === '\r') {
        term?.write('\r\n\x1b[90mps\x1b[0m \x1b[37m>\x1b[0m ');
      } else if (data === '\u007f') {
        term?.write('\b \b');
      } else {
        term?.write(data);
      }
    });
    sessionDisposables.add(onData);
  }

  onDestroy(() => {
    // 1. Dispose all session-level listeners and subscriptions (data stream, onData)
    sessionDisposables.dispose();

    // 2. Dispose all component-level listeners, observers, DOM events, and terminal instance
    componentDisposables.dispose();

    // 3. Gracefully terminate backend process if still running
    if (isTauriLive) {
      const closeCmd = session.type === 'ssh' ? 'ssh_disconnect' : 'pty_close';
      invoke(closeCmd, { tabId: session.id }).catch(() => {});
      if (session.type === 'ssh') {
        invoke('sftp_disconnect', { tabId: session.id }).catch(() => {});
      }
    }
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions a11y_no_static_element_interactions -->
<div
  class="terminal-shell"
  role="presentation"
  bind:this={shellElement}
  onclick={(e) => {
    const target = e.target as HTMLElement | null;
    if (target && !target.closest('.sftp-sidebar-pane') && !target.closest('.terminal-topbar')) {
      term?.focus();
    }
  }}
>
  <!-- Top Utility Bar for SSH sessions with MobaXterm-style SFTP toggle -->
  {#if session.type === 'ssh'}
    <div class="terminal-topbar">
      <div class="topbar-left">
        <button
          class="sftp-toggle-btn {isSftpOpen ? 'active' : ''}"
          onclick={toggleSftp}
          title="Toggle SFTP Side File Navigator (Ctrl+Shift+F)"
        >
          <FolderTree size={12} />
          <span>SFTP Browser</span>
        </button>
      </div>

      {#if isDisconnected}
        <div class="disconnect-inline">
          <span class="status-indicator"></span>
          <span class="disconnect-text">Session closed</span>
          <button class="reconnect-btn small" onclick={reconnect}>
            <RotateCcw size={11} />
            <span>Reconnect</span>
          </button>
          <button class="close-btn small" onclick={() => appState.closeTab(session.id)}>
            <X size={11} />
            <span>Close</span>
          </button>
        </div>
      {/if}
    </div>
  {:else if isDisconnected}
    <div class="disconnect-bar">
      <div class="disconnect-left">
        <span class="status-indicator"></span>
        <span class="disconnect-text">Session closed</span>
      </div>
      <div class="disconnect-right">
        <button class="reconnect-btn" onclick={reconnect}>
          <RotateCcw size={12} />
          <span>Reconnect</span>
        </button>
        <button class="close-btn" onclick={() => appState.closeTab(session.id)}>
          <X size={12} />
          <span>Close Tab</span>
        </button>
      </div>
    </div>
  {/if}

  <!-- Split Workspace containing SFTP navigator and terminal -->
  <div class="workspace-split">
    {#if session.type === 'ssh' && isSftpOpen && session.connectionId}
      <div class="sftp-sidebar-pane" style="width: {sftpWidth}px;">
        <SftpSidebar
          tabId={session.id}
          connectionId={session.connectionId}
          {terminalCwd}
          onclose={toggleSftp}
          onresize={() => fitAddon?.fit()}
        />
      </div>

      <!-- Resizable Splitter Handle -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div
        class="split-divider {isDraggingSplitter ? 'dragging' : ''}"
        onmousedown={startResizeSplitter}
        role="separator"
        aria-orientation="vertical"
        tabindex="-1"
      ></div>
    {/if}

    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions a11y_no_static_element_interactions -->
    <div
      class="terminal-body"
      role="presentation"
      bind:this={terminalContainer}
      onclick={() => term?.focus()}
      onmousedown={() => term?.focus()}
    ></div>
  </div>
</div>

<style>
  .terminal-shell {
    width: 100%;
    height: 100%;
    background: #000000;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .terminal-topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 3px 10px;
    background: #09090b;
    border-bottom: 1px solid #1f1f23;
    min-height: 26px;
    flex-shrink: 0;
  }

  .topbar-left {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .sftp-toggle-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 2px 8px;
    background: transparent;
    border: 1px solid var(--hairline, #262626);
    border-radius: var(--radius-sm, 6px);
    color: var(--body, #a1a1a1);
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.12s ease;
  }

  .sftp-toggle-btn:hover {
    background: var(--canvas-hover, #1a1a1a);
    color: var(--ink, #ffffff);
    border-color: var(--hairline-hover, #404040);
  }

  .sftp-toggle-btn.active {
    background: var(--canvas-elevated, #0f0f0f);
    border-color: var(--hairline-focus, #ffffff);
    color: var(--ink, #ffffff);
  }

  .disconnect-inline {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
  }

  .workspace-split {
    flex: 1;
    display: flex;
    width: 100%;
    height: 100%;
    min-height: 0;
    position: relative;
    overflow: hidden;
  }

  .sftp-sidebar-pane {
    height: 100%;
    flex-shrink: 0;
    overflow: hidden;
  }

  .split-divider {
    width: 4px;
    height: 100%;
    background: #18181b;
    border-left: 1px solid #27272a;
    cursor: col-resize;
    flex-shrink: 0;
    transition: background 0.12s ease;
    position: relative;
    z-index: 5;
  }

  .split-divider:hover,
  .split-divider.dragging {
    background: #3b82f6;
    border-left-color: #3b82f6;
  }

  .reconnect-btn.small {
    padding: 2px 7px;
    font-size: 10.5px;
  }

  .close-btn.small {
    padding: 2px 6px;
    font-size: 10.5px;
  }

  .disconnect-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 14px;
    background: #0a0a0a;
    border-bottom: 1px solid #262626;
    font-size: 12px;
    animation: fadeIn 0.15s ease-out;
  }

  @keyframes fadeIn {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .disconnect-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .status-indicator {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #e5484d;
  }

  .disconnect-text {
    font-family: var(--font-mono, monospace);
    color: #888888;
    letter-spacing: -0.01em;
  }

  .disconnect-right {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .reconnect-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: #ffffff;
    color: #000000;
    border: none;
    border-radius: 4px;
    padding: 4px 10px;
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    font-family: inherit;
    transition: opacity 0.15s ease;
  }

  .reconnect-btn:hover {
    opacity: 0.9;
  }

  .close-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: transparent;
    color: #888888;
    border: 1px solid #262626;
    border-radius: 4px;
    padding: 3px 8px;
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    font-family: inherit;
    transition: all 0.15s ease;
  }

  .close-btn:hover {
    color: #ededed;
    border-color: #404040;
    background: #141414;
  }

  .terminal-body {
    flex: 1;
    min-width: 0;
    width: 100%;
    height: 100%;
    padding: 10px 14px;
    background: #000000;
    user-select: text !important;
  }

  :global(.xterm) {
    height: 100%;
    user-select: text !important;
  }

  :global(.xterm-screen) {
    user-select: text !important;
  }

  :global(.xterm-rows) {
    user-select: text !important;
  }

  :global(.xterm-viewport) {
    background-color: #000000 !important;
  }
</style>
