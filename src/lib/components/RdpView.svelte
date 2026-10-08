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
    Cpu,
    Sparkles,
    Shield,
    Zap
  } from '@lucide/svelte';

  interface RdpFramePayload {
    x: number;
    y: number;
    width: number;
    height: number;
    b64?: string;
    data?: number[];
  }

  let { session = $bindable() }: { session: TabSession } = $props();

  let containerRef: HTMLDivElement | null = $state(null);
  let viewportRef: HTMLDivElement | null = $state(null);
  let canvasRef: HTMLCanvasElement | null = $state(null);
  let ctx: CanvasRenderingContext2D | null = null;
  let isFreeRdpNative = $state(false);

  let desktopWidth = $state(1280);
  let desktopHeight = $state(800);
  let isConnecting = $state(true);
  let isConnected = $state(false);
  let isDisconnected = $state(false);
  let disconnectReason = $state('');
  let isFullscreen = $state(false);
  let scaleMode = $state<'fit' | 'actual'>('fit');
  let framesReceived = $state(0);
  let fps = $state(0);
  let isTauriLive = false;

  const componentDisposables = new DisposableStore();
  const sessionDisposables = new DisposableStore();
  let sessionGeneration = 0;

  // Key code mapping to standard RDP / PC scancodes
  const KEY_SCANCODES: Record<string, number> = {
    Escape: 0x01,
    Digit1: 0x02, Digit2: 0x03, Digit3: 0x04, Digit4: 0x05, Digit5: 0x06,
    Digit6: 0x07, Digit7: 0x08, Digit8: 0x09, Digit9: 0x0a, Digit0: 0x0b,
    Minus: 0x0c, Equal: 0x0d, Backspace: 0x0e, Tab: 0x0f,
    KeyQ: 0x10, KeyW: 0x11, KeyE: 0x12, KeyR: 0x13, KeyT: 0x14,
    KeyY: 0x15, KeyU: 0x16, KeyI: 0x17, KeyO: 0x18, KeyP: 0x19,
    BracketLeft: 0x1a, BracketRight: 0x1b, Enter: 0x1c, ControlLeft: 0x1d,
    KeyA: 0x1e, KeyS: 0x1f, KeyD: 0x20, KeyF: 0x21, KeyG: 0x22,
    KeyH: 0x23, KeyJ: 0x24, KeyK: 0x25, KeyL: 0x26, Semicolon: 0x27,
    Quote: 0x28, Backquote: 0x29, ShiftLeft: 0x2a, Backslash: 0x2b,
    KeyZ: 0x2c, KeyX: 0x2d, KeyC: 0x2e, KeyV: 0x2f, KeyB: 0x30,
    KeyN: 0x31, KeyM: 0x32, Comma: 0x33, Period: 0x34, Slash: 0x35,
    ShiftRight: 0x36, NumpadMultiply: 0x37, AltLeft: 0x38, Space: 0x39,
    CapsLock: 0x3a, F1: 0x3b, F2: 0x3c, F3: 0x3d, F4: 0x3e, F5: 0x3f,
    F6: 0x40, F7: 0x41, F8: 0x42, F9: 0x43, F10: 0x44, NumLock: 0x45,
    ScrollLock: 0x46, Numpad7: 0x47, Numpad8: 0x48, Numpad9: 0x49,
    NumpadSubtract: 0x4a, Numpad4: 0x4b, Numpad5: 0x4c, Numpad6: 0x4d,
    NumpadAdd: 0x4e, Numpad1: 0x4f, Numpad2: 0x50, Numpad3: 0x51,
    Numpad0: 0x52, NumpadDecimal: 0x53, F11: 0x57, F12: 0x58,
    ArrowUp: 0x48, ArrowLeft: 0x4b, ArrowRight: 0x4d, ArrowDown: 0x50,
    Delete: 0x53, Home: 0x47, End: 0x4f, PageUp: 0x49, PageDown: 0x51,
    Insert: 0x52, ControlRight: 0x1d, AltRight: 0x38,
  };

  function getScancode(e: KeyboardEvent): number {
    if (KEY_SCANCODES[e.code]) {
      return KEY_SCANCODES[e.code];
    }
    // Fallback based on which key is pressed
    return (e.keyCode & 0xff) || 0x39;
  }

  // FPS calculation ticker
  let lastFrameTime = Date.now();
  let frameCountThisSec = 0;

  function recordFrameRendered() {
    framesReceived++;
    frameCountThisSec++;
    const now = Date.now();
    if (now - lastFrameTime >= 1000) {
      fps = frameCountThisSec;
      frameCountThisSec = 0;
      lastFrameTime = now;
    }
  }

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
    if (isTauri && session.connectionId) {
      try {
        const nativeOk = await startFreeRdpSession(currentGen);
        if (nativeOk) return;
      } catch (nativeErr) {
        console.warn('FreeRDP native connection attempt failed, falling back to IronRDP canvas:', nativeErr);
      }
    }

    await startCanvasSession(currentGen);
  }

  async function startFreeRdpSession(currentGen: number): Promise<boolean> {
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
      disconnectReason = event.payload === 0 ? 'FreeRDP session ended by user or server' : `FreeRDP exited with code ${event.payload}`;
    });

    if (currentGen !== sessionGeneration || componentDisposables.isDisposed) {
      unlistenExit();
      return false;
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
      return false;
    }

    isFreeRdpNative = true;
    isConnected = true;
    isConnecting = false;
    isDisconnected = false;
    session.status = 'connected';
    isTauriLive = true;

    // 4. Setup viewport bounds sync on container resize or window resize
    const syncBounds = () => {
      if (!isFreeRdpNative || !viewportRef || isDisconnected) return;
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

    return true;
  }

  async function startCanvasSession(currentGen: number) {
    if (componentDisposables.isDisposed || currentGen !== sessionGeneration) return;
    isFreeRdpNative = false;

    try {
      // 1. Listen for incoming RDP frame updates from Rustius backend
      const unlistenFrame = await listen<RdpFramePayload>(`rdp-frame-${session.id}`, (event) => {
        if (!canvasRef) return;
        if (!ctx) ctx = canvasRef.getContext('2d', { alpha: false });
        if (!ctx) return;
        const frame = event.payload;
        if (!frame || (!frame.b64 && !frame.data)) return;

        // Auto-adjust canvas dimensions if first full frame arrives
        if (frame.x === 0 && frame.y === 0 && frame.width > 0 && frame.height > 0) {
          if (desktopWidth !== frame.width || desktopHeight !== frame.height) {
            desktopWidth = frame.width;
            desktopHeight = frame.height;
            canvasRef.width = frame.width;
            canvasRef.height = frame.height;
          }
        }

        try {
          let clamped: Uint8ClampedArray<ArrayBuffer>;
          if (frame.b64) {
            const binary = atob(frame.b64);
            const len = binary.length;
            clamped = new Uint8ClampedArray(len);
            for (let i = 0; i < len; i++) {
              clamped[i] = binary.charCodeAt(i);
            }
          } else if (frame.data) {
            clamped = new Uint8ClampedArray(frame.data);
          } else {
            return;
          }

          const imgData = new ImageData(clamped, frame.width, frame.height);
          ctx.putImageData(imgData, frame.x, frame.y);
          recordFrameRendered();
          if (!isConnected) {
            isConnected = true;
            isConnecting = false;
            session.status = 'connected';
          }
        } catch (renderErr) {
          console.error('Error rendering RDP frame:', renderErr);
        }
      });

      if (currentGen !== sessionGeneration || componentDisposables.isDisposed) {
        unlistenFrame();
        return;
      }
      sessionDisposables.add(unlistenFrame);

      // 2. Listen for RDP session termination event
      const unlistenExit = await listen<number>(`rdp-exit-${session.id}`, (event) => {
        if (currentGen !== sessionGeneration || componentDisposables.isDisposed) return;
        isConnected = false;
        isConnecting = false;
        isDisconnected = true;
        session.status = 'disconnected';
        disconnectReason = event.payload === 0 ? 'Session ended by server or user' : `Exited with code ${event.payload}`;
      });

      if (currentGen !== sessionGeneration || componentDisposables.isDisposed) {
        unlistenExit();
        return;
      }
      sessionDisposables.add(unlistenExit);

      // 3. Initiate backend RDP connection via Tauri invoke
      if (session.connectionId) {
        await invoke('rdp_connect', {
          tabId: session.id,
          connectionId: session.connectionId,
        });
      } else {
        throw new Error('No connectionId specified for RDP tab');
      }

      if (currentGen !== sessionGeneration || componentDisposables.isDisposed) {
        invoke('rdp_disconnect', { tabId: session.id }).catch(() => {});
        return;
      }

      isTauriLive = true;
    } catch (err: any) {
      console.warn('RDP connection failed or running in preview mode:', err);
      if (currentGen !== sessionGeneration || componentDisposables.isDisposed) return;

      const isTauri = typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
      if (isTauri && session.connectionId) {
        isConnected = false;
        isConnecting = false;
        isDisconnected = true;
        session.status = 'error';
        disconnectReason = err?.message || String(err);
      } else {
        // Fallback simulated interactive desktop for web preview
        setupFallbackCanvas();
      }
    }
  }

  function setupFallbackCanvas() {
    if (!canvasRef || !ctx) return;
    isConnecting = false;
    isConnected = true;
    isDisconnected = false;
    session.status = 'connected';

    canvasRef.width = desktopWidth;
    canvasRef.height = desktopHeight;

    // Draw simulated modern desktop
    drawSimulatedDesktop();
  }

  function drawSimulatedDesktop() {
    if (!canvasRef || !ctx) return;
    const w = canvasRef.width;
    const h = canvasRef.height;

    // Dark slate gradient
    const grad = ctx.createLinearGradient(0, 0, w, h);
    grad.addColorStop(0, '#0f172a');
    grad.addColorStop(0.5, '#090d16');
    grad.addColorStop(1, '#020408');
    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, w, h);

    // Subtle grid pattern
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.03)';
    ctx.lineWidth = 1;
    for (let x = 0; x < w; x += 40) {
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, h);
      ctx.stroke();
    }
    for (let y = 0; y < h; y += 40) {
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(w, y);
      ctx.stroke();
    }

    // Windows / Desktop status taskbar at bottom
    ctx.fillStyle = 'rgba(15, 23, 42, 0.85)';
    ctx.fillRect(0, h - 44, w, 44);
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.08)';
    ctx.strokeRect(0, h - 44, w, 1);

    // Taskbar brand pill
    ctx.fillStyle = '#0070f3';
    ctx.beginPath();
    ctx.roundRect(14, h - 34, 24, 24, 6);
    ctx.fill();

    // Taskbar text
    ctx.font = '12px "Geist Mono", monospace';
    ctx.fillStyle = '#ededed';
    ctx.fillText('Rustius IronRDP Session', 48, h - 19);

    const now = new Date();
    const timeStr = now.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    ctx.fillStyle = '#888888';
    ctx.fillText(`${timeStr} · 32-bit RGBA`, w - 160, h - 19);

    // Center splash card
    const cardW = 460;
    const cardH = 200;
    const cardX = (w - cardW) / 2;
    const cardY = (h - cardH) / 2 - 20;

    ctx.fillStyle = 'rgba(10, 10, 10, 0.75)';
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.1)';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.roundRect(cardX, cardY, cardW, cardH, 12);
    ctx.fill();
    ctx.stroke();

    ctx.fillStyle = '#ffffff';
    ctx.font = '600 16px "Geist", system-ui, sans-serif';
    ctx.textAlign = 'center';
    ctx.fillText(session.title || 'Remote Desktop', w / 2, cardY + 50);

    ctx.fillStyle = '#00dfd8';
    ctx.font = '12px "Geist Mono", monospace';
    ctx.fillText(`${session.badge || 'RDP Client'} · Ready for Input`, w / 2, cardY + 80);

    ctx.fillStyle = '#888888';
    ctx.font = '12px "Geist", system-ui, sans-serif';
    ctx.fillText('Click inside the canvas to capture mouse & keyboard input', w / 2, cardY + 120);

    ctx.fillStyle = 'rgba(255, 255, 255, 0.08)';
    ctx.beginPath();
    ctx.roundRect(cardX + 40, cardY + 142, cardW - 80, 28, 6);
    ctx.fill();

    ctx.fillStyle = '#ededed';
    ctx.font = '11px "Geist Mono", monospace';
    ctx.fillText('IronRDP Native Graphics & Input Subsystem Active', w / 2, cardY + 160);

    ctx.textAlign = 'start';
  }

  // Mouse coordinate mapping
  function getCanvasCoords(e: MouseEvent): { x: number; y: number } {
    if (!canvasRef) return { x: 0, y: 0 };
    const rect = canvasRef.getBoundingClientRect();
    const scaleX = canvasRef.width / rect.width;
    const scaleY = canvasRef.height / rect.height;
    const x = Math.round((e.clientX - rect.left) * scaleX);
    const y = Math.round((e.clientY - rect.top) * scaleY);
    return {
      x: Math.max(0, Math.min(canvasRef.width - 1, x)),
      y: Math.max(0, Math.min(canvasRef.height - 1, y)),
    };
  }

  let mouseThrottleTimer: number | null = null;
  function handleMouseMove(e: MouseEvent) {
    if (isDisconnected || !isTauriLive) return;
    const coords = getCanvasCoords(e);

    // Throttle high-frequency move events (~60Hz)
    if (mouseThrottleTimer === null) {
      mouseThrottleTimer = window.requestAnimationFrame(() => {
        mouseThrottleTimer = null;
        invoke('rdp_mouse_event', {
          tabId: session.id,
          x: coords.x,
          y: coords.y,
          buttons: e.buttons,
          wheel: 0,
        }).catch(() => {});
      });
    }
  }

  function handleMouseDown(e: MouseEvent) {
    if (isDisconnected) return;
    canvasRef?.focus();
    const coords = getCanvasCoords(e);
    if (isTauriLive) {
      invoke('rdp_mouse_event', {
        tabId: session.id,
        x: coords.x,
        y: coords.y,
        buttons: e.buttons,
        wheel: 0,
      }).catch(() => {});
    }
  }

  function handleMouseUp(e: MouseEvent) {
    if (isDisconnected) return;
    const coords = getCanvasCoords(e);
    if (isTauriLive) {
      invoke('rdp_mouse_event', {
        tabId: session.id,
        x: coords.x,
        y: coords.y,
        buttons: e.buttons,
        wheel: 0,
      }).catch(() => {});
    }
  }

  function handleWheel(e: WheelEvent) {
    if (isDisconnected) return;
    e.preventDefault();
    const coords = getCanvasCoords(e);
    const wheel = e.deltaY < 0 ? 1 : -1;
    if (isTauriLive) {
      invoke('rdp_mouse_event', {
        tabId: session.id,
        x: coords.x,
        y: coords.y,
        buttons: e.buttons,
        wheel,
      }).catch(() => {});
    }
  }

  function handleContextMenu(e: MouseEvent) {
    // Prevent browser context menu so right-click is routed directly into RDP
    e.preventDefault();
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (isDisconnected) return;

    // Prevent default browser shortcuts when canvas has focus
    if (['Tab', 'Backspace', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Space'].includes(e.code) || e.altKey) {
      e.preventDefault();
    }

    const scancode = getScancode(e);
    if (isTauriLive) {
      invoke('rdp_key_event', {
        tabId: session.id,
        keyCode: scancode,
        pressed: true,
      }).catch(() => {});
    }
  }

  function handleKeyUp(e: KeyboardEvent) {
    if (isDisconnected) return;
    const scancode = getScancode(e);
    if (isTauriLive) {
      invoke('rdp_key_event', {
        tabId: session.id,
        keyCode: scancode,
        pressed: false,
      }).catch(() => {});
    }
  }

  // Ctrl+Alt+Del helper
  async function sendCtrlAltDel() {
    if (isDisconnected || !isTauriLive) return;
    try {
      // Press Ctrl, Alt, Del
      await invoke('rdp_key_event', { tabId: session.id, keyCode: 0x1d, pressed: true }); // Ctrl
      await invoke('rdp_key_event', { tabId: session.id, keyCode: 0x38, pressed: true }); // Alt
      await invoke('rdp_key_event', { tabId: session.id, keyCode: 0x53, pressed: true }); // Del
      setTimeout(async () => {
        await invoke('rdp_key_event', { tabId: session.id, keyCode: 0x53, pressed: false });
        await invoke('rdp_key_event', { tabId: session.id, keyCode: 0x38, pressed: false });
        await invoke('rdp_key_event', { tabId: session.id, keyCode: 0x1d, pressed: false });
      }, 80);
    } catch (err) {
      console.error('Failed to send Ctrl+Alt+Del:', err);
    }
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

  function toggleScaleMode() {
    scaleMode = scaleMode === 'fit' ? 'actual' : 'fit';
  }

  async function reconnect() {
    if (isConnecting || componentDisposables.isDisposed) return;
    isDisconnected = false;
    isConnected = false;
    isConnecting = true;
    disconnectReason = '';
    if (isFreeRdpNative) {
      await invoke('freerdp_disconnect', { tabId: session.id }).catch(() => {});
      isFreeRdpNative = false;
    }
    if (canvasRef && ctx) {
      ctx.clearRect(0, 0, canvasRef.width, canvasRef.height);
    }
    await startSession();
  }

  function handleCloseTab() {
    appState.closeTab(session.id);
  }

  $effect(() => {
    const isTabActive = appState.activeTabId === session.id;
    if (isFreeRdpNative && isConnected) {
      invoke('freerdp_set_visibility', { tabId: session.id, visible: isTabActive }).catch(() => {});
      if (isTabActive) {
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
    if (canvasRef) {
      ctx = canvasRef.getContext('2d', { alpha: false });
    }

    // Track fullscreen change event
    const handleFsChange = () => {
      isFullscreen = !!document.fullscreenElement;
    };
    componentDisposables.trackEventListener(document, 'fullscreenchange', handleFsChange);

    await startSession();
  });

  onDestroy(() => {
    sessionDisposables.dispose();
    componentDisposables.dispose();
    if (mouseThrottleTimer !== null) {
      window.cancelAnimationFrame(mouseThrottleTimer);
    }
    if (isFreeRdpNative) {
      invoke('freerdp_disconnect', { tabId: session.id }).catch(() => {});
    } else if (isTauriLive) {
      invoke('rdp_disconnect', { tabId: session.id }).catch(() => {});
    }
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

      {#if isFreeRdpNative}
        <span class="acceleration-pill native" title="Direct3D Hardware-Accelerated FreeRDP Engine">
          <Zap size={11} />
          <span>Direct3D 60+ FPS</span>
        </span>
      {:else}
        <span class="acceleration-pill canvas" title="Pure Rust IronRDP Software Canvas Engine">
          <Cpu size={11} />
          <span>IronRDP Canvas</span>
        </span>
      {/if}

      <span class="mono-eyebrow">{desktopWidth}×{desktopHeight} · 32-BIT</span>
      {#if fps > 0 && !isFreeRdpNative}
        <span class="fps-badge">{fps} FPS</span>
      {/if}
    </div>

    <div class="header-right">
      <button
        class="ctrl-btn"
        onclick={sendCtrlAltDel}
        title="Send Ctrl+Alt+Del to Remote Session"
        disabled={isDisconnected}
      >
        <Shield size={12} />
        <span>Ctrl+Alt+Del</span>
      </button>

      <button
        class="ctrl-btn"
        onclick={toggleScaleMode}
        title="Toggle between Fit to Window and 100% Native Resolution"
      >
        <span>{scaleMode === 'fit' ? '100% Zoom' : 'Fit Window'}</span>
      </button>

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

  <!-- RDP Viewport Stage Surface -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="rdp-viewport {scaleMode}" bind:this={viewportRef}>
    {#if isFreeRdpNative}
      <div class="freerdp-native-backdrop">
        {#if isConnecting}
          <div class="native-connecting-msg">
            <span class="status-indicator connecting"></span>
            <span>Initializing Direct3D hardware-accelerated remote desktop...</span>
          </div>
        {/if}
      </div>
    {:else}
      <canvas
        bind:this={canvasRef}
        class="rdp-canvas"
        tabindex="0"
        onmousemove={handleMouseMove}
        onmousedown={handleMouseDown}
        onmouseup={handleMouseUp}
        onwheel={handleWheel}
        oncontextmenu={handleContextMenu}
        onkeydown={handleKeyDown}
        onkeyup={handleKeyUp}
      ></canvas>
    {/if}
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
  }

  .status-indicator.online {
    background: var(--accent-green);
    box-shadow: 0 0 6px rgba(80, 227, 194, 0.4);
  }

  .status-indicator.connecting {
    background: var(--accent-amber);
    animation: pulse 1.2s infinite ease-in-out;
  }

  .status-indicator.offline {
    background: var(--accent-red);
  }

  @keyframes pulse {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 1; }
  }

  .status-label {
    font-size: 11px;
    color: var(--mute);
  }

  .mono-eyebrow {
    font-size: 10px;
    letter-spacing: 0.05em;
    color: var(--mute);
    margin-left: 4px;
  }

  .fps-badge {
    font-family: var(--font-mono);
    font-size: 10px;
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    background: rgba(255, 255, 255, 0.06);
    color: var(--body);
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

  .ctrl-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
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
    padding: 3px 9px;
    font-size: 11px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: all 0.12s ease;
    border: 1px solid #333333;
    background: #141414;
    color: #ededed;
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

  /* Viewport & Canvas */
  .rdp-viewport {
    flex: 1;
    position: relative;
    background: #000000;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: auto;
  }

  .rdp-viewport.fit {
    overflow: hidden;
  }

  .rdp-canvas {
    outline: none;
    box-shadow: 0 4px 24px rgba(0, 0, 0, 0.6);
    cursor: crosshair;
    image-rendering: -webkit-optimize-contrast;
    image-rendering: crisp-edges;
  }

  .rdp-viewport.fit .rdp-canvas {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
  }

  .rdp-viewport.actual .rdp-canvas {
    width: auto;
    height: auto;
  }

  .acceleration-pill {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 10px;
    font-family: var(--font-mono);
    padding: 2px 7px;
    border-radius: var(--radius-full);
    border: 1px solid transparent;
    user-select: none;
  }

  .acceleration-pill.native {
    background: rgba(0, 223, 216, 0.12);
    border-color: rgba(0, 223, 216, 0.35);
    color: #00dfd8;
  }

  .acceleration-pill.canvas {
    background: rgba(255, 255, 255, 0.06);
    border-color: var(--hairline);
    color: var(--mute);
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

  :global(.rdp-icon) {
    color: var(--mute);
  }
</style>
