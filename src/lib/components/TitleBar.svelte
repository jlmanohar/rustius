<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onDestroy, onMount } from 'svelte';
  import { appState } from '../stores/appState.svelte';
  import { DisposableStore } from '../utils/disposable';

  let isMaximized = $state(false);
  const disposables = new DisposableStore();

  let activeSession = $derived(
    appState.tabs.find((t) => t.id === appState.activeTabId) || null
  );

  async function updateMaximizedState() {
    try {
      isMaximized = await invoke<boolean>('window_is_maximized');
    } catch {
      // In dev browser fallback
    }
  }

  async function handleMinimize() {
    try {
      await invoke('window_minimize');
    } catch (err) {
      console.error('Failed to minimize window:', err);
    }
  }

  async function handleToggleMaximize() {
    try {
      await invoke('window_toggle_maximize');
      await updateMaximizedState();
    } catch (err) {
      console.error('Failed to toggle maximize:', err);
    }
  }

  async function handleClose() {
    try {
      await invoke('window_close');
    } catch (err) {
      console.error('Failed to close window:', err);
    }
  }

  onMount(() => {
    updateMaximizedState();
    disposables.trackEventListener(window, 'resize', () => {
      updateMaximizedState();
    });
  });

  onDestroy(() => {
    disposables.dispose();
  });
</script>

<header class="titlebar" data-tauri-drag-region>
  <!-- Left: Branding & App title -->
  <div class="titlebar-section titlebar-left" data-tauri-drag-region>
    <img src="/rustius_icon.svg" alt="Rustius" class="titlebar-logo" width="14" height="14" />
    <span class="titlebar-brand-name">Rustius</span>
  </div>

  <!-- Center: Active Tab Context / Draggable Area -->
  <div class="titlebar-section titlebar-center" data-tauri-drag-region>
    {#if activeSession}
      <div class="titlebar-active-pill" data-tauri-drag-region>
        <span class="session-indicator {activeSession.status}"></span>
        <span class="session-name">{activeSession.title}</span>
        {#if activeSession.badge}
          <span class="session-badge">{activeSession.badge}</span>
        {/if}
      </div>
    {/if}
  </div>

  <!-- Right: Window Management Controls -->
  <div class="titlebar-controls" aria-label="Window controls">
    <button
      type="button"
      class="control-btn minimize-btn"
      aria-label="Minimize"
      title="Minimize"
      onclick={handleMinimize}
    >
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <path d="M1 5.5h8" stroke="currentColor" stroke-width="1.1" stroke-linecap="round" />
      </svg>
    </button>

    <button
      type="button"
      class="control-btn maximize-btn"
      aria-label={isMaximized ? 'Restore' : 'Maximize'}
      title={isMaximized ? 'Restore' : 'Maximize'}
      onclick={handleToggleMaximize}
    >
      {#if isMaximized}
        <!-- Restore icon (overlapping squares) -->
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
          <path d="M3 2.5V1.5H8.5V7H7.5" stroke="currentColor" stroke-width="1" />
          <rect x="1.5" y="3" width="5.5" height="5.5" stroke="currentColor" stroke-width="1" />
        </svg>
      {:else}
        <!-- Maximize icon (single square) -->
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
          <rect x="1.5" y="1.5" width="7" height="7" stroke="currentColor" stroke-width="1" />
        </svg>
      {/if}
    </button>

    <button
      type="button"
      class="control-btn close-btn"
      aria-label="Close"
      title="Close"
      onclick={handleClose}
    >
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <path d="M1.5 1.5l7 7M8.5 1.5l-7 7" stroke="currentColor" stroke-width="1.1" stroke-linecap="round" />
      </svg>
    </button>
  </div>
</header>

<style>
  .titlebar {
    height: 32px;
    min-height: 32px;
    width: 100%;
    background: var(--canvas);
    border-bottom: 1px solid var(--hairline);
    display: flex;
    align-items: center;
    justify-content: space-between;
    user-select: none;
    -webkit-user-select: none;
    position: relative;
    z-index: 1000;
  }

  .titlebar-section {
    display: flex;
    align-items: center;
    height: 100%;
  }

  .titlebar-left {
    padding-left: 12px;
    gap: 8px;
    min-width: 160px;
  }

  .titlebar-logo {
    width: 14px;
    height: 14px;
    object-fit: contain;
    pointer-events: none;
    filter: drop-shadow(0 1px 3px rgba(240, 67, 36, 0.3));
  }

  .titlebar-brand-name {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: -0.2px;
    color: var(--ink-secondary);
    pointer-events: none;
  }

  .titlebar-center {
    flex: 1;
    justify-content: center;
    overflow: hidden;
    padding: 0 16px;
  }

  .titlebar-active-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 2px 10px;
    border-radius: var(--radius-pill);
    background: var(--canvas-subtle);
    border: 1px solid var(--hairline-soft);
    font-size: 11px;
    color: var(--body);
    max-width: 380px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .session-indicator {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--body);
    flex-shrink: 0;
  }

  .session-indicator.connected {
    background: #50e3c2;
    box-shadow: 0 0 6px rgba(80, 227, 194, 0.4);
  }

  .session-indicator.connecting {
    background: #f5a623;
  }

  .session-indicator.error,
  .session-indicator.disconnected {
    background: #ee0000;
  }

  .session-name {
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 500;
    color: var(--ink-secondary);
  }

  .session-badge {
    font-family: var(--font-mono);
    font-size: 9px;
    color: var(--mute);
    text-transform: uppercase;
  }

  .titlebar-controls {
    display: flex;
    align-items: center;
    height: 100%;
    margin-left: auto;
  }

  .control-btn {
    width: 46px;
    height: 100%;
    border: none;
    background: transparent;
    color: var(--body);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: default;
    outline: none;
    padding: 0;
    transition: background-color 0.1s ease, color 0.1s ease;
  }

  .control-btn:hover {
    background-color: rgba(255, 255, 255, 0.08);
    color: var(--ink);
  }

  .control-btn:active {
    background-color: rgba(255, 255, 255, 0.14);
  }

  .control-btn.close-btn:hover {
    background-color: #e81123;
    color: #ffffff;
  }

  .control-btn.close-btn:active {
    background-color: #b8101e;
    color: #ffffff;
  }
</style>
