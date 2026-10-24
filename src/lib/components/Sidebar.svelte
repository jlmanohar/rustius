<script lang="ts">
  import { appState } from '../stores/appState.svelte';
  import type { TreeNode, Connection, Folder, CredentialOverride } from '../types';
  import ConnectWithCredentialsModal from './ConnectWithCredentialsModal.svelte';
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
    KeyRound,
    FileText,
    Lock,
    LockOpen
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
        (node.description && node.description.toLowerCase().includes(q)) ||
        (node.notes && node.notes.toLowerCase().includes(q)) ||
        node.tags.some((t) => t.toLowerCase().includes(q));
      return matchesEnv && matchesQuery;
    } else {
      if (!q && env === 'all') return true;
      return node.children.some(matchesFilter);
    }
  }

  function handleConnect(conn: Connection) {
    const cType = conn.connection_type || conn.conn_type || 'ssh';
    const title = (conn.name && conn.name.trim()) ? conn.name.trim() : conn.host;
    appState.openTab({
      title,
      type: cType,
      connectionId: conn.id,
      status: 'connecting',
      badge: conn.host,
    });
  }

  function handleConnectWithOverride(conn: Connection, override: CredentialOverride) {
    const cType = conn.connection_type || conn.conn_type || 'ssh';
    const title = (conn.name && conn.name.trim()) ? conn.name.trim() : conn.host;
    appState.openTab({
      title,
      type: cType,
      connectionId: conn.id,
      status: 'connecting',
      badge: override.username ? `${override.username}@${conn.host}` : conn.host,
      credentialsOverride: override,
    });
    credentialsModalConnection = null;
  }

  // Right-click connection context menu and temporary credentials modal state
  let contextMenu = $state<{
    x: number;
    y: number;
    connection: Connection;
  } | null>(null);

  let credentialsModalConnection = $state<Connection | null>(null);

  function handleConnectionContextMenu(e: MouseEvent, conn: Connection) {
    e.preventDefault();
    e.stopPropagation();
    const x = Math.min(e.clientX, window.innerWidth - 240);
    const y = Math.min(e.clientY, window.innerHeight - 200);
    contextMenu = { x, y, connection: conn };
  }

  function closeContextMenu() {
    contextMenu = null;
  }

  function toggleFolder(folder: Folder) {
    folder.expanded = !folder.expanded;
  }

  // Drag and drop state
  let draggedNodeId = $state<string | null>(null);
  let draggedNodeType = $state<'connection' | 'folder' | null>(null);
  let draggedParentId = $state<string | null>(null);
  let dropTargetId = $state<string | null>(null);
  let dropPosition = $state<'before' | 'after' | 'inside' | null>(null);
  let isOverRootDropZone = $state<boolean>(false);

  function isDescendantFolder(folderId: string, checkTargetId: string): boolean {
    if (folderId === checkTargetId) return true;
    const hasNode = (nodes: TreeNode[], id: string): boolean => {
      for (const n of nodes) {
        if (n.id === id) return true;
        if (n.node_type === 'folder' && hasNode(n.children, id)) return true;
      }
      return false;
    };
    const search = (nodes: TreeNode[]): boolean => {
      for (const n of nodes) {
        if (n.node_type === 'folder') {
          if (n.id === folderId) {
            return hasNode(n.children, checkTargetId);
          }
          if (search(n.children)) return true;
        }
      }
      return false;
    };
    return search(appState.config.tree);
  }

  function findFolderById(nodes: TreeNode[], folderId: string): Folder | null {
    for (const node of nodes) {
      if (node.node_type === 'folder') {
        if (node.id === folderId) return node;
        const sub = findFolderById(node.children, folderId);
        if (sub) return sub;
      }
    }
    return null;
  }

  function handleDragStart(e: DragEvent, node: TreeNode, parentFolderId: string | null) {
    if (appState.searchQuery.trim() !== '') {
      e.preventDefault();
      return;
    }
    draggedNodeId = node.id;
    draggedNodeType = node.node_type;
    draggedParentId = parentFolderId;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', node.id);
    }
  }

  function handleDragEnd() {
    draggedNodeId = null;
    draggedNodeType = null;
    draggedParentId = null;
    dropTargetId = null;
    dropPosition = null;
    isOverRootDropZone = false;
  }

  function handleDragOver(e: DragEvent, targetNode: TreeNode, parentFolderId: string | null) {
    if (!draggedNodeId) return;
    if (draggedNodeId === targetNode.id) return;

    if (draggedNodeType === 'folder' && isDescendantFolder(draggedNodeId, targetNode.id)) {
      return;
    }

    e.preventDefault();
    if (e.dataTransfer) {
      e.dataTransfer.dropEffect = 'move';
    }

    const currentTarget = e.currentTarget as HTMLElement;
    const rect = currentTarget.getBoundingClientRect();
    const relativeY = e.clientY - rect.top;
    const height = rect.height;

    dropTargetId = targetNode.id;

    if (targetNode.node_type === 'folder') {
      if (relativeY < height * 0.25) {
        dropPosition = 'before';
      } else if (relativeY > height * 0.75 && !targetNode.expanded) {
        dropPosition = 'after';
      } else {
        dropPosition = 'inside';
      }
    } else {
      if (relativeY < height * 0.5) {
        dropPosition = 'before';
      } else {
        dropPosition = 'after';
      }
    }
  }

  async function handleDrop(e: DragEvent, targetNode: TreeNode, parentFolderId: string | null) {
    e.preventDefault();
    e.stopPropagation();

    const movingId = draggedNodeId;
    const position = dropPosition;
    handleDragEnd();

    if (!movingId || !position || movingId === targetNode.id) return;

    if (position === 'inside' && targetNode.node_type === 'folder') {
      targetNode.expanded = true;
      const targetList = targetNode.children.filter((n) => n.id !== movingId);
      await appState.moveNode(movingId, targetNode.id, targetList.length);
      return;
    }

    let targetList: TreeNode[] = [];
    if (parentFolderId) {
      const parentFolder = findFolderById(appState.config.tree, parentFolderId);
      if (parentFolder) {
        targetList = parentFolder.children;
      }
    } else {
      targetList = appState.config.tree;
    }

    const filtered = targetList.filter((n) => n.id !== movingId);
    const targetIdx = filtered.findIndex((n) => n.id === targetNode.id);
    if (targetIdx === -1) return;

    const newIndex = position === 'before' ? targetIdx : targetIdx + 1;
    await appState.moveNode(movingId, parentFolderId, newIndex);
  }

  async function handleDropOnRoot(e: DragEvent) {
    e.preventDefault();
    e.stopPropagation();

    const movingId = draggedNodeId;
    handleDragEnd();

    if (!movingId) return;

    const rootList = appState.config.tree.filter((n) => n.id !== movingId);
    await appState.moveNode(movingId, null, rootList.length);
  }
</script>

<svelte:window onclick={closeContextMenu} />

<aside class="sidebar">
  <!-- Brand Header -->
  <div class="brand">
    <div class="brand-left">
      <!-- Rustius crab icon -->
      <img src="/rustius_icon.svg" alt="Rustius" class="brand-icon" width="18" height="18" />
      <span class="brand-title">RUSTIUS</span>
      <span class="brand-version">0.1.3</span>
    </div>
    <div class="brand-right">
      <button
        class="btn-icon"
        title="Create Folder"
        onclick={async () => {
          const name = await appState.prompt({
            title: 'New Folder',
            placeholder: 'e.g. Production Servers, AWS Clusters',
            confirmText: 'Create Folder',
          });
          if (name && name.trim()) {
            await appState.addFolder(name.trim());
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
      {#if appState.vaultStatus?.has_vault_file}
        <button
          class="btn-icon {appState.vaultStatus.is_unlocked ? 'vault-unlocked-btn' : 'vault-locked-btn'}"
          title={appState.vaultStatus.is_unlocked ? 'Vault Unlocked (Click to lock credentials)' : 'Vault Locked (Click to unlock)'}
          onclick={() => {
            if (appState.vaultStatus?.is_unlocked) {
              void appState.lockVault();
            } else {
              appState.isVaultUnlockModalOpen = true;
            }
          }}
        >
          {#if appState.vaultStatus.is_unlocked}
            <LockOpen size={14} />
          {:else}
            <Lock size={14} />
          {/if}
        </button>
      {/if}
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
  <div
    class="tree-list"
    role="region"
    aria-label="Connection Tree"
    ondragover={(e) => {
      if (!draggedNodeId) return;
      e.preventDefault();
      if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
    }}
    ondrop={(e) => {
      if (e.target === e.currentTarget) {
        handleDropOnRoot(e);
      }
    }}
  >
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
      {#snippet renderNode(node: TreeNode, depth: number, parentFolderId: string | null)}
        {#if matchesFilter(node)}
          {#if node.node_type === 'folder'}
            <div class="folder-block" style="padding-left: {depth * 12}px">
              <div class="folder-row-wrapper relative">
                {#if dropTargetId === node.id && dropPosition === 'before'}
                  <div class="drop-line drop-line-top"></div>
                {/if}

                <div
                  class="folder-row {draggedNodeId === node.id ? 'is-dragging' : ''} {dropTargetId === node.id && dropPosition === 'inside' ? 'folder-drop-inside' : ''}"
                  role="button"
                  tabindex="0"
                  draggable={!appState.searchQuery}
                  ondragstart={(e) => handleDragStart(e, node, parentFolderId)}
                  ondragend={handleDragEnd}
                  ondragover={(e) => handleDragOver(e, node, parentFolderId)}
                  ondragleave={() => {
                    if (dropTargetId === node.id) {
                      dropTargetId = null;
                      dropPosition = null;
                    }
                  }}
                  ondrop={(e) => handleDrop(e, node, parentFolderId)}
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
                    onclick={async (e) => {
                      e.stopPropagation();
                      const ok = await appState.confirm({
                        title: 'Delete Folder',
                        message: `Are you sure you want to delete folder "${node.name}" and all of its contents?`,
                        confirmText: 'Delete Folder',
                        isDanger: true,
                      });
                      if (ok) {
                        await appState.removeNode(node.id);
                      }
                    }}
                  >
                    <Trash2 size={12} />
                  </button>
                </div>

                {#if dropTargetId === node.id && dropPosition === 'after'}
                  <div class="drop-line drop-line-bottom"></div>
                {/if}
              </div>

              {#if node.expanded}
                <div class="folder-sub">
                  {#each node.children as child}
                    {@render renderNode(child, depth + 1, node.id)}
                  {/each}
                </div>
              {/if}
            </div>
          {:else}
            <div class="connection-row-wrapper relative">
              {#if dropTargetId === node.id && dropPosition === 'before'}
                <div class="drop-line drop-line-top"></div>
              {/if}

              <div
                class="connection-row {draggedNodeId === node.id ? 'is-dragging' : ''}"
                style="padding-left: {depth * 12 + 10}px"
                role="button"
                tabindex="0"
                draggable={!appState.searchQuery}
                ondragstart={(e) => handleDragStart(e, node, parentFolderId)}
                ondragend={handleDragEnd}
                ondragover={(e) => handleDragOver(e, node, parentFolderId)}
                ondragleave={() => {
                  if (dropTargetId === node.id) {
                    dropTargetId = null;
                    dropPosition = null;
                  }
                }}
                ondrop={(e) => handleDrop(e, node, parentFolderId)}
                ondblclick={() => handleConnect(node)}
                onkeydown={(e) => e.key === 'Enter' && handleConnect(node)}
                oncontextmenu={(e) => handleConnectionContextMenu(e, node)}
              >
                <div class="node-icon">
                  {#if (node.connection_type || node.conn_type) === 'ssh'}
                    <Terminal size={13} />
                  {:else if (node.connection_type || node.conn_type) === 'sftp'}
                    <ArrowLeftRight size={13} />
                  {:else if (node.connection_type || node.conn_type) === 'rdp'}
                    <Monitor size={13} />
                  {/if}
                </div>

                <div class="node-details">
                  <div class="node-name-row">
                    <span class="node-title" title={node.name}>{node.name}</span>
                    {#if node.notes}
                      <span class="notes-indicator" title="Has documentation/notes">
                        <FileText size={10} />
                      </span>
                    {/if}
                  </div>
                  <div class="node-sub">
                    {#if node.environment}
                      <span class="env-pill">{node.environment}</span>
                    {/if}
                    <span class="node-endpoint">{node.host}:{node.port}</span>
                    {#if node.description}
                      <span class="node-desc" title={node.description}>· {node.description}</span>
                    {/if}
                  </div>
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
                    onclick={async (e) => {
                      e.stopPropagation();
                      const ok = await appState.confirm({
                        title: 'Delete Connection',
                        message: `Are you sure you want to delete connection "${node.name}"?`,
                        confirmText: 'Delete Connection',
                        isDanger: true,
                      });
                      if (ok) {
                        await appState.removeNode(node.id);
                      }
                    }}
                  >
                    <Trash2 size={11} />
                  </button>
                </div>
              </div>

              {#if dropTargetId === node.id && dropPosition === 'after'}
                <div class="drop-line drop-line-bottom"></div>
              {/if}
            </div>
          {/if}
        {/if}
      {/snippet}

      {#each appState.config.tree as rootNode}
        {@render renderNode(rootNode, 0, null)}
      {/each}

      <!-- Bottom Root Drop Target -->
      <div
        class="root-drop-zone {isOverRootDropZone ? 'active' : ''} {draggedNodeId ? 'visible' : ''}"
        role="region"
        aria-label="Move to root drop zone"
        ondragenter={(e) => {
          if (!draggedNodeId) return;
          e.preventDefault();
          isOverRootDropZone = true;
          dropTargetId = null;
          dropPosition = null;
        }}
        ondragover={(e) => {
          if (!draggedNodeId) return;
          e.preventDefault();
          if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
          isOverRootDropZone = true;
          dropTargetId = null;
          dropPosition = null;
        }}
        ondragleave={(e) => {
          if (e.currentTarget.contains(e.relatedTarget as Node)) return;
          isOverRootDropZone = false;
        }}
        ondrop={handleDropOnRoot}
      >
        {#if draggedNodeId}
          <div class="root-drop-hint {isOverRootDropZone ? 'active' : ''}">
            {isOverRootDropZone ? 'Release to move to root' : 'Drop to move to root'}
          </div>
        {/if}
      </div>
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

{#if contextMenu}
  <div
    class="connection-context-menu"
    role="menu"
    tabindex="-1"
    style="left: {contextMenu.x}px; top: {contextMenu.y}px;"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.key === 'Escape' && closeContextMenu()}
  >
    <button
      class="context-menu-item"
      onclick={() => {
        if (contextMenu) handleConnect(contextMenu.connection);
        closeContextMenu();
      }}
    >
      <Play size={12} class="menu-item-icon" />
      <span>Connect</span>
    </button>
    <button
      class="context-menu-item"
      onclick={() => {
        if (contextMenu) {
          credentialsModalConnection = contextMenu.connection;
        }
        closeContextMenu();
      }}
    >
      <KeyRound size={12} class="menu-item-icon text-amber" />
      <span>Connect with different credentials...</span>
    </button>
    <div class="menu-divider"></div>
    <button
      class="context-menu-item"
      onclick={() => {
        if (contextMenu) {
          appState.editingConnection = contextMenu.connection;
          appState.isConnectionModalOpen = true;
        }
        closeContextMenu();
      }}
    >
      <Pencil size={12} class="menu-item-icon" />
      <span>Edit</span>
    </button>
    <button
      class="context-menu-item text-danger"
      onclick={async () => {
        const conn = contextMenu?.connection;
        closeContextMenu();
        if (conn) {
          const ok = await appState.confirm({
            title: 'Delete Connection',
            message: `Are you sure you want to delete connection "${conn.name}"?`,
            confirmText: 'Delete Connection',
            isDanger: true,
          });
          if (ok) {
            await appState.removeNode(conn.id);
          }
        }
      }}
    >
      <Trash2 size={12} class="menu-item-icon" />
      <span>Delete</span>
    </button>
  </div>
{/if}

{#if credentialsModalConnection}
  <ConnectWithCredentialsModal
    connection={credentialsModalConnection}
    onconnect={(override) => handleConnectWithOverride(credentialsModalConnection!, override)}
    onclose={() => (credentialsModalConnection = null)}
  />
{/if}

<style>
  .sidebar {
    width: clamp(250px, 19vw, 300px);
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
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 38px;
    padding: 0 8px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background 0.12s ease;
    margin-bottom: 1px;
    overflow: hidden;
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
    gap: 1px;
  }

  .node-name-row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .node-title {
    font-size: 12px;
    font-weight: 500;
    color: var(--ink-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    flex: 1;
  }

  .env-pill {
    font-family: var(--font-mono);
    font-size: 9px;
    padding: 0 4px;
    border-radius: 2px;
    background: var(--canvas-elevated);
    border: 1px solid var(--hairline);
    color: var(--mute);
    flex-shrink: 0;
    line-height: 13px;
  }

  .notes-indicator {
    display: inline-flex;
    align-items: center;
    color: var(--mute);
    margin-left: 2px;
    flex-shrink: 0;
  }

  .node-sub {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--mute);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
  }

  .node-endpoint {
    flex-shrink: 0;
  }

  .node-desc {
    color: var(--body);
    font-family: var(--font-sans);
    font-size: 10px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 130px;
  }

  .node-actions {
    position: absolute;
    right: 4px;
    top: 50%;
    transform: translateY(-50%);
    display: flex;
    align-items: center;
    gap: 2px;
    opacity: 0;
    pointer-events: none;
    background: var(--canvas-hover);
    padding: 2px 4px 2px 8px;
    border-radius: var(--radius-sm);
    box-shadow: -10px 0 12px var(--canvas-hover);
    transition: opacity 0.12s ease;
  }

  .connection-row:hover .node-actions {
    opacity: 1;
    pointer-events: auto;
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

  /* Drag and drop styling */
  .relative {
    position: relative;
  }

  .connection-row-wrapper,
  .folder-row-wrapper {
    position: relative;
  }

  .is-dragging {
    opacity: 0.35 !important;
    background: var(--canvas-hover) !important;
    cursor: grabbing !important;
  }

  .drop-line {
    position: absolute;
    left: 6px;
    right: 6px;
    height: 2px;
    background: #ffffff;
    border-radius: 2px;
    z-index: 50;
    pointer-events: none;
    box-shadow: 0 0 8px rgba(255, 255, 255, 0.5), 0 0 2px rgba(255, 255, 255, 0.9);
  }

  .drop-line::before {
    content: '';
    position: absolute;
    left: -4px;
    top: -3px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #ffffff;
    box-shadow: 0 0 6px rgba(255, 255, 255, 0.7);
  }

  .drop-line-top {
    top: -1px;
  }

  .drop-line-bottom {
    bottom: -1px;
  }

  .folder-drop-inside {
    background: rgba(255, 255, 255, 0.08) !important;
    outline: 1px solid rgba(255, 255, 255, 0.6);
    outline-offset: -1px;
    border-radius: var(--radius-sm);
  }

  .root-drop-zone {
    height: 38px;
    margin-top: 8px;
    border: 1px dashed transparent;
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background 0.12s ease, border-color 0.12s ease;
    user-select: none;
  }

  .root-drop-zone.visible {
    border-color: var(--hairline);
  }

  .root-drop-zone.active {
    background: rgba(255, 255, 255, 0.08);
    border-color: rgba(255, 255, 255, 0.6);
    border-style: solid;
  }

  .root-drop-hint {
    font-size: 11px;
    color: var(--mute);
    font-weight: 500;
    pointer-events: none;
    transition: color 0.12s ease;
  }

  .root-drop-hint.active {
    color: var(--ink);
  }

  /* Right-click Connection Context Menu */
  .connection-context-menu {
    position: fixed;
    z-index: 10000;
    min-width: 235px;
    background: var(--canvas-elevated);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-sm);
    padding: 4px;
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.7);
    display: flex;
    flex-direction: column;
    animation: menuFadeIn 0.1s ease-out;
    user-select: none;
  }

  .context-menu-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 10px;
    font-size: 11.5px;
    font-weight: 500;
    color: var(--ink-secondary);
    background: transparent;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    text-align: left;
    transition: all 0.1s ease;
    font-family: inherit;
  }

  .context-menu-item:hover {
    background: var(--canvas-hover);
    color: var(--ink);
  }

  :global(.menu-item-icon) {
    flex-shrink: 0;
    color: var(--mute);
  }

  .context-menu-item:hover :global(.menu-item-icon) {
    color: var(--ink);
  }

  :global(.menu-item-icon.text-amber) {
    color: var(--accent-amber) !important;
  }

  .context-menu-item.text-danger {
    color: #ff4d4d;
  }

  .context-menu-item.text-danger:hover {
    background: rgba(255, 77, 77, 0.12);
    color: #ff4d4d;
  }

  .context-menu-item.text-danger :global(.menu-item-icon) {
    color: #ff4d4d;
  }

  .menu-divider {
    height: 1px;
    background: var(--hairline);
    margin: 4px 0;
  }

  @keyframes menuFadeIn {
    from {
      opacity: 0;
      transform: scale(0.98);
    }
    to {
      opacity: 1;
      transform: scale(1);
    }
  }
  .vault-unlocked-btn {
    color: #10b981 !important;
  }

  .vault-unlocked-btn:hover {
    color: #34d399 !important;
  }

  .vault-locked-btn {
    color: #f59e0b !important;
  }

  .vault-locked-btn:hover {
    color: #fbbf24 !important;
  }
</style>
