<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke, Channel } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { appState } from '../stores/appState.svelte';
  import { DisposableStore } from '../utils/disposable';
  import { RdpWebGlRenderer } from '../utils/rdpWebGlRenderer';
  import type { TabSession, RdpExitDiagnostic, IronRdpPointerBitmap } from '../types';
  import {
    Monitor,
    Maximize2,
    Minimize2,
    RotateCcw,
    X,
    Zap,
    Power,
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
  let exitInfo = $state<RdpExitDiagnostic | null>(null);
  let copiedDetails = $state(false);

  let conn = $derived(appState.findConnection(session.connectionId));

  let webglRenderer: RdpWebGlRenderer | null = null;
  let useWebGl = $state(false);
  let canvasCtx: CanvasRenderingContext2D | null = null;
  let pendingMouseMove: { x: number; y: number } | null = null;
  let mouseMoveRafId: number | null = null;
  let cursorCanvas: HTMLCanvasElement | null = null;
  const cursorCache = new Map<string, string>();

  function updateCursorBitmap(bitmap: IronRdpPointerBitmap) {
    if (!canvasRef) return;
    if (!bitmap.width || !bitmap.height || !bitmap.data) {
      canvasRef.style.cursor = 'default';
      return;
    }

    try {
      const rect = canvasRef.getBoundingClientRect();
      const scaleX = (rect.width > 0 && canvasRef.width > 0) ? rect.width / canvasRef.width : 1;
      const scaleY = (rect.height > 0 && canvasRef.height > 0) ? rect.height / canvasRef.height : 1;
      const scale = (scaleX + scaleY) / 2;

      // Scale cursor if the canvas display is significantly scaled
      const isScaled = Math.abs(scale - 1) > 0.05 && scale > 0.1 && scale < 5;
      const targetW = isScaled ? Math.max(1, Math.round(bitmap.width * scale)) : bitmap.width;
      const targetH = isScaled ? Math.max(1, Math.round(bitmap.height * scale)) : bitmap.height;
      const hx = isScaled
        ? Math.min(Math.max(0, Math.round(bitmap.hotspot_x * scale)), targetW - 1)
        : Math.min(Math.max(0, bitmap.hotspot_x), bitmap.width - 1);
      const hy = isScaled
        ? Math.min(Math.max(0, Math.round(bitmap.hotspot_y * scale)), targetH - 1)
        : Math.min(Math.max(0, bitmap.hotspot_y), bitmap.height - 1);

      const cacheKey = `${targetW}x${targetH}:${hx},${hy}:${bitmap.data.slice(0, 32)}:${bitmap.data.length}`;
      const cached = cursorCache.get(cacheKey);
      if (cached) {
        canvasRef.style.cursor = cached;
        return;
      }

      if (!cursorCanvas) {
        cursorCanvas = document.createElement('canvas');
      }

      const binaryStr = atob(bitmap.data);
      const len = binaryStr.length;
      const bytes = new Uint8ClampedArray(len);
      for (let i = 0; i < len; i++) {
        bytes[i] = binaryStr.charCodeAt(i);
      }

      const rawCanvas = document.createElement('canvas');
      rawCanvas.width = bitmap.width;
      rawCanvas.height = bitmap.height;
      const rawCtx = rawCanvas.getContext('2d');
      if (!rawCtx) return;
      const imgData = new ImageData(bytes, bitmap.width, bitmap.height);
      rawCtx.putImageData(imgData, 0, 0);

      cursorCanvas.width = targetW;
      cursorCanvas.height = targetH;
      const ctx = cursorCanvas.getContext('2d');
      if (!ctx) return;
      ctx.clearRect(0, 0, targetW, targetH);
      ctx.drawImage(rawCanvas, 0, 0, targetW, targetH);

      const dataUrl = cursorCanvas.toDataURL('image/png');
      const cursorVal = `url("${dataUrl}") ${hx} ${hy}, default`;

      if (cursorCache.size >= 64) {
        const oldest = cursorCache.keys().next().value;
        if (oldest !== undefined) cursorCache.delete(oldest);
      }
      cursorCache.set(cacheKey, cursorVal);

      canvasRef.style.cursor = cursorVal;
    } catch (err) {
      console.warn('Failed to update cursor bitmap:', err);
      canvasRef.style.cursor = 'default';
    }
  }

  interface BinaryTile {
    x: number;
    y: number;
    width: number;
    height: number;
    slice: Uint8Array;
  }

  let pendingTiles: BinaryTile[] = [];
  let pendingDesktopWidth = 0;
  let pendingDesktopHeight = 0;
  let frameRafId: number | null = null;

  function initRenderer(): boolean {
    if (!canvasRef) return false;
    if (useWebGl && webglRenderer) return true;
    if (canvasCtx) return false;

    try {
      const renderer = new RdpWebGlRenderer();
      if (renderer.init(canvasRef)) {
        webglRenderer = renderer;
        useWebGl = true;
        canvasCtx = null;
        return true;
      }
      renderer.destroy();
    } catch (err) {
      console.warn('Failed to initialize WebGL2 in RdpView; falling back to 2D canvas:', err);
    }

    useWebGl = false;
    webglRenderer = null;
    canvasCtx = canvasRef.getContext('2d', { alpha: false });
    return false;
  }

  function handleBinaryFrameBatch(raw: ArrayBuffer | Uint8Array | number[]) {
    if (!canvasRef) return;

    let buf: ArrayBuffer;
    let byteOffset = 0;
    let byteLength = 0;

    if (raw instanceof ArrayBuffer) {
      buf = raw;
      byteOffset = 0;
      byteLength = raw.byteLength;
    } else if (ArrayBuffer.isView(raw)) {
      buf = raw.buffer as ArrayBuffer;
      byteOffset = raw.byteOffset;
      byteLength = raw.byteLength;
    } else if (Array.isArray(raw)) {
      const u8 = new Uint8Array(raw);
      buf = u8.buffer;
      byteOffset = 0;
      byteLength = u8.byteLength;
    } else {
      return;
    }

    if (byteLength < 6) return;

    const view = new DataView(buf, byteOffset, byteLength);
    pendingDesktopWidth = view.getUint16(0, true);
    pendingDesktopHeight = view.getUint16(2, true);
    const numTiles = view.getUint16(4, true);

    let offset = 6;
    for (let i = 0; i < numTiles; i++) {
      if (offset + 12 > byteLength) break;
      const x = view.getUint16(offset, true);
      const y = view.getUint16(offset + 2, true);
      const width = view.getUint16(offset + 4, true);
      const height = view.getUint16(offset + 6, true);
      const dataLen = view.getUint32(offset + 8, true);
      offset += 12;

      if (offset + dataLen > byteLength) break;
      if (
        width > 0 &&
        height > 0 &&
        x + width <= pendingDesktopWidth &&
        y + height <= pendingDesktopHeight &&
        dataLen === width * height * 4
      ) {
        const slice = new Uint8Array(buf, byteOffset + offset, dataLen);
        pendingTiles.push({ x, y, width, height, slice });
      }
      offset += dataLen;
    }

    if (frameRafId === null) {
      frameRafId = requestAnimationFrame(flushPendingTiles);
    }
  }

  function flushPendingTiles() {
    frameRafId = null;
    if (!canvasRef || pendingTiles.length === 0) return;

    if (!webglRenderer && !canvasCtx) {
      initRenderer();
    }

    if (canvasRef.width !== pendingDesktopWidth || canvasRef.height !== pendingDesktopHeight) {
      canvasRef.width = pendingDesktopWidth;
      canvasRef.height = pendingDesktopHeight;
      if (useWebGl && webglRenderer) {
        webglRenderer.ensureSize(pendingDesktopWidth, pendingDesktopHeight);
      } else if (!useWebGl) {
        canvasCtx = canvasRef.getContext('2d', { alpha: false });
      }
    }

    const tilesToRender = pendingTiles;
    pendingTiles = [];

    try {
      for (let t = 0; t < tilesToRender.length; t++) {
        const tile = tilesToRender[t];

        if (useWebGl && webglRenderer) {
          webglRenderer.uploadTile(tile.x, tile.y, tile.width, tile.height, tile.slice);
        } else if (canvasCtx) {
          const slice = tile.slice;
          const clamped = new Uint8ClampedArray(slice.length);
          clamped.set(slice);
          // Swizzle BGR[0] to RGB[255] for 2D canvas fallback
          for (let p = 0; p < clamped.length; p += 4) {
            const b = clamped[p];
            clamped[p] = clamped[p + 2]; // Red
            clamped[p + 2] = b;          // Blue
            clamped[p + 3] = 255;        // Alpha
          }
          const imgData = new ImageData(clamped, tile.width, tile.height);
          canvasCtx.putImageData(imgData, tile.x, tile.y);
        }
      }

      if (useWebGl && webglRenderer) {
        webglRenderer.render();
      }
    } catch (err) {
      console.error('Failed to render IronRDP batch tiles:', err);
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
    if (!isConnected) return;
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
    if (!isConnected) return;
    canvasRef?.focus();
    const coords = getCanvasMouseCoords(e);
    invoke('ironrdp_mouse_button', {
      tabId: session.id,
      button: e.button,
      pressed: true,
      x: coords?.x,
      y: coords?.y,
    }).catch(() => {});
  }

  function handleMouseUp(e: MouseEvent) {
    if (!isConnected) return;
    const coords = getCanvasMouseCoords(e);
    invoke('ironrdp_mouse_button', {
      tabId: session.id,
      button: e.button,
      pressed: false,
      x: coords?.x,
      y: coords?.y,
    }).catch(() => {});
  }

  function handleContextMenu(e: MouseEvent) {
    e.preventDefault();
  }

  let accumulatedWheelY = 0;
  let accumulatedWheelX = 0;

  function handleWheel(e: WheelEvent) {
    if (!isConnected) return;
    e.preventDefault();

    let dy = e.deltaY;
    let dx = e.deltaX;
    if (e.deltaMode === 1) {
      dy *= 20;
      dx *= 20;
    } else if (e.deltaMode === 2) {
      dy *= 100;
      dx *= 100;
    }

    const isVertical = Math.abs(dy) >= Math.abs(dx);
    if (isVertical) {
      accumulatedWheelY -= dy;
      if (Math.abs(accumulatedWheelY) >= 20) {
        const notches = Math.trunc(accumulatedWheelY / 40);
        if (notches !== 0) {
          const rotationUnits = notches * 120;
          accumulatedWheelY -= notches * 40;
          invoke('ironrdp_mouse_wheel', {
            tabId: session.id,
            isVertical: true,
            rotationUnits,
          }).catch(() => {});
        }
      }
    } else {
      accumulatedWheelX -= dx;
      if (Math.abs(accumulatedWheelX) >= 20) {
        const notches = Math.trunc(accumulatedWheelX / 40);
        if (notches !== 0) {
          const rotationUnits = notches * 120;
          accumulatedWheelX -= notches * 40;
          invoke('ironrdp_mouse_wheel', {
            tabId: session.id,
            isVertical: false,
            rotationUnits,
          }).catch(() => {});
        }
      }
    }
  }

  const pressedKeys = new Set<string>();

  function handleKeyDown(e: KeyboardEvent) {
    if (!isConnected) return;
    if (e.code !== 'F12') {
      e.preventDefault();
    }
    pressedKeys.add(e.code);
    invoke('ironrdp_key_event', {
      tabId: session.id,
      code: e.code,
      pressed: true,
    }).catch(() => {});
  }

  function handleKeyUp(e: KeyboardEvent) {
    if (!isConnected) return;
    if (e.code !== 'F12') {
      e.preventDefault();
    }
    pressedKeys.delete(e.code);
    invoke('ironrdp_key_event', {
      tabId: session.id,
      code: e.code,
      pressed: false,
    }).catch(() => {});
  }

  function releaseAllKeys() {
    if (!isConnected) return;
    if (pressedKeys.size > 0) {
      for (const code of pressedKeys) {
        invoke('ironrdp_key_event', {
          tabId: session.id,
          code,
          pressed: false,
        }).catch(() => {});
      }
      pressedKeys.clear();
    }
    invoke('ironrdp_release_all', { tabId: session.id }).catch(() => {});
  }

  async function sendCtrlAltDel() {
    if (!isConnected) return;
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
      lines.push(`IronRDP Diagnostic: ${exitInfo.raw_error}`);
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
    const resMode = rdpCfg?.resolution_mode || appState.config.app_settings?.default_rdp_resolution_mode || 'auto';

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
    const cfgScale = rdpCfg?.scale_factor ?? (appState.config.app_settings?.default_rdp_scale_factor ?? 0);
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

  let ironResizeTimer: any = null;
  let lastIronRdpWidth = 0;
  let lastIronRdpHeight = 0;

  function handleIronRdpResize() {
    if (!isConnected || isDisconnected || !viewportRef) return;
    if (appState.activeTabId !== session.id) return;
    if (ironResizeTimer) {
      clearTimeout(ironResizeTimer);
    }
    ironResizeTimer = setTimeout(async () => {
      ironResizeTimer = null;
      if (!isConnected || isDisconnected || !viewportRef) return;
      if (appState.activeTabId !== session.id) return;
      const bounds = getFittedBounds();
      if (!bounds) return;

      const dwDiff = Math.abs(bounds.desktopWidth - lastIronRdpWidth);
      const dhDiff = Math.abs(bounds.desktopHeight - lastIronRdpHeight);
      if (
        (dwDiff >= 12 || dhDiff >= 12 || lastIronRdpWidth === 0) &&
        bounds.desktopWidth > 0 &&
        bounds.desktopHeight > 0
      ) {
        try {
          await invoke('ironrdp_resize', {
            tabId: session.id,
            width: bounds.desktopWidth,
            height: bounds.desktopHeight,
            scaleFactor: bounds.scaleDesktop,
          });
          lastIronRdpWidth = bounds.desktopWidth;
          lastIronRdpHeight = bounds.desktopHeight;
        } catch (err) {
          console.warn('[RDP] Dynamic resize failed:', err);
        }
      }
    }, 350);
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
    if (frameRafId !== null) {
      cancelAnimationFrame(frameRafId);
      frameRafId = null;
    }
    pendingTiles = [];

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
          const engineName = 'IronRDP';
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

      const onFrame = new Channel<ArrayBuffer>((payload) => {
        if (currentGen !== sessionGeneration || componentDisposables.isDisposed) return;
        handleBinaryFrameBatch(payload);
      });

      const unlistenConnected = await listen(`rdp-connected-${session.id}`, () => {
        if (currentGen !== sessionGeneration || componentDisposables.isDisposed) return;
        isConnected = true;
        isConnecting = false;
        isDisconnected = false;
        session.status = 'connected';
        initRenderer();
        setTimeout(() => {
          canvasRef?.focus();
        }, 100);
      });
      sessionDisposables.add(unlistenConnected);

      const unlistenPtrDefault = await listen(`rdp-pointer-default-${session.id}`, () => {
        if (currentGen !== sessionGeneration || componentDisposables.isDisposed) return;
        if (canvasRef) canvasRef.style.cursor = 'default';
      });
      sessionDisposables.add(unlistenPtrDefault);

      const unlistenPtrHidden = await listen(`rdp-pointer-hidden-${session.id}`, () => {
        if (currentGen !== sessionGeneration || componentDisposables.isDisposed) return;
        if (canvasRef) canvasRef.style.cursor = 'none';
      });
      sessionDisposables.add(unlistenPtrHidden);

      const unlistenPtrBitmap = await listen<IronRdpPointerBitmap>(`rdp-pointer-bitmap-${session.id}`, (event) => {
        if (currentGen !== sessionGeneration || componentDisposables.isDisposed) return;
        updateCursorBitmap(event.payload);
      });
      sessionDisposables.add(unlistenPtrBitmap);

      await invoke('ironrdp_connect', {
        tabId: session.id,
        connectionId: session.connectionId,
        credentialsOverride: session.credentialsOverride,
        width: bounds.desktopWidth,
        height: bounds.desktopHeight,
        scaleFactor: bounds.scaleDesktop,
        onFrame,
      });

      if (currentGen !== sessionGeneration || componentDisposables.isDisposed) {
        invoke('ironrdp_disconnect', { tabId: session.id }).catch(() => {});
        return;
      }

      lastIronRdpWidth = bounds.desktopWidth;
      lastIronRdpHeight = bounds.desktopHeight;

      if (window.ResizeObserver && viewportRef) {
        const ro = new ResizeObserver(() => {
          handleIronRdpResize();
        });
        ro.observe(viewportRef);
        sessionDisposables.add(() => ro.disconnect());
      }
      sessionDisposables.trackEventListener(window, 'resize', handleIronRdpResize);
      sessionDisposables.trackEventListener(window, 'blur', releaseAllKeys);
      sessionDisposables.trackEventListener(document, 'visibilitychange', () => {
        if (document.hidden) {
          releaseAllKeys();
        }
      });
      sessionDisposables.add(() => {
        if (ironResizeTimer) {
          clearTimeout(ironResizeTimer);
          ironResizeTimer = null;
        }
        releaseAllKeys();
      });
    } catch (err: any) {
      console.error('IronRDP connection error:', err);
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
    if (canvasRef) canvasRef.style.cursor = 'default';
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
    if (isConnected && isTabActive) {
      requestAnimationFrame(() => {
        canvasRef?.focus();
      });
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
    if (frameRafId !== null) {
      cancelAnimationFrame(frameRafId);
    }
    pendingTiles = [];
    if (webglRenderer) {
      webglRenderer.destroy();
      webglRenderer = null;
    }
    useWebGl = false;
    canvasCtx = null;
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

      <span
        class="acceleration-pill iron"
        class:webgl={useWebGl}
        title={useWebGl
          ? 'Hardware-Accelerated WebGL2 GPU Texture Pipeline'
          : 'Pure Rust IronRDP Engine with 2D Canvas Fallback'}
      >
        {#if useWebGl}
          <Zap size={11} />
          <span>IronRDP · WebGL2 GPU</span>
        {:else}
          <Shield size={11} />
          <span>IronRDP · Pure Rust</span>
        {/if}
      </span>
    </div>

    <div class="header-right">
      {#if isConnected || isConnecting}
        {#if isConnected}
          <button
            class="ctrl-btn"
            onclick={sendCtrlAltDel}
            title="Send Ctrl+Alt+Del to remote session"
          >
            <Key size={12} />
            <span>Ctrl+Alt+Del</span>
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

    {#if isConnecting}
      <div class="native-connecting-msg">
        <span class="status-indicator connecting"></span>
        <span>Initializing secure IronRDP session...</span>
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
            <span class="detail-label">IronRDP Diagnostic:</span>
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
    cursor: default;
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

  .acceleration-pill.iron.webgl {
    background: rgba(0, 112, 243, 0.12);
    border-color: rgba(0, 112, 243, 0.45);
    color: #58a6ff;
  }

  .acceleration-pill.iron.webgl:hover {
    background: rgba(0, 112, 243, 0.2);
    border-color: #0070f3;
    color: #79b8ff;
  }

  .native-connecting-msg {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--body);
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
