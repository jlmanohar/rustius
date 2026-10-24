<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import type { TabSession, FileEntry, TransferItem } from '../types';
  import {
    Folder,
    FolderPlus,
    FileText,
    ArrowUp,
    ArrowDown,
    RefreshCw,
    Trash2,
    HardDrive,
    Server,
    Search,
    X,
    CheckCircle2,
    AlertCircle,
    Loader2,
    ChevronRight,
    CornerLeftUp,
    Download,
    Upload
  } from '@lucide/svelte';

  let { session }: { session: TabSession } = $props();

  // Local state
  let localPath = $state<string>('');
  let localFiles = $state<FileEntry[]>([]);
  let localSearch = $state<string>('');
  let selectedLocal = $state<FileEntry | null>(null);
  let isLoadingLocal = $state<boolean>(false);

  // Remote state
  let remotePath = $state<string>('');
  let remoteFiles = $state<FileEntry[]>([]);
  let remoteSearch = $state<string>('');
  let selectedRemote = $state<FileEntry | null>(null);
  let isLoadingRemote = $state<boolean>(false);
  let isConnected = $state<boolean>(false);

  // Status & Transfers
  let errorMessage = $state<string | null>(null);
  let transfers = $state<TransferItem[]>([]);
  let isTransfersExpanded = $state<boolean>(false);

  // Modal State
  type ModalAction = 'new_folder' | 'rename' | 'delete';
  let modalTarget = $state<'local' | 'remote'>('local');
  let modalAction = $state<ModalAction | null>(null);
  let modalInput = $state<string>('');
  let modalTargetFile = $state<FileEntry | null>(null);

  // Drag and Drop
  let draggedSource = $state<{ side: 'local' | 'remote'; file: FileEntry } | null>(null);

  function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
  }

  // Filtered files
  let filteredLocalFiles = $derived.by(() => {
    if (!localSearch.trim()) return localFiles;
    const q = localSearch.toLowerCase();
    return localFiles.filter((f) => f.name.toLowerCase().includes(q));
  });

  let filteredRemoteFiles = $derived.by(() => {
    if (!remoteSearch.trim()) return remoteFiles;
    const q = remoteSearch.toLowerCase();
    return remoteFiles.filter((f) => f.name.toLowerCase().includes(q));
  });

  // Local navigation
  async function loadLocalDir(path: string) {
    isLoadingLocal = true;
    errorMessage = null;
    try {
      const files = await invoke<FileEntry[]>('local_list_dir', { path });
      localFiles = files;
      localPath = path;
      selectedLocal = null;
    } catch (err: any) {
      errorMessage = `Local error: ${err?.message || err}`;
    } finally {
      isLoadingLocal = false;
    }
  }

  function getLocalParent(p: string): string {
    const isWindows = p.includes('\\') || /^[A-Za-z]:/.test(p);
    if (isWindows) {
      const parts = p.replace(/\/+$/, '').replace(/\\+$/, '').split(/[/\\]/);
      if (parts.length <= 1) return p;
      parts.pop();
      const res = parts.join('\\');
      return res.endsWith(':') ? res + '\\' : (res || 'C:\\');
    } else {
      const parts = p.replace(/\/+$/, '').split('/');
      parts.pop();
      return parts.join('/') || '/';
    }
  }

  function goUpLocal() {
    const parent = getLocalParent(localPath);
    if (parent && parent !== localPath) {
      loadLocalDir(parent);
    }
  }

  // Remote navigation
  async function loadRemoteDir(path: string) {
    if (!session.connectionId) return;
    isLoadingRemote = true;
    errorMessage = null;
    try {
      const files = await invoke<FileEntry[]>('sftp_list_dir', {
        tabId: session.id,
        path,
      });
      remoteFiles = files;
      remotePath = path;
      selectedRemote = null;
    } catch (err: any) {
      errorMessage = `SFTP error: ${err?.message || err}`;
    } finally {
      isLoadingRemote = false;
    }
  }

  function getRemoteParent(p: string): string {
    const parts = p.replace(/\/+$/, '').split('/');
    parts.pop();
    return parts.join('/') || '/';
  }

  function goUpRemote() {
    const parent = getRemoteParent(remotePath);
    if (parent && parent !== remotePath) {
      loadRemoteDir(parent);
    }
  }

  // Connect SFTP session
  async function initSftp() {
    if (!session.connectionId) return;
    isLoadingRemote = true;
    errorMessage = null;
    try {
      const initialPath = await invoke<string>('sftp_connect', {
        tabId: session.id,
        connectionId: session.connectionId,
        credentialsOverride: session.credentialsOverride,
      });
      isConnected = true;
      await loadRemoteDir(initialPath);
    } catch (err: any) {
      isConnected = false;
      errorMessage = `Failed to connect SFTP: ${err?.message || err}`;
    } finally {
      isLoadingRemote = false;
    }
  }

  // File Transfer: Upload
  async function uploadFile(file: FileEntry) {
    if (!isConnected) return;
    const destRemote = remotePath.endsWith('/')
      ? `${remotePath}${file.name}`
      : `${remotePath}/${file.name}`;

    const transferId = Math.random().toString(36).substring(2, 9);
    const item: TransferItem = {
      id: transferId,
      source: file.path,
      dest: destRemote,
      direction: 'upload',
      status: 'transferring',
    };
    transfers = [item, ...transfers];

    try {
      await invoke('sftp_upload', {
        tabId: session.id,
        localPath: file.path,
        remotePath: destRemote,
      });
      item.status = 'completed';
      transfers = [...transfers];
      await loadRemoteDir(remotePath);
    } catch (err: any) {
      item.status = 'failed';
      item.error = err?.message || err;
      transfers = [...transfers];
      errorMessage = `Upload failed: ${err?.message || err}`;
    }
  }

  // File Transfer: Download
  async function downloadFile(file: FileEntry) {
    if (!isConnected) return;
    const isWindows = localPath.includes('\\') || /^[A-Za-z]:/.test(localPath);
    const sep = isWindows ? '\\' : '/';
    const destLocal = localPath.endsWith(sep)
      ? `${localPath}${file.name}`
      : `${localPath}${sep}${file.name}`;

    const transferId = Math.random().toString(36).substring(2, 9);
    const item: TransferItem = {
      id: transferId,
      source: file.path,
      dest: destLocal,
      direction: 'download',
      status: 'transferring',
    };
    transfers = [item, ...transfers];

    try {
      await invoke('sftp_download', {
        tabId: session.id,
        remotePath: file.path,
        localPath: destLocal,
      });
      item.status = 'completed';
      transfers = [...transfers];
      await loadLocalDir(localPath);
    } catch (err: any) {
      item.status = 'failed';
      item.error = err?.message || err;
      transfers = [...transfers];
      errorMessage = `Download failed: ${err?.message || err}`;
    }
  }

  // Open modals
  function openNewFolderModal(target: 'local' | 'remote') {
    modalTarget = target;
    modalAction = 'new_folder';
    modalInput = '';
    modalTargetFile = null;
  }

  function openRenameModal(target: 'local' | 'remote', file: FileEntry) {
    modalTarget = target;
    modalAction = 'rename';
    modalInput = file.name;
    modalTargetFile = file;
  }

  function openDeleteModal(target: 'local' | 'remote', file: FileEntry) {
    modalTarget = target;
    modalAction = 'delete';
    modalInput = '';
    modalTargetFile = file;
  }

  function closeModal() {
    modalAction = null;
    modalTargetFile = null;
    modalInput = '';
  }

  // Execute modal actions
  async function handleModalSubmit() {
    if (!modalAction) return;

    try {
      if (modalAction === 'new_folder') {
        const name = modalInput.trim();
        if (!name) return;

        if (modalTarget === 'local') {
          const sep = localPath.includes('\\') ? '\\' : '/';
          const p = localPath.endsWith(sep) ? `${localPath}${name}` : `${localPath}${sep}${name}`;
          await invoke('local_mkdir', { path: p });
          await loadLocalDir(localPath);
        } else {
          const p = remotePath.endsWith('/') ? `${remotePath}${name}` : `${remotePath}/${name}`;
          await invoke('sftp_mkdir', { tabId: session.id, path: p });
          await loadRemoteDir(remotePath);
        }
      } else if (modalAction === 'rename' && modalTargetFile) {
        const newName = modalInput.trim();
        if (!newName || newName === modalTargetFile.name) return;

        if (modalTarget === 'local') {
          const parent = getLocalParent(modalTargetFile.path);
          const sep = parent.includes('\\') ? '\\' : '/';
          const newPath = parent.endsWith(sep) ? `${parent}${newName}` : `${parent}${sep}${newName}`;
          await invoke('local_rename', { from: modalTargetFile.path, to: newPath });
          await loadLocalDir(localPath);
        } else {
          const parent = getRemoteParent(modalTargetFile.path);
          const newPath = parent.endsWith('/') ? `${parent}${newName}` : `${parent}/${newName}`;
          await invoke('sftp_rename', { tabId: session.id, from: modalTargetFile.path, to: newPath });
          await loadRemoteDir(remotePath);
        }
      } else if (modalAction === 'delete' && modalTargetFile) {
        if (modalTarget === 'local') {
          await invoke('local_remove', {
            path: modalTargetFile.path,
            isDir: modalTargetFile.is_dir,
          });
          await loadLocalDir(localPath);
        } else {
          await invoke('sftp_remove', {
            tabId: session.id,
            path: modalTargetFile.path,
            isDir: modalTargetFile.is_dir,
          });
          await loadRemoteDir(remotePath);
        }
      }
      closeModal();
    } catch (err: any) {
      errorMessage = `Operation failed: ${err?.message || err}`;
    }
  }

  // Drag and Drop handlers
  function handleDragStart(side: 'local' | 'remote', file: FileEntry) {
    draggedSource = { side, file };
  }

  function handleDrop(targetSide: 'local' | 'remote') {
    if (!draggedSource) return;
    if (draggedSource.side === 'local' && targetSide === 'remote') {
      uploadFile(draggedSource.file);
    } else if (draggedSource.side === 'remote' && targetSide === 'local') {
      downloadFile(draggedSource.file);
    }
    draggedSource = null;
  }

  onMount(async () => {
    try {
      const home = await invoke<string>('local_get_home_dir');
      await loadLocalDir(home);
    } catch {
      await loadLocalDir('C:\\');
    }

    if (session.connectionId) {
      await initSftp();
    }
  });

  onDestroy(() => {
    if (isConnected) {
      invoke('sftp_disconnect', { tabId: session.id }).catch(() => {});
    }
  });
</script>

<div class="sftp-frame">
  <!-- Top Navigation & Action Bar -->
  <div class="sftp-nav">
    <div class="nav-left">
      <div class="session-badge">
        <Server size={13} class="badge-icon" />
        <span class="badge-text">{session.badge || 'SFTP'}</span>
      </div>
      {#if errorMessage}
        <div class="error-pill" title={errorMessage}>
          <AlertCircle size={12} />
          <span>{errorMessage}</span>
          <button class="clear-err" onclick={() => (errorMessage = null)}>×</button>
        </div>
      {/if}
    </div>

    <div class="nav-right">
      {#if transfers.length > 0}
        <button
          class="transfers-trigger {transfers.some((t) => t.status === 'transferring') ? 'active-transfer' : ''}"
          onclick={() => (isTransfersExpanded = !isTransfersExpanded)}
        >
          {#if transfers.some((t) => t.status === 'transferring')}
            <Loader2 size={12} class="spin" />
          {:else}
            <CheckCircle2 size={12} />
          {/if}
          <span>Transfers ({transfers.length})</span>
        </button>
      {/if}
      <button class="btn-refresh" title="Reconnect SFTP" onclick={initSftp}>
        <RefreshCw size={13} class={isLoadingRemote ? 'spin' : ''} />
      </button>
    </div>
  </div>

  <!-- Dual Split Panes -->
  <div class="panes-row">
    <!-- Local Pane (Left) -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="file-pane"
      ondragover={(e) => e.preventDefault()}
      ondrop={(e) => {
        e.preventDefault();
        handleDrop('local');
      }}
    >
      <div class="pane-header">
        <div class="pane-header-left">
          <HardDrive size={13} class="pane-icon" />
          <span class="pane-label">Local Filesystem</span>
        </div>
        <div class="pane-header-actions">
          <button class="icon-btn" title="Go Up" onclick={goUpLocal}>
            <CornerLeftUp size={13} />
          </button>
          <button class="icon-btn" title="New Folder" onclick={() => openNewFolderModal('local')}>
            <FolderPlus size={13} />
          </button>
          <button class="icon-btn" title="Refresh" onclick={() => loadLocalDir(localPath)}>
            <RefreshCw size={12} class={isLoadingLocal ? 'spin' : ''} />
          </button>
        </div>
      </div>

      <div class="pane-path-bar">
        <input
          type="text"
          class="path-input"
          bind:value={localPath}
          onkeydown={(e) => {
            if (e.key === 'Enter') loadLocalDir(localPath);
          }}
        />
        <div class="search-wrap">
          <Search size={11} class="search-icon" />
          <input
            type="text"
            placeholder="Filter files..."
            class="filter-input"
            bind:value={localSearch}
          />
        </div>
      </div>

      <!-- File List -->
      <div class="file-list-wrap">
        <div class="file-table-header">
          <span class="col-name">Name</span>
          <span class="col-size">Size</span>
          <span class="col-date">Modified</span>
          <span class="col-actions">Actions</span>
        </div>

        {#if isLoadingLocal}
          <div class="pane-loading">
            <Loader2 size={16} class="spin" />
            <span>Loading local directory...</span>
          </div>
        {:else if filteredLocalFiles.length === 0}
          <div class="empty-state">No files in directory</div>
        {:else}
          <div class="file-rows">
            {#each filteredLocalFiles as file}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                class="file-row {selectedLocal?.path === file.path ? 'selected' : ''}"
                draggable="true"
                ondragstart={() => handleDragStart('local', file)}
                onclick={() => (selectedLocal = file)}
                ondblclick={() => {
                  if (file.is_dir) {
                    loadLocalDir(file.path);
                  }
                }}
              >
                <div class="col-name file-name-cell">
                  <span class="file-type-icon {file.is_dir ? 'dir-icon' : ''}">
                    {#if file.is_dir}
                      <Folder size={13} />
                    {:else}
                      <FileText size={13} />
                    {/if}
                  </span>
                  <span class="file-name-text" title={file.name}>{file.name}</span>
                </div>
                <div class="col-size">{file.is_dir ? '—' : formatBytes(file.size)}</div>
                <div class="col-date">{file.modified || '—'}</div>
                <div class="col-actions">
                  {#if !file.is_dir}
                    <button
                      class="row-btn upload"
                      title="Upload to remote"
                      onclick={(e) => {
                        e.stopPropagation();
                        uploadFile(file);
                      }}
                    >
                      <Upload size={11} />
                    </button>
                  {/if}
                  <button
                    class="row-btn"
                    title="Rename"
                    onclick={(e) => {
                      e.stopPropagation();
                      openRenameModal('local', file);
                    }}
                  >
                    r
                  </button>
                  <button
                    class="row-btn danger"
                    title="Delete"
                    onclick={(e) => {
                      e.stopPropagation();
                      openDeleteModal('local', file);
                    }}
                  >
                    <Trash2 size={11} />
                  </button>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </div>

    <!-- Center Action Divider -->
    <div class="center-actions">
      <button
        class="transfer-btn"
        title="Upload selected to Remote"
        disabled={!selectedLocal || selectedLocal.is_dir || !isConnected}
        onclick={() => selectedLocal && uploadFile(selectedLocal)}
      >
        <ArrowUp size={14} />
      </button>
      <button
        class="transfer-btn"
        title="Download selected to Local"
        disabled={!selectedRemote || selectedRemote.is_dir || !isConnected}
        onclick={() => selectedRemote && downloadFile(selectedRemote)}
      >
        <ArrowDown size={14} />
      </button>
    </div>

    <!-- Remote Pane (Right) -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="file-pane"
      ondragover={(e) => e.preventDefault()}
      ondrop={(e) => {
        e.preventDefault();
        handleDrop('remote');
      }}
    >
      <div class="pane-header">
        <div class="pane-header-left">
          <Server size={13} class="pane-icon" />
          <span class="pane-label">Remote SFTP</span>
          {#if isConnected}
            <span class="online-dot" title="Connected"></span>
          {:else}
            <span class="offline-dot" title="Disconnected"></span>
          {/if}
        </div>
        <div class="pane-header-actions">
          <button class="icon-btn" title="Go Up" disabled={!isConnected} onclick={goUpRemote}>
            <CornerLeftUp size={13} />
          </button>
          <button
            class="icon-btn"
            title="New Remote Folder"
            disabled={!isConnected}
            onclick={() => openNewFolderModal('remote')}
          >
            <FolderPlus size={13} />
          </button>
          <button
            class="icon-btn"
            title="Refresh Remote"
            disabled={!isConnected}
            onclick={() => loadRemoteDir(remotePath)}
          >
            <RefreshCw size={12} class={isLoadingRemote ? 'spin' : ''} />
          </button>
        </div>
      </div>

      <div class="pane-path-bar">
        <input
          type="text"
          class="path-input"
          bind:value={remotePath}
          disabled={!isConnected}
          onkeydown={(e) => {
            if (e.key === 'Enter') loadRemoteDir(remotePath);
          }}
        />
        <div class="search-wrap">
          <Search size={11} class="search-icon" />
          <input
            type="text"
            placeholder="Filter remote..."
            class="filter-input"
            bind:value={remoteSearch}
          />
        </div>
      </div>

      <!-- File List -->
      <div class="file-list-wrap">
        <div class="file-table-header">
          <span class="col-name">Name</span>
          <span class="col-size">Size</span>
          <span class="col-date">Modified</span>
          <span class="col-actions">Actions</span>
        </div>

        {#if !isConnected && !isLoadingRemote}
          <div class="pane-empty">
            <span>Remote SFTP disconnected</span>
            <button class="connect-action-btn" onclick={initSftp}>Connect Now</button>
          </div>
        {:else if isLoadingRemote}
          <div class="pane-loading">
            <Loader2 size={16} class="spin" />
            <span>Loading remote directory...</span>
          </div>
        {:else if filteredRemoteFiles.length === 0}
          <div class="empty-state">No files in directory</div>
        {:else}
          <div class="file-rows">
            {#each filteredRemoteFiles as file}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                class="file-row {selectedRemote?.path === file.path ? 'selected' : ''}"
                draggable="true"
                ondragstart={() => handleDragStart('remote', file)}
                onclick={() => (selectedRemote = file)}
                ondblclick={() => {
                  if (file.is_dir) {
                    loadRemoteDir(file.path);
                  }
                }}
              >
                <div class="col-name file-name-cell">
                  <span class="file-type-icon {file.is_dir ? 'dir-icon' : ''}">
                    {#if file.is_dir}
                      <Folder size={13} />
                    {:else}
                      <FileText size={13} />
                    {/if}
                  </span>
                  <span class="file-name-text" title={file.name}>{file.name}</span>
                </div>
                <div class="col-size">{file.is_dir ? '—' : formatBytes(file.size)}</div>
                <div class="col-date">{file.modified || '—'}</div>
                <div class="col-actions">
                  {#if !file.is_dir}
                    <button
                      class="row-btn download"
                      title="Download to local"
                      onclick={(e) => {
                        e.stopPropagation();
                        downloadFile(file);
                      }}
                    >
                      <Download size={11} />
                    </button>
                  {/if}
                  <button
                    class="row-btn"
                    title="Rename"
                    onclick={(e) => {
                      e.stopPropagation();
                      openRenameModal('remote', file);
                    }}
                  >
                    r
                  </button>
                  <button
                    class="row-btn danger"
                    title="Delete"
                    onclick={(e) => {
                      e.stopPropagation();
                      openDeleteModal('remote', file);
                    }}
                  >
                    <Trash2 size={11} />
                  </button>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  </div>

  <!-- Bottom Transfers Bar / Drawer -->
  {#if transfers.length > 0 && isTransfersExpanded}
    <div class="transfers-drawer">
      <div class="drawer-header">
        <span class="drawer-title">File Transfers</span>
        <button class="icon-btn" onclick={() => (isTransfersExpanded = false)}>
          <X size={13} />
        </button>
      </div>
      <div class="transfer-list">
        {#each transfers as t}
          <div class="transfer-row">
            <div class="t-direction">
              {#if t.direction === 'upload'}
                <ArrowUp size={12} class="up-arrow" />
              {:else}
                <ArrowDown size={12} class="down-arrow" />
              {/if}
              <span class="t-type">{t.direction.toUpperCase()}</span>
            </div>
            <div class="t-paths">
              <span class="t-source" title={t.source}>{t.source.split(/[/\\]/).pop()}</span>
              <ChevronRight size={11} class="t-arrow" />
              <span class="t-dest" title={t.dest}>{t.dest}</span>
            </div>
            <div class="t-status">
              {#if t.status === 'transferring'}
                <span class="status-badge running"><Loader2 size={11} class="spin" /> Transferring</span>
              {:else if t.status === 'completed'}
                <span class="status-badge success">Completed</span>
              {:else if t.status === 'failed'}
                <span class="status-badge failed" title={t.error}>Failed</span>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  <!-- Action Modal Dialog (New Folder / Rename / Delete) -->
  {#if modalAction}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="modal-backdrop" onclick={closeModal}>
      <div class="modal-box" onclick={(e) => e.stopPropagation()}>
        <div class="modal-header">
          <span class="modal-title">
            {#if modalAction === 'new_folder'}
              Create Folder ({modalTarget.toUpperCase()})
            {:else if modalAction === 'rename'}
              Rename Item ({modalTarget.toUpperCase()})
            {:else if modalAction === 'delete'}
              Confirm Delete ({modalTarget.toUpperCase()})
            {/if}
          </span>
          <button class="icon-btn" onclick={closeModal}><X size={14} /></button>
        </div>

        <div class="modal-body">
          {#if modalAction === 'delete'}
            <p class="delete-warning">
              Are you sure you want to permanently delete
              <strong>{modalTargetFile?.name}</strong>?
            </p>
          {:else}
            <label class="field-label" for="sftp-modal-input">
              {modalAction === 'new_folder' ? 'Folder Name' : 'New Name'}
            </label>
            <input
              id="sftp-modal-input"
              type="text"
              class="text-input"
              bind:value={modalInput}
              onkeydown={(e) => {
                if (e.key === 'Enter') handleModalSubmit();
                if (e.key === 'Escape') closeModal();
              }}
            />
          {/if}
        </div>

        <div class="modal-actions">
          <button class="btn btn-secondary" onclick={closeModal}>Cancel</button>
          <button
            class="btn {modalAction === 'delete' ? 'btn-danger' : 'btn-primary'}"
            onclick={handleModalSubmit}
          >
            {modalAction === 'delete' ? 'Delete' : 'Save'}
          </button>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .sftp-frame {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    background: #000000;
    color: #ededed;
    overflow: hidden;
    font-family: var(--font-sans, -apple-system, sans-serif);
  }

  .sftp-nav {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 38px;
    padding: 0 12px;
    border-bottom: 1px solid #262626;
    background: #0a0a0a;
  }

  .nav-left,
  .nav-right {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .session-badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 8px;
    background: #141414;
    border: 1px solid #262626;
    border-radius: 4px;
    font-family: var(--font-mono, monospace);
    font-size: 11px;
    color: #a1a1a1;
  }

  :global(.badge-icon) {
    color: #0070f3;
  }

  .error-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 2px 8px;
    background: rgba(229, 72, 77, 0.1);
    border: 1px solid rgba(229, 72, 77, 0.3);
    border-radius: 4px;
    font-size: 11px;
    color: #e5484d;
    max-width: 400px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .clear-err {
    background: none;
    border: none;
    color: #e5484d;
    cursor: pointer;
    font-size: 14px;
    line-height: 1;
    padding: 0;
  }

  .transfers-trigger {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: #141414;
    border: 1px solid #262626;
    border-radius: 4px;
    padding: 4px 8px;
    font-size: 11px;
    color: #a1a1a1;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .transfers-trigger:hover,
  .active-transfer {
    color: #ffffff;
    border-color: #404040;
  }

  .btn-refresh {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: 1px solid #262626;
    border-radius: 4px;
    color: #888888;
    cursor: pointer;
  }

  .btn-refresh:hover {
    color: #ffffff;
    border-color: #404040;
  }

  /* Dual Panes */
  .panes-row {
    flex: 1;
    display: flex;
    overflow: hidden;
  }

  .file-pane {
    flex: 1;
    display: flex;
    flex-direction: column;
    background: #000000;
    overflow: hidden;
    min-width: 0;
  }

  .pane-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 34px;
    padding: 0 10px;
    background: #0d0d0d;
    border-bottom: 1px solid #222222;
  }

  .pane-header-left {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  :global(.pane-icon) {
    color: #888888;
  }

  .pane-label {
    font-size: 11px;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: #888888;
  }

  .online-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #30a46c;
  }

  .offline-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #e5484d;
  }

  .pane-header-actions {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    color: #888888;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .icon-btn:hover:not(:disabled) {
    color: #ededed;
    border-color: #333333;
    background: #141414;
  }

  .icon-btn:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }

  .pane-path-bar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    border-bottom: 1px solid #1a1a1a;
    background: #080808;
  }

  .path-input {
    flex: 1;
    background: #121212;
    border: 1px solid #262626;
    border-radius: 4px;
    padding: 4px 8px;
    font-family: var(--font-mono, monospace);
    font-size: 11px;
    color: #ededed;
    outline: none;
    min-width: 0;
  }

  .path-input:focus {
    border-color: #555555;
  }

  .search-wrap {
    position: relative;
    width: 140px;
  }

  :global(.search-icon) {
    position: absolute;
    left: 7px;
    top: 50%;
    transform: translateY(-50%);
    color: #666666;
  }

  .filter-input {
    width: 100%;
    padding: 4px 8px 4px 24px;
    background: #121212;
    border: 1px solid #262626;
    border-radius: 4px;
    font-size: 11px;
    color: #ededed;
    outline: none;
  }

  .filter-input:focus {
    border-color: #555555;
  }

  /* File Table */
  .file-list-wrap {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .file-table-header {
    display: grid;
    grid-template-columns: minmax(180px, 1fr) 75px 125px 65px;
    padding: 6px 12px;
    border-bottom: 1px solid #1a1a1a;
    font-size: 10px;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: #666666;
  }

  .file-rows {
    flex: 1;
    overflow-y: auto;
  }

  .file-row {
    display: grid;
    grid-template-columns: minmax(180px, 1fr) 75px 125px 65px;
    align-items: center;
    padding: 5px 12px;
    border-bottom: 1px solid #111111;
    font-size: 11px;
    cursor: default;
    user-select: none;
    transition: background 0.1s ease;
  }

  .file-row:hover {
    background: #111111;
  }

  .file-row.selected {
    background: #171717;
  }

  .file-name-cell {
    display: flex;
    align-items: center;
    gap: 8px;
    overflow: hidden;
  }

  .file-type-icon {
    display: flex;
    align-items: center;
    color: #888888;
    flex-shrink: 0;
  }

  .dir-icon {
    color: #0070f3;
  }

  .file-name-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #ededed;
  }

  .col-size,
  .col-date {
    font-family: var(--font-mono, monospace);
    font-size: 10px;
    color: #777777;
  }

  .col-actions {
    display: flex;
    align-items: center;
    gap: 4px;
    justify-content: flex-end;
  }

  .row-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 3px;
    color: #777777;
    cursor: pointer;
    font-size: 10px;
  }

  .row-btn:hover {
    background: #222222;
    color: #ededed;
  }

  .row-btn.upload:hover {
    color: #30a46c;
  }

  .row-btn.download:hover {
    color: #0070f3;
  }

  .row-btn.danger:hover {
    color: #e5484d;
  }

  .pane-loading,
  .empty-state,
  .pane-empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: #666666;
    font-size: 12px;
  }

  .connect-action-btn {
    padding: 6px 14px;
    background: #ffffff;
    color: #000000;
    border: none;
    border-radius: 4px;
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
  }

  /* Center Action Divider */
  .center-actions {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    width: 38px;
    background: #080808;
    border-left: 1px solid #1a1a1a;
    border-right: 1px solid #1a1a1a;
  }

  .transfer-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    background: #141414;
    border: 1px solid #262626;
    border-radius: 4px;
    color: #ededed;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .transfer-btn:hover:not(:disabled) {
    background: #222222;
    border-color: #404040;
  }

  .transfer-btn:disabled {
    opacity: 0.25;
    cursor: not-allowed;
  }

  /* Transfers Drawer */
  .transfers-drawer {
    border-top: 1px solid #262626;
    background: #0a0a0a;
    max-height: 160px;
    display: flex;
    flex-direction: column;
    font-size: 11px;
  }

  .drawer-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 12px;
    border-bottom: 1px solid #1a1a1a;
  }

  .drawer-title {
    font-size: 10px;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: #888888;
  }

  .transfer-list {
    overflow-y: auto;
    max-height: 120px;
  }

  .transfer-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 12px;
    border-bottom: 1px solid #141414;
    font-family: var(--font-mono, monospace);
  }

  .t-direction {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 10px;
    width: 90px;
  }

  :global(.up-arrow) {
    color: #30a46c;
  }

  :global(.down-arrow) {
    color: #0070f3;
  }

  .t-paths {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 6px;
    overflow: hidden;
  }

  .t-source,
  .t-dest {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #a1a1a1;
  }

  :global(.t-arrow) {
    color: #555555;
    flex-shrink: 0;
  }

  .status-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 6px;
    border-radius: 3px;
    font-size: 10px;
  }

  .status-badge.running {
    background: rgba(0, 112, 243, 0.15);
    color: #0070f3;
  }

  .status-badge.success {
    background: rgba(48, 164, 108, 0.15);
    color: #30a46c;
  }

  .status-badge.failed {
    background: rgba(229, 72, 77, 0.15);
    color: #e5484d;
  }

  /* Modal */
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    backdrop-filter: blur(2px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .modal-box {
    width: 360px;
    background: #0a0a0a;
    border: 1px solid #262626;
    border-radius: 6px;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .modal-title {
    font-size: 13px;
    font-weight: 500;
    color: #ededed;
  }

  .field-label {
    display: block;
    font-size: 11px;
    color: #888888;
    margin-bottom: 6px;
  }

  .text-input {
    width: 100%;
    padding: 6px 10px;
    background: #141414;
    border: 1px solid #262626;
    border-radius: 4px;
    color: #ededed;
    font-size: 12px;
    outline: none;
  }

  .text-input:focus {
    border-color: #555555;
  }

  .delete-warning {
    font-size: 12px;
    color: #a1a1a1;
    line-height: 1.4;
  }

  .delete-warning strong {
    color: #ffffff;
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 4px;
  }

  .btn {
    padding: 5px 12px;
    border-radius: 4px;
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
  }

  .btn-secondary {
    background: transparent;
    border: 1px solid #262626;
    color: #a1a1a1;
  }

  .btn-secondary:hover {
    color: #ededed;
    border-color: #404040;
  }

  .btn-primary {
    background: #ffffff;
    border: none;
    color: #000000;
  }

  .btn-primary:hover {
    opacity: 0.9;
  }

  .btn-danger {
    background: #e5484d;
    border: none;
    color: #ffffff;
  }

  .btn-danger:hover {
    opacity: 0.9;
  }

  :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
  }
</style>
