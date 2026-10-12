<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { appState } from '../stores/appState.svelte';
  import { DisposableStore } from '../utils/disposable';
  import type { TabSession } from '../types';
  import {
    Monitor,
    Maximize2,
    Minimize2,
    RotateCcw,
    X,
    Zap,
    Power,
    ExternalLink,
  } from '@lucide/svelte';

  let { session = $bindable() }: { session: TabSession } = $props();

  let containerRef: HTMLDivElement | null = $state(null);
  let viewportRef: HTMLDivElement | null = $state(null);

  let isConnecting = $state(true);
  let isConnected = $state(false);
  let isDisconnected = $state(false);
  let disconnectReason = $state('');
  let isFullscreen = $state(false);
  let isDocked = $state(true);

  const componentDisposables = new DisposableStore();
  const sessionDisposables = new DisposableStore();
  let sessionGeneration = 0;

  async function startSession() {
    if (componentDisposables.isDisposed) return;
    const currentGen = ++sessionGeneration;

    isConnecting = true;
    isConnected = false;
    isDisconnected = false;
    disconnectReason = '';
    session.status = 'connecting';

    sessionDisposables.clear();

    const isTauri = typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
    if (!isTauri) {
      isConnecting = false;
      isDisconnected = true;
      session.status = 'error';
      disconnectReason = 'Native FreeRDP acceleration requires the Rustius desktop application.';
      return;
    }

    if (!session.connectionId) {
      isConnecting = false;
      isDisconnected = true;
      session.status = 'error';
      disconnectReason = 'No connection ID specified for this RDP session.';
      return;
    }

    try {
      // 1. Calculate physical viewport coordinates on the desktop
      let x = 0, y = 0, width = 1280, height = 800;
      const dpr = window.devicePixelRatio || 1;
      if (viewportRef) {
        const rect = viewportRef.getBoundingClientRect();
        x = Math.round(rect.left * dpr);
        y = Math.round(rect.top * dpr);
        width = Math.round(Math.max(rect.width * dpr, 200));
        height = Math.round(Math.max(rect.height * dpr, 200));
      }

      // 2. Listen for session exit event
      const unlistenExit = await listen<number>(`rdp-exit-${session.id}`, (event) => {
        if (currentGen !== sessionGeneration || componentDisposables.isDisposed) return;
        isConnected = false;
        isConnecting = false;
        isDisconnected = true;
        session.status = 'disconnected';
        disconnectReason = event.payload === 0
          ? 'FreeRDP session ended by user or server'
          : `FreeRDP exited with code ${event.payload}`;
      });

      if (currentGen !== sessionGeneration || componentDisposables.isDisposed) {
        unlistenExit();
        return;
      }
      sessionDisposables.add(unlistenExit);

      // 3. Invoke freerdp_connect backend command
      await invoke('freerdp_connect', {
        tabId: session.id,
        connectionId: session.connectionId,
        x,
        y,
        width,
        height,
      });

      if (currentGen !== sessionGeneration || componentDisposables.isDisposed) {
        invoke('freerdp_disconnect', { tabId: session.id }).catch(() => {});
        return;
      }

      isConnected = true;
      isConnecting = false;
      isDisconnected = false;
      session.status = 'connected';

      // 4. Setup viewport bounds sync on container resize or window resize
      const syncBounds = () => {
        if (!viewportRef || isDisconnected || !isDocked) return;
        const rect = viewportRef.getBoundingClientRect();
        const scale = window.devicePixelRatio || 1;
        invoke('freerdp_update_bounds', {
          tabId: session.id,
          x: Math.round(rect.left * scale),
          y: Math.round(rect.top * scale),
          width: Math.round(Math.max(rect.width * scale, 100)),
          height: Math.round(Math.max(rect.height * scale, 100)),
        }).catch(() => {});
      };

      if (window.ResizeObserver && viewportRef) {
        const ro = new ResizeObserver(() => {
          syncBounds();
        });
        ro.observe(viewportRef);
        sessionDisposables.add(() => ro.disconnect());
      }
      sessionDisposables.trackEventListener(window, 'resize', syncBounds);

      const unlistenMove = await listen('window-moved', () => syncBounds());
      sessionDisposables.add(unlistenMove);
    } catch (err: any) {
      console.error('FreeRDP connection error:', err);
      if (currentGen !== sessionGeneration || componentDisposables.isDisposed) return;
      isConnected = false;
      isConnecting = false;
      isDisconnected = true;
      session.status = 'error';
      disconnectReason = err?.message || String(err);
    }
  }

  async function toggleDocked() {
    isDocked = !isDocked;
    await invoke('freerdp_set_docked', { tabId: session.id, docked: isDocked }).catch(() => {});
  }

  async function handleDisconnect() {
    isConnecting = false;
    isConnected = false;
    isDisconnected = true;
    session.status = 'disconnected';
    disconnectReason = 'Session disconnected by user';
    await invoke('freerdp_disconnect', { tabId: session.id }).catch(() => {});
  }

  function toggleFullscreen() {
    if (!containerRef) return;
    if (!document.fullscreenElement) {
      containerRef.requestFullscreen().then(() => {
        isFullscreen = true;
      }).catch(() => {});
    } else {
      document.exitFullscreen().then(() => {
        isFullscreen = false;
      }).catch(() => {});
    }
  }

  async function reconnect() {
    if (isConnecting || componentDisposables.isDisposed) return;
    isDisconnected = false;
    isConnected = false;
    isConnecting = true;
    disconnectReason = '';
    await invoke('freerdp_disconnect', { tabId: session.id }).catch(() => {});
    await startSession();
  }

  function handleCloseTab() {
    handleDisconnect();
    appState.closeTab(session.id);
  }

  $effect(() => {
    const isTabActive = appState.activeTabId === session.id;
    if (isConnected) {
      invoke('freerdp_set_visibility', { tabId: session.id, visible: isTabActive }).catch(() => {});
      if (isTabActive && isDocked) {
        requestAnimationFrame(() => {
          if (!viewportRef) return;
          const rect = viewportRef.getBoundingClientRect();
          const scale = window.devicePixelRatio || 1;
          invoke('freerdp_update_bounds', {
            tabId: session.id,
            x: Math.round(rect.left * scale),
            y: Math.round(rect.top * scale),
            width: Math.round(Math.max(rect.width * scale, 100)),
            height: Math.round(Math.max(rect.height * scale, 100)),
          }).catch(() => {});
        });
      }
    }
  });

  onMount(async () => {
    const handleFsChange = () => {
      isFullscreen = !!document.fullscreenElement;
    };
    componentDisposables.trackEventListener(document, 'fullscreenchange', handleFsChange);

    await startSession();
  });

  onDestroy(() => {
    sessionDisposables.dispose();
    componentDisposables.dispose();
    invoke('freerdp_disconnect', { tabId: session.id }).catch(() => {});
  });
</script>

<div class="rdp-shell" bind:this={containerRef}>
  <!-- Minimalist Geist Control Bar -->
  <div class="rdp-header">
    <div class="header-left">
      <Monitor size={14} class="rdp-icon" />
      <span class="session-name">{session.title}</span>

      {#if isConnected}
        <span class="status-indicator online"></span>
        <span class="status-label">Online</span>
      {:else if isConnecting}
        <span class="status-indicator connecting"></span>
        <span class="status-label">Connecting...</span>
      {:else}
        <span class="status-indicator offline"></span>
        <span class="status-label">Disconnected</span>
      {/if}

      <span class="acceleration-pill native" title="Direct3D Hardware-Accelerated FreeRDP Engine">
        <Zap size={11} />
        <span>FreeRDP · Direct3D</span>
      </span>

      {#if !isDocked}
        <span class="detached-badge">Detached Window</span>
      {/if}
    </div>

    <div class="header-right">
      {#if isConnected || isConnecting}
        <button
          class="ctrl-btn"
          onclick={toggleDocked}
          title={isDocked ? 'Detach to standalone native window' : 'Dock back to tab viewport'}
        >
          {#if isDocked}
            <ExternalLink size={12} />
            <span>Pop Out</span>
          {:else}
            <Minimize2 size={12} />
            <span>Dock</span>
          {/if}
        </button>

        <button
          class="ctrl-btn danger"
          onclick={handleDisconnect}
          title="Disconnect FreeRDP Session"
        >
          <Power size={12} />
          <span>Disconnect</span>
        </button>
      {/if}

      <button
        class="ctrl-btn"
        onclick={toggleFullscreen}
        title="Toggle Fullscreen Mode"
      >
        {#if isFullscreen}
          <Minimize2 size={12} />
        {:else}
          <Maximize2 size={12} />
        {/if}
        <span>{isFullscreen ? 'Exit Fullscreen' : 'Fullscreen'}</span>
      </button>
    </div>
  </div>

  <!-- Disconnected / Exit Banner -->
  {#if isDisconnected}
    <div class="disconnect-bar">
      <div class="disconnect-left">
        <span class="status-indicator offline"></span>
        <span class="disconnect-text">{disconnectReason || 'Remote Desktop session ended'}</span>
      </div>
      <div class="disconnect-right">
        <button class="action-btn reconnect-btn" onclick={reconnect}>
          <RotateCcw size={12} />
          <span>Reconnect</span>
        </button>
        <button class="action-btn close-btn" onclick={handleCloseTab} title="Close Tab">
          <X size={12} />
          <span>Close</span>
        </button>
      </div>
    </div>
  {/if}

  <!-- RDP Viewport Surface for child window docking -->
  <div class="rdp-viewport" bind:this={viewportRef}>
    <div class="freerdp-native-backdrop">
      {#if !isDocked && isConnected}
        <div class="native-detached-msg">
          <ExternalLink size={28} class="placeholder-icon" />
          <p class="placeholder-title">Session Running in Standalone Window</p>
          <p class="placeholder-desc">FreeRDP is running in its own native window with full OS titlebar and controls.</p>
          <button class="action-btn" onclick={toggleDocked}>
            <Minimize2 size={12} />
            <span>Dock Back to Tab</span>
          </button>
        </div>
      {:else if isConnecting}
        <div class="native-connecting-msg">
          <span class="status-indicator connecting"></span>
          <span>Initializing hardware-accelerated FreeRDP session...</span>
        </div>
      {:else if isDisconnected}
        <div class="native-disconnected-msg">
          <Monitor size={32} class="placeholder-icon" />
          <p class="placeholder-title">{session.title || 'Remote Desktop'}</p>
          <p class="placeholder-desc">{disconnectReason || 'Session disconnected'}</p>
          <button class="action-btn reconnect-btn" onclick={reconnect}>
            <RotateCcw size={12} />
            <span>Reconnect</span>
          </button>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .rdp-shell {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    background: #000000;
    overflow: hidden;
  }

  .rdp-header {
    height: 38px;
    padding: 0 14px;
    background: var(--canvas);
    border-bottom: 1px solid var(--hairline);
    display: flex;
    align-items: center;
    justify-content: space-between;
    user-select: none;
    z-index: 10;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .header-right {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .session-name {
    font-size: 12px;
    font-weight: 500;
    color: var(--ink);
  }

  .status-indicator {
    width: 6px;
    height: 6px;
    border-radius: var(--radius-full);
    flex-shrink: 0;
  }

  .status-indicator.online {
    background: var(--accent-green);
    box-shadow: none;
  }

  .status-indicator.connecting {
    background: var(--accent-amber);
    box-shadow: none;
    animation: pulse 1.2s infinite ease-in-out;
  }

  .status-indicator.offline {
    background: var(--faint);
    box-shadow: none;
  }

  @keyframes pulse {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 1; }
  }

  .status-label {
    font-size: 11px;
    color: var(--mute);
  }

  .ctrl-btn {
    height: 26px;
    font-size: 11px;
    padding: 0 10px;
    background: transparent;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-md);
    color: var(--body);
    display: flex;
    align-items: center;
    gap: 5px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .ctrl-btn:hover:not(:disabled) {
    background: var(--surface-hover);
    color: var(--ink);
    border-color: var(--border);
  }

  .ctrl-btn.danger {
    color: #ff4d4d;
    border-color: rgba(238, 0, 0, 0.25);
  }

  .ctrl-btn.danger:hover:not(:disabled) {
    background: rgba(238, 0, 0, 0.15);
    border-color: #ee0000;
    color: #ff4d4d;
  }

  /* Disconnect banner */
  .disconnect-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 14px;
    background: #0a0a0a;
    border-bottom: 1px solid #262626;
    font-size: 12px;
    animation: fadeIn 0.15s ease-out;
    z-index: 9;
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

  .disconnect-text {
    color: var(--body);
    font-size: 12px;
  }

  .disconnect-right {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .action-btn {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 4px 10px;
    font-size: 11px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: all 0.12s ease;
    border: 1px solid #333333;
    background: #141414;
    color: #ededed;
  }

  .action-btn:hover {
    background: #222222;
    border-color: #555555;
    color: #ffffff;
  }

  .reconnect-btn:hover {
    background: #222222;
    border-color: #555555;
    color: #ffffff;
  }

  .close-btn:hover {
    background: rgba(238, 0, 0, 0.15);
    border-color: #ee0000;
    color: #ff4d4d;
  }

  /* Viewport */
  .rdp-viewport {
    flex: 1;
    position: relative;
    background: #000000;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }

  .acceleration-pill {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 10px;
    font-family: var(--font-mono);
    padding: 2px 7px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--hairline);
    background: rgba(255, 255, 255, 0.03);
    color: var(--body);
    user-select: none;
    transition: all 0.12s ease;
  }

  .acceleration-pill :global(svg) {
    color: var(--mute);
    transition: color 0.12s ease;
  }

  .acceleration-pill.native {
    background: rgba(255, 255, 255, 0.03);
    border-color: var(--hairline);
    color: var(--body);
  }

  .acceleration-pill.native:hover {
    background: rgba(255, 255, 255, 0.06);
    border-color: var(--hairline-hover);
    color: var(--ink-secondary);
  }

  .acceleration-pill.native:hover :global(svg) {
    color: var(--ink-secondary);
  }

  .detached-badge {
    font-size: 10px;
    font-family: var(--font-mono);
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    background: rgba(255, 255, 255, 0.08);
    color: var(--body);
  }

  .freerdp-native-backdrop {
    width: 100%;
    height: 100%;
    background: #000000;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--mute);
    user-select: none;
  }

  .native-connecting-msg {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--body);
  }

  .native-detached-msg {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 24px;
    text-align: center;
  }

  .native-disconnected-msg {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 24px;
    text-align: center;
  }

  .placeholder-title {
    font-size: 14px;
    font-weight: 500;
    color: var(--ink);
    margin: 4px 0 0 0;
  }

  .placeholder-desc {
    font-size: 12px;
    color: var(--mute);
    max-width: 360px;
    margin: 0 0 12px 0;
  }

  :global(.placeholder-icon) {
    color: var(--mute);
    opacity: 0.4;
  }

  :global(.rdp-icon) {
    color: var(--mute);
  }
</style>
