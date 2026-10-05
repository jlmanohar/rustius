<script lang="ts">
  import { appState } from '../stores/appState.svelte';
  import {
    Terminal,
    ArrowLeftRight,
    Monitor,
    X,
    Plus
  } from '@lucide/svelte';

  function handleClose(e: MouseEvent, id: string) {
    e.stopPropagation();
    appState.closeTab(id);
  }
</script>

<div class="tab-strip">
  <div class="tabs-container">
    {#each appState.tabs as tab (tab.id)}
      <div
        class="tab-item {appState.activeTabId === tab.id ? 'tab-active' : ''}"
        role="button"
        tabindex="0"
        onclick={() => (appState.activeTabId = tab.id)}
        onkeydown={(e) => e.key === 'Enter' && (appState.activeTabId = tab.id)}
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

        {#if tab.badge}
          <span class="tab-mono-badge">{tab.badge}</span>
        {/if}

        <button
          class="tab-close"
          title="Close"
          onclick={(e) => handleClose(e, tab.id)}
        >
          <X size={12} />
        </button>
      </div>
    {/each}

    <!-- Add Tab Button -->
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
  </div>
</div>

<style>
  .tab-strip {
    height: 38px;
    background: var(--canvas);
    border-bottom: 1px solid var(--hairline);
    display: flex;
    align-items: center;
    padding: 0 8px;
    user-select: none;
  }

  .tabs-container {
    display: flex;
    align-items: center;
    gap: 2px;
    overflow-x: auto;
    flex: 1;
    height: 100%;
  }

  .tab-item {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 10px;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--mute);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    border: 1px solid transparent;
    transition: all 0.12s ease;
    max-width: 220px;
  }

  .tab-item:hover {
    color: var(--ink-secondary);
    background: var(--canvas-hover);
  }

  .tab-active {
    background: var(--canvas-elevated);
    color: var(--ink);
    border-color: var(--hairline);
  }

  .tab-icon {
    display: flex;
    color: inherit;
  }

  .tab-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tab-mono-badge {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--mute);
    background: var(--canvas-subtle);
    border: 1px solid var(--hairline);
    padding: 1px 5px;
    border-radius: 3px;
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
    margin-left: 2px;
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
  }

  .add-btn:hover {
    color: var(--ink);
    background: var(--canvas-hover);
  }
</style>
