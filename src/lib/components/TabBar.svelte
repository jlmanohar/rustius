<script lang="ts">
  import { appState } from '../stores/appState.svelte';
  import {
    Terminal,
    ArrowLeftRight,
    Monitor,
    X,
    Plus,
    XCircle,
    FileText,
    ChevronLeft,
    ChevronRight,
  } from '@lucide/svelte';
  import { onMount, onDestroy } from 'svelte';
  import { DisposableStore } from '../utils/disposable';

  let contextMenu = $state<{ x: number; y: number; tabId: string } | null>(null);
  let activeConn = $derived(appState.getActiveConnection());
  let hasNotes = $derived(Boolean(activeConn?.notes && activeConn.notes.trim()));

  let tabsContainerRef = $state<HTMLDivElement | null>(null);
  let canScrollLeft = $state(false);
  let canScrollRight = $state(false);
  const disposables = new DisposableStore();

  function updateScrollState() {
    if (!tabsContainerRef) return;
    canScrollLeft = tabsContainerRef.scrollLeft > 4;
    canScrollRight =
      tabsContainerRef.scrollLeft + tabsContainerRef.clientWidth <
      tabsContainerRef.scrollWidth - 4;
  }

  function handleWheel(e: WheelEvent) {
    if (!tabsContainerRef) return;
    if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
      tabsContainerRef.scrollLeft += e.deltaY;
      updateScrollState();
      e.preventDefault();
    }
  }

  function scrollTabs(direction: 'left' | 'right') {
    if (!tabsContainerRef) return;
    const amount = 160;
    tabsContainerRef.scrollBy({
      left: direction === 'left' ? -amount : amount,
      behavior: 'smooth',
    });
    setTimeout(updateScrollState, 250);
  }

  function handleClose(e: MouseEvent, id: string) {
    e.stopPropagation();
    appState.closeTab(id);
  }

  function handleTabContextMenu(e: MouseEvent, tabId: string) {
    e.preventDefault();
    contextMenu = { x: e.clientX, y: e.clientY, tabId };
  }

  function closeContextMenu() {
    contextMenu = null;
  }

  $effect(() => {
    const _ = appState.activeTabId;
    const __ = appState.tabs.length;
    if (tabsContainerRef) {
      requestAnimationFrame(() => {
        const activeEl = tabsContainerRef?.querySelector('.tab-active') as HTMLElement | null;
        if (activeEl) {
          activeEl.scrollIntoView({ behavior: 'smooth', block: 'nearest', inline: 'nearest' });
        }
        updateScrollState();
      });
    }
  });

  onMount(() => {
    updateScrollState();
    if (tabsContainerRef) {
      disposables.trackEventListener(tabsContainerRef, 'scroll', updateScrollState);
      if (window.ResizeObserver) {
        const ro = new ResizeObserver(() => updateScrollState());
        ro.observe(tabsContainerRef);
        disposables.add(() => ro.disconnect());
      }
    }
    disposables.trackEventListener(window, 'resize', updateScrollState);
  });

  onDestroy(() => {
    disposables.dispose();
  });
</script>

<svelte:window onclick={closeContextMenu} />

<div class="tab-strip">
  {#if canScrollLeft}
    <button
      type="button"
      class="tab-scroll-btn left"
      title="Scroll tabs left"
      onclick={() => scrollTabs('left')}
    >
      <ChevronLeft size={13} />
    </button>
  {/if}

  <div
    class="tabs-container"
    bind:this={tabsContainerRef}
    onwheel={handleWheel}
  >
    {#each appState.tabs as tab (tab.id)}
      <div
        class="tab-item {appState.activeTabId === tab.id ? 'tab-active' : ''}"
        role="button"
        tabindex="0"
        onclick={() => (appState.activeTabId = tab.id)}
        onkeydown={(e) => e.key === 'Enter' && (appState.activeTabId = tab.id)}
        oncontextmenu={(e) => handleTabContextMenu(e, tab.id)}
        title={tab.badge && tab.badge !== tab.title ? `${tab.title} (${tab.badge})` : tab.title}
      >
        <span class="tab-icon">
          {#if tab.type === 'ssh'}
            <Terminal size={13} />
          {:else if tab.type === 'sftp'}
            <ArrowLeftRight size={13} />
          {:else if tab.type === 'rdp'}
            <Monitor size={13} />
          {:else}
            <Terminal size={13} />
          {/if}
        </span>

        <span class="tab-label">{tab.title}</span>

        {#if tab.status === 'connecting'}
          <span class="tab-status-dot connecting" title="Connecting..."></span>
        {:else if tab.status === 'error' || tab.status === 'disconnected'}
          <span class="tab-status-dot disconnected" title={tab.status === 'error' ? 'Connection lost' : 'Disconnected'}></span>
        {/if}

        <button
          type="button"
          class="tab-close"
          title="Close tab"
          onclick={(e) => handleClose(e, tab.id)}
        >
          <X size={12} />
        </button>
      </div>
    {/each}

    <!-- Tab Actions -->
    <div class="tab-actions">
      <button
        class="add-btn"
        title="New Local Shell"
        onclick={() => {
          appState.openTab({
            title: 'Local Shell',
            type: 'local_terminal',
            status: 'connected',
            badge: 'POWERSHELL',
          });
        }}
      >
        <Plus size={13} />
      </button>

      {#if appState.tabs.length > 1}
        <button
          class="add-btn"
          title="Close All Tabs"
          onclick={() => appState.closeAllTabs()}
        >
          <XCircle size={13} />
        </button>
      {/if}
    </div>
  </div>

  {#if canScrollRight}
    <button
      type="button"
      class="tab-scroll-btn right"
      title="Scroll tabs right"
      onclick={() => scrollTabs('right')}
    >
      <ChevronRight size={13} />
    </button>
  {/if}

  <div class="strip-right-actions">
    {#if activeConn}
      <button
        class="notes-toggle-btn {appState.isNotesDrawerOpen ? 'active' : ''}"
        title={hasNotes ? 'Connection Notes (Ctrl+Alt+N)' : 'Add Notes (Ctrl+Alt+N)'}
        onclick={() => appState.toggleNotesDrawer()}
      >
        <FileText size={12} />
        <span class="notes-btn-text">Notes</span>
        {#if hasNotes}
          <span class="has-notes-badge" title="Has notes"></span>
        {/if}
      </button>
    {/if}
  </div>
</div>

{#if contextMenu}
  <div
    class="tab-context-menu"
    role="menu"
    tabindex="-1"
    style="left: {contextMenu.x}px; top: {contextMenu.y}px;"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.key === 'Escape' && closeContextMenu()}
  >
    <button
      class="context-menu-item"
      onclick={() => {
        if (contextMenu) appState.closeTab(contextMenu.tabId);
        closeContextMenu();
      }}
    >
      <span>Close Tab</span>
    </button>
    <button
      class="context-menu-item"
      onclick={() => {
        if (contextMenu) appState.closeOtherTabs(contextMenu.tabId);
        closeContextMenu();
      }}
    >
      <span>Close Other Tabs</span>
    </button>
    {#if appState.tabs.some((t) => t.status === 'disconnected')}
      <button
        class="context-menu-item"
        onclick={() => {
          appState.closeDisconnectedTabs();
          closeContextMenu();
        }}
      >
        <span>Close Disconnected Tabs</span>
      </button>
    {/if}
    <div class="menu-divider"></div>
    <button
      class="context-menu-item text-danger"
      onclick={() => {
        appState.closeAllTabs();
        closeContextMenu();
      }}
    >
      <span>Close All Tabs</span>
    </button>
  </div>
{/if}

<style>
  .tab-strip {
    height: 38px;
    min-height: 38px;
    background: var(--canvas);
    border-bottom: 1px solid var(--hairline);
    display: flex;
    align-items: center;
    padding: 0 8px;
    user-select: none;
    min-width: 0;
    width: 100%;
    position: relative;
    overflow: hidden;
    gap: 4px;
  }

  .tabs-container {
    display: flex;
    align-items: center;
    gap: 4px;
    overflow-x: auto;
    overflow-y: hidden;
    flex: 1;
    min-width: 0;
    height: 100%;
    scrollbar-width: none;
    -ms-overflow-style: none;
  }

  .tabs-container::-webkit-scrollbar {
    display: none;
  }

  .tab-item {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 8px 0 10px;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--mute);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    border: 1px solid transparent;
    transition: all 0.12s ease;
    min-width: 70px;
    max-width: 220px;
    flex: 0 1 auto;
    overflow: hidden;
    box-sizing: border-box;
    position: relative;
    user-select: none;
  }

  .tab-item:hover {
    color: var(--ink-secondary);
    background: var(--canvas-hover);
  }

  .tab-active {
    background: var(--canvas-elevated);
    color: var(--ink);
    border-color: var(--hairline);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.4);
  }

  .tab-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    color: inherit;
    flex-shrink: 0;
  }

  .tab-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    flex: 0 1 auto;
  }

  .tab-status-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .tab-status-dot.connecting {
    background: #f5a623;
    box-shadow: 0 0 6px rgba(245, 166, 35, 0.6);
    animation: pulse-dot 1.2s ease-in-out infinite;
  }

  .tab-status-dot.disconnected {
    background: #888888;
  }

  @keyframes pulse-dot {
    0%, 100% {
      opacity: 0.3;
      transform: scale(0.85);
    }
    50% {
      opacity: 1;
      transform: scale(1.15);
    }
  }


  .tab-close {
    background: transparent;
    border: none;
    color: var(--mute);
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    border-radius: 3px;
    cursor: pointer;
    transition: all 0.12s ease;
    margin-left: auto;
    flex-shrink: 0;
    padding: 0;
  }

  .tab-close:hover {
    color: var(--ink);
    background: var(--hairline);
  }

  .add-btn {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--mute);
    cursor: pointer;
    transition: all 0.12s ease;
    flex-shrink: 0;
  }

  .add-btn:hover {
    color: var(--ink);
    background: var(--canvas-hover);
  }

  .tab-actions {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-left: 2px;
    flex-shrink: 0;
  }

  .tab-scroll-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 28px;
    background: var(--canvas-elevated);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-sm);
    color: var(--body);
    cursor: pointer;
    padding: 0;
    flex-shrink: 0;
    transition: all 0.12s ease;
    z-index: 2;
  }

  .tab-scroll-btn:hover {
    color: var(--ink);
    background: var(--canvas-hover);
    border-color: var(--hairline-hover);
  }

  .tab-scroll-btn.left {
    margin-right: 2px;
  }

  .tab-scroll-btn.right {
    margin-left: 2px;
  }

  .tab-context-menu {
    position: fixed;
    z-index: 1000;
    min-width: 170px;
    background: #0a0a0a;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-sm);
    padding: 4px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.5);
    display: flex;
    flex-direction: column;
    animation: fadeIn 0.1s ease-out;
  }

  .context-menu-item {
    display: flex;
    align-items: center;
    width: 100%;
    padding: 6px 10px;
    font-size: 11px;
    font-weight: 500;
    color: var(--ink-secondary);
    background: transparent;
    border: none;
    border-radius: 3px;
    cursor: pointer;
    text-align: left;
    transition: all 0.1s ease;
  }

  .context-menu-item:hover {
    background: var(--canvas-hover);
    color: var(--ink);
  }

  .context-menu-item.text-danger {
    color: #ff4d4d;
  }

  .context-menu-item.text-danger:hover {
    background: rgba(255, 77, 77, 0.1);
    color: #ff4d4d;
  }

  .menu-divider {
    height: 1px;
    background: var(--hairline);
    margin: 3px 0;
  }

  .strip-right-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-left: auto;
    padding-left: 8px;
    flex-shrink: 0;
  }

  .notes-toggle-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 10px;
    border-radius: var(--radius-sm);
    background: var(--canvas-subtle);
    border: 1px solid var(--hairline);
    color: var(--mute);
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s ease;
    user-select: none;
  }

  .notes-toggle-btn:hover {
    color: var(--ink);
    background: var(--canvas-hover);
    border-color: var(--hairline-hover);
  }

  .notes-toggle-btn.active {
    background: var(--canvas-elevated);
    color: var(--ink);
    border-color: var(--ink-secondary);
  }

  .notes-btn-text {
    font-size: 11px;
    letter-spacing: -0.2px;
  }

  .has-notes-badge {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 6px rgba(0, 112, 243, 0.6);
  }
</style>
