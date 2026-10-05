<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { appState } from '../stores/appState.svelte';
  import { RotateCcw, X } from '@lucide/svelte';
  import '@xterm/xterm/css/xterm.css';
  import type { TabSession } from '../types';

  let { session = $bindable() }: { session: TabSession } = $props();

  let terminalContainer: HTMLDivElement | null = $state(null);
  let term: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let unlistenData: UnlistenFn | null = null;
  let unlistenExit: UnlistenFn | null = null;
  let onDataDisposable: { dispose(): void } | null = null;
  let isConnecting = false;
  let isTauriLive = false;
  let isDisconnected = $state(false);

  $effect(() => {
    if (appState.activeTabId === session.id && term) {
      setTimeout(() => {
        try {
          fitAddon?.fit();
          term?.focus();
        } catch {}
      }, 50);
    }
  });

  async function startSession() {
    if (isConnecting) return;
    isConnecting = true;
    try {
      isDisconnected = false;
      session.status = 'connected';

      if (unlistenData) {
        unlistenData();
        unlistenData = null;
      }
      if (onDataDisposable) {
        onDataDisposable.dispose();
        onDataDisposable = null;
      }

      if (session.type === 'local_terminal') {
        try {
          unlistenData = await listen<number[]>(`terminal-data-${session.id}`, (event) => {
            if (term && event.payload) {
              term.write(new Uint8Array(event.payload));
            }
          });

          await invoke('pty_spawn', {
            tabId: session.id,
            cols: term?.cols || 80,
            rows: term?.rows || 24,
          });

          isTauriLive = true;

          onDataDisposable = term?.onData((data) => {
            if (isDisconnected) return;
            const encoded = Array.from(new TextEncoder().encode(data));
            invoke('pty_write', { tabId: session.id, data: encoded }).catch((err) => {
              console.error('Failed to write to PTY:', err);
            });
          }) ?? null;
        } catch (e) {
          console.warn('Tauri PTY unavailable (running in web preview mode):', e);
          setupFallbackEcho();
        }
      } else if (session.type === 'ssh' && session.connectionId) {
        try {
          term?.writeln(`\x1b[90mConnecting to remote SSH host...\x1b[0m\r\n`);

          unlistenData = await listen<number[]>(`terminal-data-${session.id}`, (event) => {
            if (term && event.payload) {
              term.write(new Uint8Array(event.payload));
            }
          });

          await invoke('ssh_connect', {
            tabId: session.id,
            connectionId: session.connectionId,
          });

          isTauriLive = true;

          onDataDisposable = term?.onData((data) => {
            if (isDisconnected) return;
            const encoded = Array.from(new TextEncoder().encode(data));
            invoke('ssh_write', { tabId: session.id, data: encoded }).catch((err) => {
              console.error('Failed to write to SSH session:', err);
            });
          }) ?? null;
        } catch (e: any) {
          console.warn('SSH connection failed or running in web preview mode:', e);
          if (term) {
            term.writeln(`\x1b[31;1mConnection failed:\x1b[0m \x1b[90m${e?.message || e}\x1b[0m\r\n`);
          }
          const isTauri = typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
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
    isDisconnected = true;
    isTauriLive = false;
    session.status = 'disconnected';

    if (appState.config.app_settings?.close_tab_on_exit) {
      appState.closeTab(session.id);
      return;
    }

    if (term) {
      term.writeln(`\r\n\x1b[90m──────────────────────────────────────────────\x1b[0m`);
      term.writeln(`\x1b[33;1m[Session closed]\x1b[0m \x1b[90mConnection closed (exit code: ${exitCode}).\x1b[0m`);
      term.writeln(`\x1b[90mPress \x1b[37;1m[Enter]\x1b[0m\x1b[90m or click \x1b[37;1mReconnect\x1b[0m\x1b[90m below to restart session.\x1b[0m\r\n`);
    }
  }

  async function reconnect() {
    if (!term || isConnecting) return;
    isDisconnected = false;
    session.status = 'connected';
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

    fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(terminalContainer);
    fitAddon.fit();

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

    terminalContainer.addEventListener('contextmenu', handleContextMenu);

    // Listen for terminal exit event from Rust backend
    unlistenExit = await listen<number>(`terminal-exit-${session.id}`, (event) => {
      handleSessionExit(event.payload ?? 0);
    });

    await startSession();

    // Resize observer to keep xterm fitting parent container
    resizeObserver = new ResizeObserver(() => {
      try {
        fitAddon?.fit();
        if (isTauriLive && term) {
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
  });

  function setupFallbackEcho() {
    if (!term) return;
    if (onDataDisposable) {
      onDataDisposable.dispose();
      onDataDisposable = null;
    }
    term.writeln(`\x1b[90m┌─ \x1b[37;1mrustius\x1b[0m \x1b[90m${session.title} (${session.badge || session.type})\x1b[0m`);
    term.writeln(`\x1b[90m└─ Status: \x1b[32mconnected (web preview mode)\x1b[0m\r\n`);
    term.write('\x1b[90mps\x1b[0m \x1b[37m>\x1b[0m ');

    onDataDisposable = term.onData((data) => {
      if (data === '\r') {
        term?.write('\r\n\x1b[90mps\x1b[0m \x1b[37m>\x1b[0m ');
      } else if (data === '\u007f') {
        term?.write('\b \b');
      } else {
        term?.write(data);
      }
    });
  }

  onDestroy(() => {
    resizeObserver?.disconnect();
    if (unlistenData) {
      unlistenData();
      unlistenData = null;
    }
    if (unlistenExit) {
      unlistenExit();
      unlistenExit = null;
    }
    if (onDataDisposable) {
      onDataDisposable.dispose();
      onDataDisposable = null;
    }
    if (isTauriLive) {
      const closeCmd = session.type === 'ssh' ? 'ssh_disconnect' : 'pty_close';
      invoke(closeCmd, { tabId: session.id }).catch(() => {});
    }
    term?.dispose();
  });
</script>

<div class="terminal-shell">
  {#if isDisconnected}
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
  <div class="terminal-body" bind:this={terminalContainer}></div>
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
