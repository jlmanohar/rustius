<script lang="ts">
  import { appState } from '../stores/appState.svelte';
  import type { TreeNode, Connection, Folder } from '../types';
  import {
    Folder as FolderIcon,
    FolderOpen,
    Terminal,
    ArrowLeftRight,
    Monitor,
    Plus,
    FolderPlus,
    Search,
    ChevronRight,
    ChevronDown,
    Trash2,
    Play,
    Server,
    Settings,
    Pencil,
    FolderDown,
    KeyRound
  } from '@lucide/svelte';

  let environments = $derived.by(() => {
    const set = new Set<string>();
    const scan = (nodes: TreeNode[]) => {
      for (const n of nodes) {
        if (n.node_type === 'connection' && n.environment) {
          set.add(n.environment);
        } else if (n.node_type === 'folder') {
          scan(n.children);
        }
      }
    };
    scan(appState.config.tree);
    return ['all', ...Array.from(set)];
  });

  function matchesFilter(node: TreeNode): boolean {
    const q = appState.searchQuery.toLowerCase().trim();
    const env = appState.selectedEnvironment;

    if (node.node_type === 'connection') {
      const matchesEnv = env === 'all' || node.environment === env;
      const matchesQuery =
        !q ||
        node.name.toLowerCase().includes(q) ||
        node.host.toLowerCase().includes(q) ||
        node.tags.some((t) => t.toLowerCase().includes(q));
      return matchesEnv && matchesQuery;
    } else {
      if (!q && env === 'all') return true;
      return node.children.some(matchesFilter);
    }
  }

  function handleConnect(conn: Connection) {
    const cType = conn.connection_type || conn.conn_type || 'ssh';
    appState.openTab({
      title: conn.name,
      type: cType,
      connectionId: conn.id,
      status: 'connecting',
      badge: `${cType.toUpperCase()} · ${conn.host}`,
    });
  }

  function toggleFolder(folder: Folder) {
    folder.expanded = !folder.expanded;
  }
</script>

<aside class="sidebar">
  <!-- Brand Header -->
  <div class="brand">
    <div class="brand-left">
      <!-- Rustius crab icon -->
      <img src="/rustius_icon.svg" alt="Rustius" class="brand-icon" width="18" height="18" />
      <span class="brand-title">RUSTIUS</span>
      <span class="brand-version">0.1.0</span>
    </div>
    <div class="brand-right">
      <button
        class="btn-icon"
        title="Create Folder"
        onclick={() => {
          const name = prompt('Folder name:');
          if (name && name.trim()) {
            appState.addFolder(name.trim());
          }
        }}
      >
        <FolderPlus size={14} />
      </button>
      <button
        class="btn-icon"
        title="Import Sessions (mRemoteNG / MobaXterm)"
        onclick={() => (appState.isImportModalOpen = true)}
      >
        <FolderDown size={14} />
      </button>
      <button
        class="btn-icon"
        title="Saved Credentials"
        onclick={() => (appState.isCredentialsModalOpen = true)}
      >
        <KeyRound size={14} />
      </button>
      <button
        class="btn-icon"
        title="Settings"
        onclick={() => (appState.isSettingsModalOpen = true)}
      >
        <Settings size={14} />
      </button>
    </div>
  </div>

  <!-- Primary Action -->
  <div class="action-row">
    <button
      class="btn btn-primary new-session-btn"
      onclick={() => {
        appState.editingConnection = null;
        appState.targetFolderId = null;
        appState.isConnectionModalOpen = true;
      }}
    >
      <Plus size={14} />
      <span>New Connection</span>
    </button>
  </div>

  <!-- Filter & Search -->
  <div class="filter-area">
    <div class="search-box">
      <Search size={13} class="search-icon" />
      <input
        type="text"
        placeholder="Search connections..."
        bind:value={appState.searchQuery}
        class="search-input"
      />
      {#if appState.searchQuery}
        <button class="clear-btn" onclick={() => (appState.searchQuery = '')}>×</button>
      {/if}
    </div>

    {#if environments.length > 1}
      <div class="env-nav">
        {#each environments as env}
          <button
            class="env-tab {appState.selectedEnvironment === env ? 'env-active' : ''}"
            onclick={() => (appState.selectedEnvironment = env)}
          >
            {env === 'all' ? 'All' : env}
          </button>
        {/each}
      </div>
    {/if}
  </div>

  <!-- Connection Tree List -->
  <div class="tree-list">
    {#if appState.config.tree.length === 0}
      <div class="empty-state">
        <Server size={24} class="empty-icon" />
        <p class="empty-text">No connections</p>
        <button
          class="btn btn-secondary btn-sm"
          onclick={() => (appState.isConnectionModalOpen = true)}
        >
          Add connection
        </button>
      </div>
    {:else}
      {#snippet renderNode(node: TreeNode, depth: number)}
        {#if matchesFilter(node)}
          {#if node.node_type === 'folder'}
            <div class="folder-block" style="padding-left: {depth * 12}px">
              <div
                class="folder-row"
                role="button"
                tabindex="0"
                onclick={() => toggleFolder(node)}
                onkeydown={(e) => e.key === 'Enter' && toggleFolder(node)}
              >
                <span class="chevron">
                  {#if node.expanded}
                    <ChevronDown size={13} />
                  {:else}
                    <ChevronRight size={13} />
                  {/if}
                </span>
                <span class="folder-icon">
                  {#if node.expanded}
                    <FolderOpen size={14} />
                  {:else}
                    <FolderIcon size={14} />
                  {/if}
                </span>
                <span class="folder-name">{node.name}</span>
                <span class="folder-badge">{node.children.length}</span>
                <button
                  class="row-action-btn"
                  title="Delete Folder"
                  onclick={(e) => {
                    e.stopPropagation();
                    if (confirm(`Delete folder "${node.name}"?`)) {
                      appState.removeNode(node.id);
                    }
                  }}
                >
                  <Trash2 size={12} />
                </button>
              </div>

              {#if node.expanded}
                <div class="folder-sub">
                  {#each node.children as child}
                    {@render renderNode(child, depth + 1)}
                  {/each}
                </div>
              {/if}
            </div>
          {:else}
            <div
              class="connection-row"
              style="padding-left: {depth * 12 + 10}px"
              role="button"
              tabindex="0"
              ondblclick={() => handleConnect(node)}
              onkeydown={(e) => e.key === 'Enter' && handleConnect(node)}
            >
              <div class="node-icon">
                {#if (node.conn_type || (node as any).connection_type) === 'ssh'}
                  <Terminal size={13} />
                {:else if (node.conn_type || (node as any).connection_type) === 'sftp'}
                  <ArrowLeftRight size={13} />
                {:else if (node.conn_type || (node as any).connection_type) === 'rdp'}
                  <Monitor size={13} />
                {/if}
              </div>

              <div class="node-details">
                <div class="node-name-row">
                  <span class="node-title">{node.name}</span>
                  {#if node.environment}
                    <span class="env-pill">{node.environment}</span>
                  {/if}
                </div>
                <div class="node-sub">{node.host}:{node.port}</div>
              </div>

              <div class="node-actions">
                <button
                  class="action-btn"
                  title="Connect"
                  onclick={(e) => {
                    e.stopPropagation();
                    handleConnect(node);
                  }}
                >
                  <Play size={11} />
                </button>
                <button
                  class="action-btn"
                  title="Edit"
                  onclick={(e) => {
                    e.stopPropagation();
                    appState.editingConnection = node;
                    appState.isConnectionModalOpen = true;
                  }}
                >
                  <Pencil size={11} />
                </button>
                <button
                  class="action-btn"
                  title="Delete"
                  onclick={(e) => {
                    e.stopPropagation();
                    if (confirm(`Delete "${node.name}"?`)) {
                      appState.removeNode(node.id);
                    }
                  }}
                >
                  <Trash2 size={11} />
                </button>
              </div>
            </div>
          {/if}
        {/if}
      {/snippet}

      {#each appState.config.tree as rootNode}
        {@render renderNode(rootNode, 0)}
      {/each}
    {/if}
  </div>

  <!-- Sidebar Footer: Local Terminal Launcher -->
  <div class="sidebar-footer">
    <button
      class="local-shell-btn"
      onclick={() => {
        appState.openTab({
          title: 'Local Shell',
          type: 'local_terminal',
          status: 'connected',
          badge: 'POWERSHELL',
        });
      }}
    >
      <Terminal size={13} />
      <span>New Terminal</span>
      <kbd class="shortcut-kbd">Ctrl+`</kbd>
    </button>
  </div>
</aside>

<style>
  .sidebar {
    width: 270px;
    height: 100%;
    background: var(--canvas);
    border-right: 1px solid var(--hairline);
    display: flex;
    flex-direction: column;
    user-select: none;
    flex-shrink: 0;
  }

  .brand {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 48px;
    padding: 0 14px;
    border-bottom: 1px solid var(--hairline);
  }

  .brand-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .brand-icon {
    width: 18px;
    height: 18px;
    object-fit: contain;
  }

  .brand-title {
    font-size: 13px;
    font-weight: 600;
    letter-spacing: -0.4px;
    color: var(--ink);
  }

  .brand-version {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--mute);
  }

  .brand-right {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .action-row {
    padding: 10px 14px 4px 14px;
  }

  .new-session-btn {
    width: 100%;
    height: 30px;
    font-size: 12px;
  }

  .filter-area {
    padding: 8px 14px 10px 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    border-bottom: 1px solid var(--hairline-soft);
  }

  .search-box {
    position: relative;
    display: flex;
    align-items: center;
  }

  .search-box :global(.search-icon) {
    position: absolute;
    left: 8px;
    color: var(--mute);
    pointer-events: none;
  }

  .search-input {
    width: 100%;
    height: 28px;
    padding: 0 24px 0 26px;
    background: var(--canvas-subtle);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-sm);
    color: var(--ink);
    font-size: 12px;
    font-family: var(--font-sans);
    outline: none;
    transition: border-color 0.12s ease;
  }

  .search-input:focus {
    border-color: var(--hairline-focus);
    background: var(--canvas-elevated);
  }

  .clear-btn {
    position: absolute;
    right: 6px;
    background: transparent;
    border: none;
    color: var(--mute);
    font-size: 13px;
    cursor: pointer;
  }

  .env-nav {
    display: flex;
    gap: 4px;
    overflow-x: auto;
  }

  .env-tab {
    padding: 2px 8px;
    font-size: 11px;
    font-weight: 500;
    border-radius: var(--radius-sm);
    background: transparent;
    border: 1px solid transparent;
    color: var(--mute);
    cursor: pointer;
    white-space: nowrap;
    transition: all 0.12s ease;
  }

  .env-tab:hover {
    color: var(--ink-secondary);
  }

  .env-active {
    background: var(--canvas-hover);
    color: var(--ink);
    border-color: var(--hairline);
  }

  .tree-list {
    flex: 1;
    overflow-y: auto;
    padding: 6px 8px;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 160px;
    gap: 8px;
    color: var(--mute);
  }

  .empty-text {
    font-size: 12px;
    margin: 0;
  }

  .folder-row {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 6px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    color: var(--body);
    transition: background 0.12s ease, color 0.12s ease;
  }

  .folder-row:hover {
    background: var(--canvas-hover);
    color: var(--ink);
  }

  .chevron {
    color: var(--mute);
    display: flex;
  }

  .folder-icon {
    color: var(--mute);
    display: flex;
  }

  .folder-name {
    font-size: 12px;
    font-weight: 500;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .folder-badge {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--mute);
  }

  .row-action-btn {
    opacity: 0;
    background: transparent;
    border: none;
    color: var(--mute);
    cursor: pointer;
    padding: 2px;
    border-radius: var(--radius-sm);
  }

  .folder-row:hover .row-action-btn {
    opacity: 1;
  }

  .row-action-btn:hover {
    color: var(--accent-red);
  }

  .connection-row {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    padding: 0 8px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background 0.12s ease;
    margin-bottom: 1px;
  }

  .connection-row:hover {
    background: var(--canvas-hover);
  }

  .node-icon {
    color: var(--mute);
    display: flex;
    flex-shrink: 0;
  }

  .connection-row:hover .node-icon {
    color: var(--ink);
  }

  .node-details {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
  }

  .node-name-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .node-title {
    font-size: 12px;
    font-weight: 500;
    color: var(--ink-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .env-pill {
    font-family: var(--font-mono);
    font-size: 9px;
    padding: 0 4px;
    border-radius: 2px;
    background: var(--canvas-elevated);
    border: 1px solid var(--hairline);
    color: var(--mute);
  }

  .node-sub {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--mute);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .node-actions {
    display: flex;
    align-items: center;
    gap: 2px;
    opacity: 0;
    transition: opacity 0.12s ease;
  }

  .connection-row:hover .node-actions {
    opacity: 1;
  }

  .action-btn {
    width: 22px;
    height: 22px;
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

  .action-btn:hover {
    background: var(--hairline);
    color: var(--ink);
  }

  .sidebar-footer {
    padding: 10px 14px;
    border-top: 1px solid var(--hairline);
  }

  .local-shell-btn {
    width: 100%;
    height: 30px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px;
    font-size: 12px;
    font-family: var(--font-sans);
    background: var(--canvas-subtle);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-sm);
    color: var(--body);
    cursor: pointer;
    transition: all 0.12s ease;
  }

  .local-shell-btn:hover {
    background: var(--canvas-hover);
    color: var(--ink);
    border-color: var(--hairline-hover);
  }

  .shortcut-kbd {
    margin-left: auto;
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--mute);
    background: var(--canvas-elevated);
    border: 1px solid var(--hairline);
    padding: 1px 4px;
    border-radius: 3px;
  }
</style>
