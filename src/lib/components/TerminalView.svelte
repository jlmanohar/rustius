<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import '@xterm/xterm/css/xterm.css';
  import type { TabSession } from '../types';

  let { session }: { session: TabSession } = $props();

  let terminalContainer: HTMLDivElement | null = $state(null);
  let term: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let unlistenData: UnlistenFn | null = null;
  let isTauriLive = false;

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
        selectionBackground: 'rgba(255, 255, 255, 0.2)',
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

    // Custom keyboard handler for copy / paste
    term.attachCustomKeyEventHandler((event: KeyboardEvent) => {
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

    if (session.type === 'local_terminal') {
      try {
        // Listen for output streaming from Rust PTY
        unlistenData = await listen<number[]>(`terminal-data-${session.id}`, (event) => {
          if (term && event.payload) {
            term.write(new Uint8Array(event.payload));
          }
        });

        // Spawn PTY in Rust backend
        await invoke('pty_spawn', {
          tabId: session.id,
          cols: term.cols,
          rows: term.rows,
        });

        isTauriLive = true;

        // Forward keystrokes to Rust PTY
        term.onData((data) => {
          const encoded = Array.from(new TextEncoder().encode(data));
          invoke('pty_write', { tabId: session.id, data: encoded }).catch((err) => {
            console.error('Failed to write to PTY:', err);
          });
        });
      } catch (e) {
        console.warn('Tauri PTY unavailable (running in web preview mode):', e);
        setupFallbackEcho();
      }
    } else if (session.type === 'ssh' && session.connectionId) {
      try {
        term.writeln(`\x1b[90mConnecting to remote SSH host...\x1b[0m\r\n`);

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

        term.onData((data) => {
          const encoded = Array.from(new TextEncoder().encode(data));
          invoke('ssh_write', { tabId: session.id, data: encoded }).catch((err) => {
            console.error('Failed to write to SSH session:', err);
          });
        });
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
        }
      }
    } else {
      setupFallbackEcho();
    }

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
    term.writeln(`\x1b[90m┌─ \x1b[37;1mrustius\x1b[0m \x1b[90m${session.title} (${session.badge || session.type})\x1b[0m`);
    term.writeln(`\x1b[90m└─ Status: \x1b[32mconnected (web preview mode)\x1b[0m\r\n`);
    term.write('\x1b[90mps\x1b[0m \x1b[37m>\x1b[0m ');

    term.onData((data) => {
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
    }
    if (isTauriLive) {
      const closeCmd = session.type === 'ssh' ? 'ssh_disconnect' : 'pty_close';
      invoke(closeCmd, { tabId: session.id }).catch(() => {});
    }
    term?.dispose();
  });
</script>

<div class="terminal-shell">
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
