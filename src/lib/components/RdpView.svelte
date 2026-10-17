<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { appState } from '../stores/appState.svelte';
  import { DisposableStore } from '../utils/disposable';
  import type { TabSession, RdpExitDiagnostic, IronRdpFrame } from '../types';
  import {
    Monitor,
    Maximize2,
    Minimize2,
    RotateCcw,
    X,
    Zap,
    Power,
    ExternalLink,
    Lock,
    Shield,
    Key,
    AlertCircle,
    Copy,
    Check,
  } from '@lucide/svelte';

  let { session = $bindable() }: { session: TabSession } = $props();

  let containerRef: HTMLDivElement | null = $state(null);
  let viewportRef: HTMLDivElement | null = $state(null);
  let canvasRef: HTMLCanvasElement | null = $state(null);

  let isConnecting = $state(true);
  let isConnected = $state(false);
  let isDisconnected = $state(false);
  let disconnectReason = $state('');
  let isFullscreen = $state(false);
  let isDocked = $state(true);
  let exitInfo = $state<RdpExitDiagnostic | null>(null);
  let copiedDetails = $state(false);

  let conn = $derived(appState.findConnection(session.connectionId));
  let rdpEngine = $derived(conn?.rdp_config?.engine || 'ironrdp');
  let isIronRdp = $derived(rdpEngine === 'ironrdp');

  let canvasCtx: CanvasRenderingContext2D | null = null;
  let pixelBuffer: Uint8ClampedArray | null = null;
  let pendingMouseMove: { x: number; y: number } | null = null;
  let mouseMoveRafId: number | null = null;

  function renderFrame(frame: IronRdpFrame) {
    if (!canvasRef) return;
    if (!canvasCtx || canvasRef.width !== frame.desktop_width || canvasRef.height !== frame.desktop_height) {
      canvasRef.width = frame.desktop_width;
      canvasRef.height = frame.desktop_height;
      canvasCtx = canvasRef.getContext('2d', { alpha: false });
    }
    if (!canvasCtx) return;

    try {
      const bin = atob(frame.data);
      const len = bin.length;
      if (!pixelBuffer || pixelBuffer.length < len) {
        pixelBuffer = new Uint8ClampedArray(Math.max(len, 1920 * 1080 * 4));
      }
      for (let i = 0; i < len; i++) {
        pixelBuffer[i] = bin.charCodeAt(i);
      }
      const slice = new Uint8ClampedArray(pixelBuffer.buffer as ArrayBuffer, 0, len);
      const imgData = new ImageData(slice, frame.width, frame.height);
      canvasCtx.putImageData(imgData, frame.x, frame.y);
    } catch (err) {
      console.error('Failed to render IronRDP frame:', err);
    }
  }

  function getCanvasMouseCoords(e: MouseEvent): { x: number; y: number } | null {
    if (!canvasRef) return null;
    const rect = canvasRef.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return null;
    const scaleX = canvasRef.width / rect.width;
    const scaleY = canvasRef.height / rect.height;
    const x = Math.floor((e.clientX - rect.left) * scaleX);
    const y = Math.floor((e.clientY - rect.top) * scaleY);
    return {
      x: Math.max(0, Math.min(canvasRef.width - 1, x)),
      y: Math.max(0, Math.min(canvasRef.height - 1, y)),
    };
  }

  function handleMouseMove(e: MouseEvent) {
    if (!isConnected || !isIronRdp) return;
    const coords = getCanvasMouseCoords(e);
    if (!coords) return;
    pendingMouseMove = coords;
    if (mouseMoveRafId === null) {
      mouseMoveRafId = requestAnimationFrame(() => {
        mouseMoveRafId = null;
        if (pendingMouseMove && isConnected) {
          invoke('ironrdp_mouse_move', {
            tabId: session.id,
            x: pendingMouseMove.x,
            y: pendingMouseMove.y,
          }).catch(() => {});
        }
      });
    }
  }

  function handleMouseDown(e: MouseEvent) {
    if (!isConnected || !isIronRdp) return;
    canvasRef?.focus();
    const coords = getCanvasMouseCoords(e);
    if (coords) {
      invoke('ironrdp_mouse_move', { tabId: session.id, x: coords.x, y: coords.y }).catch(() => {});
    }
    invoke('ironrdp_mouse_button', {
      tabId: session.id,
      button: e.button,
      pressed: true,
    }).catch(() => {});
  }

  function handleMouseUp(e: MouseEvent) {
    if (!isConnected || !isIronRdp) return;
    invoke('ironrdp_mouse_button', {
      tabId: session.id,
      button: e.button,
      pressed: false,
    }).catch(() => {});
  }

  function handleContextMenu(e: MouseEvent) {
    e.preventDefault();
  }

  function handleWheel(e: WheelEvent) {
    if (!isConnected || !isIronRdp) return;
    e.preventDefault();
    const isVertical = Math.abs(e.deltaY) >= Math.abs(e.deltaX);
    const delta = isVertical ? -e.deltaY : -e.deltaX;
    const rotationUnits = Math.round(Math.sign(delta) * 120);
    invoke('ironrdp_mouse_wheel', {
      tabId: session.id,
      isVertical,
      rotationUnits,
    }).catch(() => {});
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (!isConnected || !isIronRdp) return;
    if (['Tab', 'F5', 'AltLeft', 'AltRight', 'ContextMenu', 'MetaLeft', 'MetaRight'].includes(e.code)) {
      e.preventDefault();
    }
    invoke('ironrdp_key_event', {
      tabId: session.id,
      code: e.code,
      pressed: true,
    }).catch(() => {});
  }

  function handleKeyUp(e: KeyboardEvent) {
    if (!isConnected || !isIronRdp) return;
    if (['Tab', 'F5', 'AltLeft', 'AltRight', 'ContextMenu', 'MetaLeft', 'MetaRight'].includes(e.code)) {
      e.preventDefault();
    }
    invoke('ironrdp_key_event', {
      tabId: session.id,
      code: e.code,
      pressed: false,
    }).catch(() => {});
  }

  async function sendCtrlAltDel() {
    if (!isConnected || !isIronRdp) return;
    for (const code of ['ControlLeft', 'AltLeft', 'Delete']) {
      await invoke('ironrdp_key_event', { tabId: session.id, code, pressed: true }).catch(() => {});
    }
    for (const code of ['Delete', 'AltLeft', 'ControlLeft']) {
      await invoke('ironrdp_key_event', { tabId: session.id, code, pressed: false }).catch(() => {});
    }
  }

  function openEditConnection() {
    if (!session.connectionId) return;
    const conn = appState.findConnection(session.connectionId);
    if (conn) {
      appState.editingConnection = conn;
      appState.isConnectionModalOpen = true;
    }
  }

  async function copyErrorDetails() {
    if (!exitInfo) return;
    const lines = [
      `Rustius RDP Error: ${exitInfo.title}`,
      `Message: ${exitInfo.message}`,
      `Exit Code: ${exitInfo.code} (0x${(exitInfo.code >>> 0).toString(16).toUpperCase()})`,
    ];
    if (exitInfo.raw_error) {
      lines.push(`${isIronRdp ? 'IronRDP' : 'FreeRDP'} Diagnostic: ${exitInfo.raw_error}`);
    }
    if (session.connectionId) {
      const conn = appState.findConnection(session.connectionId);
      if (conn) {
        lines.push(`Target: ${conn.host}:${conn.port}`);
      }
    }
    try {
      await navigator.clipboard.writeText(lines.join('\n'));
      copiedDetails = true;
      setTimeout(() => {
        copiedDetails = false;
      }, 2000);
    } catch (e) {
      console.warn('Clipboard copy failed:', e);
    }
  }

  const componentDisposables = new DisposableStore();
  const sessionDisposables = new DisposableStore();
  let sessionGeneration = 0;

  function getFittedBounds() {
    if (!viewportRef) return null;
    const rect = viewportRef.getBoundingClientRect();
    const scale = window.devicePixelRatio || 1;
    const rdpCfg = appState.findConnection(session.connectionId)?.rdp_config;
    const resMode = rdpCfg?.resolution_mode || 'auto';

    let desktopW: number;
    let desktopH: number;

    if (resMode === 'auto') {
      desktopW = Math.round(Math.max(rect.width * scale, 800));
      desktopH = Math.round(Math.max(rect.height * scale, 600));
    } else if (resMode === 'native') {
      desktopW = Math.round((window.screen?.width || 1920) * scale);
      desktopH = Math.round((window.screen?.height || 1080) * scale);
    } else if (resMode === '1080p') {
      desktopW = 1920;
      desktopH = 1080;
    } else if (resMode === '720p') {
      desktopW = 1280;
      desktopH = 720;
    } else if (resMode === 'custom') {
      desktopW = rdpCfg?.width || 1920;
      desktopH = rdpCfg?.height || 1080;
    } else {
      desktopW = Math.round(Math.max(rect.width * scale, 800));
      desktopH = Math.round(Math.max(rect.height * scale, 600));
    }

    let desktopScale: number | undefined;
    const cfgScale = rdpCfg?.scale_factor ?? 0;
    if (cfgScale > 0) {
      desktopScale = cfgScale;
    } else {
      if (resMode === 'native' || desktopW >= 2560 || desktopH >= 1440) {
        desktopScale = Math.min(Math.max(Math.round(scale * 100), 100), 250);
      } else {
        desktopScale = 100;
      }
    }

    const aspect = (desktopW > 0 && desktopH > 0) ? (desktopW / desktopH) : (16 / 9);

    let renderW = rect.width;
    let renderH = rect.height;
    let offX = 0;
    let offY = 0;

    if (renderW > 0 && renderH > 0) {
      if (renderW / renderH > aspect) {
        const fittedW = renderH * aspect;
        offX = (renderW - fittedW) / 2;
        renderW = fittedW;
      } else {
        const fittedH = renderW / aspect;
        offY = (renderH - fittedH) / 2;
        renderH = fittedH;
      }
    }

    return {
      x: Math.round((rect.left + offX) * scale),
      y: Math.round((rect.top + offY) * scale),
      width: Math.round(Math.max(renderW * scale, 100)),
      height: Math.round(Math.max(renderH * scale, 100)),
      desktopWidth: desktopW,
      desktopHeight: desktopH,
      scaleDesktop: desktopScale,
    };
  }

  function syncBounds() {
    if (isIronRdp || !viewportRef || isDisconnected || !isDocked) return;
    if (appState.activeTabId !== session.id) return;
    const bounds = getFittedBounds();
    if (!bounds) return;
    invoke('freerdp_update_bounds', {
      tabId: session.id,
      x: bounds.x,
      y: bounds.y,
      width: bounds.width,
      height: bounds.height,
    }).catch(() => {});
  }

  async function startSession() {
    if (componentDisposables.isDisposed) return;
    const currentGen = ++sessionGeneration;

    isConnecting = true;
    isConnected = false;
    isDisconnected = false;
    disconnectReason = '';
    exitInfo = null;
    session.status = 'connecting';

    sessionDisposables.clear();

    const isTauri = typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
    if (!isTauri) {
      isConnecting = false;
      isDisconnected = true;
      session.status = 'error';
      disconnectReason = 'Native RDP acceleration requires the Rustius desktop application.';
      exitInfo = {
        code: -1,
        title: 'Desktop Shell Required',
        message: disconnectReason,
        severity: 'error',
      };
      return;
    }

    if (!session.connectionId) {
      isConnecting = false;
      isDisconnected = true;
      session.status = 'error';
      disconnectReason = 'No connection ID specified for this RDP session.';
      exitInfo = {
        code: -1,
        title: 'Configuration Error',
        message: disconnectReason,
        severity: 'error',
      };
      return;
    }

    try {
      const bounds = getFittedBounds() || {
        x: 0,
        y: 0,
        width: 1280,
        height: 720,
        desktopWidth: 1920,
        desktopHeight: 1080,
        scaleDesktop: 100,
      };

      const unlistenExit = await listen<number | RdpExitDiagnostic>(`rdp-exit-${session.id}`, (event) => {
        if (currentGen !== sessionGeneration || componentDisposables.isDisposed) return;
        isConnected = false;
        isConnecting = false;
        isDisconnected = true;
        session.status = 'disconnected';

        if (typeof event.payload === 'object' && event.payload !== null) {
          const diag = event.payload;
          exitInfo = diag;
          disconnectReason = `${diag.title}: ${diag.message}`;
          if (diag.code !== 0) {
            session.status = 'error';
          }
        } else {
          const code = event.payload;
          const engineName = isIronRdp ? 'IronRDP' : 'FreeRDP';
          if (code === 0) {
            disconnectReason = `${engineName} session ended by user or server`;
            exitInfo = {
              code: 0,
              title: 'Session Disconnected',
              message: disconnectReason,
              severity: 'info',
            };
          } else {
            disconnectReason = `${engineName} exited with code ${code}`;
            exitInfo = {
              code,
              title: 'Session Disconnected',
              message: disconnectReason,
              severity: 'error',
            };
            session.status = 'error';
          }
        }
      });

      if (currentGen !== sessionGeneration || componentDisposables.isDisposed) {
        unlistenExit();
        return;
      }
      sessionDisposables.add(unlistenExit);

      if (isIronRdp) {
        const unlistenConnected = await listen(`rdp-connected-${session.id}`, () => {
          if (currentGen !== sessionGeneration || componentDisposables.isDisposed) return;
          isConnected = true;
          isConnecting = false;
          isDisconnected = false;
          session.status = 'connected';
          setTimeout(() => {
            canvasRef?.focus();
          }, 100);
        });
        sessionDisposables.add(unlistenConnected);

        const unlistenFrame = await listen<IronRdpFrame>(`rdp-frame-${session.id}`, (event) => {
          if (currentGen !== sessionGeneration || componentDisposables.isDisposed) return;
          renderFrame(event.payload);
        });
        sessionDisposables.add(unlistenFrame);

        await invoke('ironrdp_connect', {
          tabId: session.id,
          connectionId: session.connectionId,
          credentialsOverride: session.credentialsOverride,
          width: bounds.desktopWidth,
          height: bounds.desktopHeight,
          scaleFactor: bounds.scaleDesktop,
        });
      } else {
        await invoke('freerdp_connect', {
          tabId: session.id,
          connectionId: session.connectionId,
          credentialsOverride: session.credentialsOverride,
          x: bounds.x,
          y: bounds.y,
          width: bounds.width,
          height: bounds.height,
          desktopWidth: bounds.desktopWidth,
          desktopHeight: bounds.desktopHeight,
          scaleDesktop: bounds.scaleDesktop,
        });

        if (currentGen !== sessionGeneration || componentDisposables.isDisposed) {
          invoke('freerdp_disconnect', { tabId: session.id }).catch(() => {});
          return;
        }

        isConnected = true;
        isConnecting = false;
        isDisconnected = false;
        session.status = 'connected';

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
      }
    } catch (err: any) {
      console.error(`${isIronRdp ? 'IronRDP' : 'FreeRDP'} connection error:`, err);
      if (currentGen !== sessionGeneration || componentDisposables.isDisposed) return;
      isConnected = false;
      isConnecting = false;
      isDisconnected = true;
      session.status = 'error';
      const msg = err?.message || String(err);
      disconnectReason = msg;
      exitInfo = {
        code: -1,
        title: 'Connection Failed',
        message: msg,
        severity: 'error',
      };
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
    exitInfo = {
      code: 0,
      title: 'Session Disconnected',
      message: 'Session disconnected by user',
      severity: 'info',
    };
    await invoke('rdp_disconnect', { tabId: session.id }).catch(() => {});
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
    await invoke('rdp_disconnect', { tabId: session.id }).catch(() => {});
    await startSession();
  }

  function handleCloseTab() {
    handleDisconnect();
    appState.closeTab(session.id);
  }

  $effect(() => {
    const isTabActive = appState.activeTabId === session.id;
    const _drawerOpen = appState.isNotesDrawerOpen;
    if (isConnected) {
      if (!isIronRdp) {
        invoke('freerdp_set_visibility', { tabId: session.id, visible: isTabActive }).catch(() => {});
        if (isTabActive && isDocked) {
          requestAnimationFrame(() => {
            syncBounds();
          });
        }
      } else if (isTabActive) {
        requestAnimationFrame(() => {
          canvasRef?.focus();
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
    if (mouseMoveRafId !== null) {
      cancelAnimationFrame(mouseMoveRafId);
    }
    sessionDisposables.dispose();
    componentDisposables.dispose();
    invoke('rdp_disconnect', { tabId: session.id }).catch(() => {});
  });
</script>

<div class="rdp-shell" class:with-notes-drawer={appState.isNotesDrawerOpen} bind:this={containerRef}>
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

      {#if isIronRdp}
        <span class="acceleration-pill iron" title="Pure Rust, Memory-Safe IronRDP Client">
          <Shield size={11} />
          <span>IronRDP · Pure Rust</span>
        </span>
      {:else}
        <span class="acceleration-pill native" title="Direct3D Hardware-Accelerated FreeRDP Engine">
          <Zap size={11} />
          <span>FreeRDP · Direct3D</span>
        </span>
      {/if}

      {#if !isIronRdp && !isDocked}
        <span class="detached-badge">Detached Window</span>
      {/if}
    </div>

    <div class="header-right">
      {#if isConnected || isConnecting}
        {#if isIronRdp && isConnected}
          <button
            class="ctrl-btn"
            onclick={sendCtrlAltDel}
            title="Send Ctrl+Alt+Del to remote session"
          >
            <Key size={12} />
            <span>Ctrl+Alt+Del</span>
          </button>
        {/if}

        {#if !isIronRdp}
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
        {/if}

        <button
          class="ctrl-btn danger"
          onclick={handleDisconnect}
          title="Disconnect RDP Session"
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
    <div class="disconnect-bar {exitInfo?.severity || 'info'}">
      <div class="disconnect-left">
        {#if exitInfo?.severity === 'security'}
          <span class="disconnect-badge security">
            <Lock size={12} />
            <span>SECURITY</span>
          </span>
        {:else if exitInfo?.severity === 'auth'}
          <span class="disconnect-badge auth">
            <Key size={12} />
            <span>AUTH</span>
          </span>
        {:else if exitInfo?.severity === 'error'}
          <span class="disconnect-badge error">
            <AlertCircle size={12} />
            <span>ERROR</span>
          </span>
        {:else}
          <span class="status-indicator offline"></span>
        {/if}

        <div class="disconnect-message-group">
          {#if exitInfo?.title && exitInfo.code !== 0}
            <span class="disconnect-title">{exitInfo.title}:</span>
          {/if}
          <span class="disconnect-text">{exitInfo?.message || disconnectReason || 'Remote Desktop session ended'}</span>
          {#if exitInfo?.raw_error}
            <span class="disconnect-raw-code" title={exitInfo.raw_error}>({exitInfo.raw_error})</span>
          {/if}
        </div>
      </div>
      <div class="disconnect-right">
        {#if session.connectionId && (exitInfo?.severity === 'auth' || exitInfo?.severity === 'security')}
          <button class="action-btn edit-btn" onclick={openEditConnection} title="Edit Connection Credentials">
            <Key size={12} />
            <span>Edit Credentials</span>
          </button>
        {/if}
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

  <!-- RDP Viewport Surface -->
  <div class="rdp-viewport" bind:this={viewportRef}>
    {#if isIronRdp}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div
        class="ironrdp-container"
        class:hidden={!isConnected}
        onclick={() => canvasRef?.focus()}
        onkeydown={(e) => { if (e.key === 'Enter') canvasRef?.focus(); }}
        role="region"
        aria-label="IronRDP Session Viewport"
      >
        <canvas
          bind:this={canvasRef}
          class="rdp-canvas"
          tabindex="0"
          onmousemove={handleMouseMove}
          onmousedown={handleMouseDown}
          onmouseup={handleMouseUp}
          oncontextmenu={handleContextMenu}
          onwheel={handleWheel}
          onkeydown={handleKeyDown}
          onkeyup={handleKeyUp}
        ></canvas>
      </div>
    {/if}

    {#if !isIronRdp}
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
        {/if}
      </div>
    {/if}

    {#if isConnecting}
      <div class="native-connecting-msg">
        <span class="status-indicator connecting"></span>
        <span>{isIronRdp ? 'Initializing secure IronRDP session...' : 'Initializing hardware-accelerated FreeRDP session...'}</span>
      </div>
    {:else if isDisconnected}
      <div class="native-disconnected-card {exitInfo?.severity || 'info'}">
        <div class="card-icon-container">
          {#if exitInfo?.severity === 'security'}
            <Lock size={30} class="card-icon security" />
          {:else if exitInfo?.severity === 'auth'}
            <Key size={30} class="card-icon auth" />
          {:else if exitInfo?.severity === 'error'}
            <AlertCircle size={30} class="card-icon error" />
          {:else}
            <Monitor size={30} class="card-icon info" />
          {/if}
        </div>

        <h3 class="card-title">
          {exitInfo?.title || session.title || 'Remote Desktop Disconnected'}
        </h3>

        <p class="card-desc">
          {exitInfo?.message || disconnectReason || 'Remote Desktop session has ended.'}
        </p>

        {#if exitInfo?.raw_error}
          <div class="card-error-detail">
            <span class="detail-label">{isIronRdp ? 'IronRDP' : 'FreeRDP'} Diagnostic:</span>
            <code class="detail-code">{exitInfo.raw_error}</code>
          </div>
        {/if}

        <div class="card-actions">
          {#if session.connectionId && (exitInfo?.severity === 'auth' || exitInfo?.severity === 'security')}
            <button class="action-btn edit-btn primary" onclick={openEditConnection}>
              <Key size={13} />
              <span>Update Credentials</span>
            </button>
          {/if}
          <button class="action-btn reconnect-btn" onclick={reconnect}>
            <RotateCcw size={13} />
            <span>Reconnect</span>
          </button>
          <button class="action-btn copy-btn" onclick={copyErrorDetails} title="Copy error details">
            {#if copiedDetails}
              <Check size={13} />
              <span>Copied</span>
            {:else}
              <Copy size={13} />
              <span>Copy Details</span>
            {/if}
          </button>
        </div>
      </div>
    {/if}
  </div>

  <!-- mRemoteNG-style bottom status bar when an error or lockout occurs -->
  {#if isDisconnected && exitInfo && exitInfo.code !== 0}
    <div class="rdp-bottom-status-bar {exitInfo.severity}">
      <div class="bottom-status-left">
        {#if exitInfo.severity === 'security'}
          <Lock size={13} class="bottom-status-icon security" />
        {:else if exitInfo.severity === 'auth'}
          <Key size={13} class="bottom-status-icon auth" />
        {:else}
          <AlertCircle size={13} class="bottom-status-icon error" />
        {/if}
        <span class="bottom-status-title">{exitInfo.title}:</span>
        <span class="bottom-status-msg">{exitInfo.message}</span>
        {#if exitInfo.raw_error}
          <span class="bottom-status-code">[{exitInfo.raw_error}]</span>
        {/if}
      </div>
      <div class="bottom-status-right">
        <button class="bottom-copy-btn" onclick={copyErrorDetails} title="Copy error to clipboard">
          {#if copiedDetails}
            <Check size={11} />
            <span>Copied</span>
          {:else}
            <Copy size={11} />
            <span>Copy</span>
          {/if}
        </button>
      </div>
    </div>
  {/if}
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

  .rdp-shell.with-notes-drawer {
    width: calc(100% - 380px);
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

  .disconnect-bar.security,
  .disconnect-bar.auth {
    background: rgba(239, 68, 68, 0.08);
    border-bottom-color: rgba(239, 68, 68, 0.3);
  }

  .disconnect-bar.error {
    background: rgba(245, 158, 11, 0.08);
    border-bottom-color: rgba(245, 158, 11, 0.3);
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
    overflow: hidden;
  }

  .disconnect-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 6px;
    font-size: 10px;
    font-weight: 600;
    font-family: var(--font-mono);
    border-radius: var(--radius-sm);
    letter-spacing: 0.04em;
    flex-shrink: 0;
  }

  .disconnect-badge.security,
  .disconnect-badge.auth {
    background: rgba(239, 68, 68, 0.18);
    color: #f87171;
    border: 1px solid rgba(239, 68, 68, 0.3);
  }

  .disconnect-badge.error {
    background: rgba(245, 158, 11, 0.18);
    color: #fbbf24;
    border: 1px solid rgba(245, 158, 11, 0.3);
  }

  .disconnect-message-group {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    overflow: hidden;
  }

  .disconnect-title {
    font-weight: 600;
    color: var(--ink);
  }

  .disconnect-text {
    color: var(--body);
    font-size: 12px;
  }

  .disconnect-raw-code {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--mute);
    opacity: 0.8;
  }

  .disconnect-right {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
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

  .action-btn.primary {
    background: #ffffff;
    color: #000000;
    border-color: #ffffff;
    font-weight: 500;
  }

  .action-btn.primary:hover {
    background: #e5e5e5;
    border-color: #e5e5e5;
    color: #000000;
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

  .ironrdp-container {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: #000000;
    position: absolute;
    top: 0;
    left: 0;
    overflow: hidden;
  }

  .ironrdp-container.hidden {
    visibility: hidden;
    pointer-events: none;
  }

  .rdp-canvas {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    display: block;
    margin: auto;
    outline: none;
    user-select: none;
    -webkit-user-select: none;
    cursor: default;
    image-rendering: auto;
  }

  .rdp-canvas:focus {
    outline: none;
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

  .acceleration-pill.iron {
    background: rgba(0, 112, 243, 0.08);
    border-color: rgba(0, 112, 243, 0.25);
    color: #79b8ff;
  }

  .acceleration-pill.iron :global(svg) {
    color: #0070f3;
  }

  .acceleration-pill.iron:hover {
    background: rgba(0, 112, 243, 0.15);
    border-color: #0070f3;
    color: #9eccff;
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

  /* Central Disconnected Alert Card */
  .native-disconnected-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 32px 28px;
    text-align: center;
    max-width: 480px;
    background: rgba(18, 18, 18, 0.75);
    border: 1px solid #262626;
    border-radius: var(--radius-lg);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.6);
    backdrop-filter: blur(12px);
    animation: fadeIn 0.18s ease-out;
  }

  .native-disconnected-card.security,
  .native-disconnected-card.auth {
    border-color: rgba(239, 68, 68, 0.35);
    background: radial-gradient(circle at top, rgba(239, 68, 68, 0.12) 0%, rgba(18, 18, 18, 0.85) 70%);
  }

  .native-disconnected-card.error {
    border-color: rgba(245, 158, 11, 0.35);
    background: radial-gradient(circle at top, rgba(245, 158, 11, 0.12) 0%, rgba(18, 18, 18, 0.85) 70%);
  }

  .card-icon-container {
    width: 52px;
    height: 52px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-full);
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid var(--hairline);
    margin-bottom: 2px;
  }

  .card-icon.security,
  .card-icon.auth {
    color: #f87171;
  }

  .card-icon.error {
    color: #fbbf24;
  }

  .card-icon.info {
    color: var(--mute);
  }

  .card-title {
    font-size: 15px;
    font-weight: 600;
    color: var(--ink);
    margin: 0;
  }

  .card-desc {
    font-size: 12px;
    line-height: 1.5;
    color: var(--body);
    margin: 0;
  }

  .card-error-detail {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    padding: 4px 10px;
    border-radius: var(--radius-sm);
    background: rgba(0, 0, 0, 0.4);
    border: 1px solid #2a2a2a;
    margin-top: 4px;
    max-width: 100%;
    overflow: hidden;
  }

  .detail-label {
    color: var(--mute);
    flex-shrink: 0;
  }

  .detail-code {
    font-family: var(--font-mono);
    color: #f87171;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .card-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 14px;
    flex-wrap: wrap;
    justify-content: center;
  }

  /* mRemoteNG-style Bottom Status Bar */
  .rdp-bottom-status-bar {
    height: 30px;
    min-height: 30px;
    padding: 0 14px;
    background: #0d0d0d;
    border-top: 1px solid #222222;
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 11px;
    z-index: 10;
    user-select: text;
  }

  .rdp-bottom-status-bar.security,
  .rdp-bottom-status-bar.auth {
    background: #180a0a;
    border-top-color: rgba(239, 68, 68, 0.4);
  }

  .rdp-bottom-status-bar.error {
    background: #18120a;
    border-top-color: rgba(245, 158, 11, 0.4);
  }

  .bottom-status-left {
    display: flex;
    align-items: center;
    gap: 7px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .bottom-status-icon.security,
  .bottom-status-icon.auth {
    color: #f87171;
    flex-shrink: 0;
  }

  .bottom-status-icon.error {
    color: #fbbf24;
    flex-shrink: 0;
  }

  .bottom-status-title {
    font-weight: 600;
    color: #ffffff;
    flex-shrink: 0;
  }

  .bottom-status-msg {
    color: var(--body);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .bottom-status-code {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--mute);
    flex-shrink: 0;
  }

  .bottom-status-right {
    flex-shrink: 0;
    margin-left: 12px;
  }

  .bottom-copy-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 10px;
    padding: 2px 7px;
    border-radius: var(--radius-sm);
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid #333333;
    color: #cccccc;
    cursor: pointer;
    transition: all 0.12s ease;
  }

  .bottom-copy-btn:hover {
    background: rgba(255, 255, 255, 0.12);
    color: #ffffff;
    border-color: #555555;
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
