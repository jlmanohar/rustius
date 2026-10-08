<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { openPath } from '@tauri-apps/plugin-opener';
  import type { FileEntry } from '../types';
  import {
    Folder,
    FileText,
    CornerLeftUp,
    Home,
    RefreshCw,
    FolderPlus,
    Upload,
    Download,
    Trash2,
    Edit2,
    Search,
    X,
    Loader2,
    ChevronRight,
    AlertCircle,
    CheckCircle2,
    FileCode,
    FileArchive,
    FileImage,
    PanelLeftClose,
    FolderSync
  } from '@lucide/svelte';
  import { appState } from '../stores/appState.svelte';

  let {
    tabId,
    connectionId,
    terminalCwd = null,
    onclose,
    onresize
  }: {
    tabId: string;
    connectionId: string;
    terminalCwd?: string | null;
    onclose: () => void;
    onresize?: () => void;
  } = $props();

  let followTerminal = $state<boolean>(
    appState.config.app_settings?.sftp_follow_terminal_folder ?? true
  );

  function toggleFollowTerminal() {
    followTerminal = !followTerminal;
  }

  let cwdDebounceTimer: number | null = null;
  let lastAttemptedPath = '';

  // Reactively track terminal working directory when followTerminal is active (debounced by 350ms)
  $effect(() => {
    const cwd = terminalCwd;
    if (!followTerminal || !isConnected || !cwd) return;

    let target = cwd.trim();
    if (target === '~') {
      if (!initialHomePath) return;
      target = initialHomePath;
    } else if (target.startsWith('~/')) {
      if (!initialHomePath) return;
      target = `${initialHomePath}/${target.slice(2)}`;
    }

    if (target.length > 1 && target.endsWith('/')) {
      target = target.slice(0, -1);
    }

    const normCurrent = currentPath.length > 1 && currentPath.endsWith('/') ? currentPath.slice(0, -1) : currentPath;

    if (target && target !== normCurrent && target !== lastAttemptedPath) {
      if (cwdDebounceTimer) {
        window.clearTimeout(cwdDebounceTimer);
      }
      cwdDebounceTimer = window.setTimeout(() => {
        if (!followTerminal || !isConnected || isLoading) return;
        lastAttemptedPath = target;
        loadDir(target);
      }, 350);
    }
  });

  let currentPath = $state<string>('');
  let initialHomePath = $state<string>('');
  let pathInput = $state<string>('');
  let isEditingPath = $state<boolean>(false);
  let files = $state<FileEntry[]>([]);
  let searchQuery = $state<string>('');
  let isLoading = $state<boolean>(false);
  let isConnected = $state<boolean>(false);
  let errorMessage = $state<string | null>(null);
  let selectedFile = $state<FileEntry | null>(null);

  // Transfer notification toast
  let transferToast = $state<{
    type: 'loading' | 'success' | 'error';
    message: string;
    localPath?: string;
  } | null>(null);
  let toastTimer: number | null = null;

  // Modals
  type ModalType = 'new_folder' | 'rename' | 'delete';
  let activeModal = $state<ModalType | null>(null);
  let targetFile = $state<FileEntry | null>(null);
  let modalInputValue = $state<string>('');

  // Drag & drop
  let isDraggingOver = $state<boolean>(false);
  let fileInputRef = $state<HTMLInputElement | null>(null);

  function showToast(type: 'loading' | 'success' | 'error', message: string, localPath?: string, autoDismiss = true) {
    if (toastTimer) {
      window.clearTimeout(toastTimer);
      toastTimer = null;
    }
    transferToast = { type, message, localPath };
    if (autoDismiss && type !== 'loading') {
      toastTimer = window.setTimeout(() => {
        transferToast = null;
      }, 4000);
    }
  }

  function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
  }

  function getFileIcon(file: FileEntry) {
    if (file.is_dir) return Folder;
    const ext = file.name.split('.').pop()?.toLowerCase();
    if (['zip', 'tar', 'gz', 'bz2', 'xz', '7z', 'rar'].includes(ext || '')) return FileArchive;
    if (['png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'ico'].includes(ext || '')) return FileImage;
    if (['rs', 'ts', 'js', 'json', 'py', 'go', 'c', 'cpp', 'h', 'html', 'css', 'svelte', 'sh', 'toml', 'yaml', 'yml'].includes(ext || '')) return FileCode;
    return FileText;
  }

  let filteredFiles = $derived.by(() => {
    if (!searchQuery.trim()) return files;
    const q = searchQuery.toLowerCase();
    return files.filter((f) => f.name.toLowerCase().includes(q));
  });

  async function initSession() {
    if (!connectionId) return;
    isLoading = true;
    errorMessage = null;
    try {
      const homePath = await invoke<string>('sftp_connect', {
        tabId,
        connectionId,
      });
      isConnected = true;
      initialHomePath = homePath;
      await loadDir(homePath);
    } catch (err: any) {
      isConnected = false;
      errorMessage = err?.message || String(err);
    } finally {
      isLoading = false;
    }
  }

  async function loadDir(path: string) {
    isLoading = true;
    errorMessage = null;
    try {
      const list = await invoke<FileEntry[]>('sftp_list_dir', {
        tabId,
        path,
      });
      files = list;
      currentPath = path;
      pathInput = path;
      selectedFile = null;
    } catch (err: any) {
      errorMessage = err?.message || String(err);
    } finally {
      isLoading = false;
    }
  }

  function goUp() {
    if (!currentPath || currentPath === '/') return;
    const parts = currentPath.replace(/\/+$/, '').split('/');
    parts.pop();
    const parent = parts.join('/') || '/';
    loadDir(parent);
  }

  function goHome() {
    if (initialHomePath) {
      loadDir(initialHomePath);
    }
  }

  function refresh() {
    if (currentPath) {
      loadDir(currentPath);
    }
  }

  function handlePathSubmit() {
    isEditingPath = false;
    const p = pathInput.trim();
    if (p && p !== currentPath) {
      loadDir(p);
    }
  }

  function handleItemClick(file: FileEntry) {
    selectedFile = file;
  }

  function handleItemDblClick(file: FileEntry) {
    if (file.is_dir) {
      loadDir(file.path);
    } else {
      downloadFile(file);
    }
  }

  async function downloadFile(file: FileEntry) {
    if (file.is_dir) return;
    showToast('loading', `Downloading ${file.name}...`, undefined, false);
    try {
      const localPath = await invoke<string>('sftp_download_to_downloads', {
        tabId,
        remotePath: file.path,
      });
      showToast('success', `Saved ${file.name} to Downloads`, localPath);
    } catch (err: any) {
      showToast('error', `Download failed: ${err?.message || err}`);
    }
  }

  // Upload handling via file picker or drag & drop
  function triggerUploadPicker() {
    fileInputRef?.click();
  }

  async function handleFileInputChange(event: Event) {
    const input = event.target as HTMLInputElement;
    if (!input.files || input.files.length === 0) return;
    const fileList = Array.from(input.files);
    await uploadFileList(fileList);
    input.value = '';
  }

  async function uploadFileList(fileList: File[]) {
    for (const file of fileList) {
      showToast('loading', `Uploading ${file.name}...`, undefined, false);
      try {
        const arrayBuf = await file.arrayBuffer();
        const bytes = Array.from(new Uint8Array(arrayBuf));
        const remoteDest = currentPath.endsWith('/')
          ? `${currentPath}${file.name}`
          : `${currentPath}/${file.name}`;

        await invoke('sftp_upload_bytes', {
          tabId,
          remotePath: remoteDest,
          data: bytes,
        });
        showToast('success', `Uploaded ${file.name}`);
      } catch (err: any) {
        showToast('error', `Upload failed: ${err?.message || err}`);
      }
    }
    await loadDir(currentPath);
  }

  function handleDragOver(e: DragEvent) {
    e.preventDefault();
    isDraggingOver = true;
  }

  function handleDragLeave(e: DragEvent) {
    e.preventDefault();
    isDraggingOver = false;
  }

  async function handleDrop(e: DragEvent) {
    e.preventDefault();
    isDraggingOver = false;
    if (e.dataTransfer?.files && e.dataTransfer.files.length > 0) {
      const fileList = Array.from(e.dataTransfer.files);
      await uploadFileList(fileList);
    }
  }

  // Modals
  function openNewFolderModal() {
    activeModal = 'new_folder';
    modalInputValue = '';
    targetFile = null;
  }

  function openRenameModal(file: FileEntry, e?: Event) {
    e?.stopPropagation();
    activeModal = 'rename';
    targetFile = file;
    modalInputValue = file.name;
  }

  function openDeleteModal(file: FileEntry, e?: Event) {
    e?.stopPropagation();
    activeModal = 'delete';
    targetFile = file;
  }

  function closeModal() {
    activeModal = null;
    targetFile = null;
    modalInputValue = '';
  }

  async function submitModal() {
    if (!activeModal) return;
    try {
      if (activeModal === 'new_folder') {
        const name = modalInputValue.trim();
        if (!name) return;
        const p = currentPath.endsWith('/') ? `${currentPath}${name}` : `${currentPath}/${name}`;
        await invoke('sftp_mkdir', { tabId, path: p });
        await loadDir(currentPath);
      } else if (activeModal === 'rename' && targetFile) {
        const newName = modalInputValue.trim();
        if (!newName || newName === targetFile.name) return;
        const parts = targetFile.path.replace(/\/+$/, '').split('/');
        parts.pop();
        const parent = parts.join('/') || '/';
        const newPath = parent.endsWith('/') ? `${parent}${newName}` : `${parent}/${newName}`;
        await invoke('sftp_rename', { tabId, from: targetFile.path, to: newPath });
        await loadDir(currentPath);
      } else if (activeModal === 'delete' && targetFile) {
        await invoke('sftp_remove', {
          tabId,
          path: targetFile.path,
          isDir: targetFile.is_dir,
        });
        await loadDir(currentPath);
      }
      closeModal();
    } catch (err: any) {
      errorMessage = `Action failed: ${err?.message || err}`;
    }
  }

  onMount(() => {
    initSession();
  });

  onDestroy(() => {
    if (toastTimer) {
      window.clearTimeout(toastTimer);
    }
    if (cwdDebounceTimer) {
      window.clearTimeout(cwdDebounceTimer);
    }
    invoke('sftp_disconnect', { tabId }).catch(() => {});
  });
</script>

<div
  class="sftp-panel"
  ondragover={handleDragOver}
  ondragleave={handleDragLeave}
  ondrop={handleDrop}
  role="region"
  aria-label="SFTP File Navigator"
>
  <!-- Hidden File Input for Native Upload Picker -->
  <input
    type="file"
    multiple
    bind:this={fileInputRef}
    onchange={handleFileInputChange}
    style="display: none;"
  />

  <!-- Top Title Strip -->
  <div class="panel-header">
    <div class="header-title">
      <Folder size={13} class="header-icon" />
      <span>SFTP Navigator</span>
    </div>
    <div class="header-actions">
      <button class="tool-btn" onclick={onclose} title="Collapse SFTP Browser">
        <PanelLeftClose size={13} />
      </button>
    </div>
  </div>

  <!-- Navigation & Action Strip -->
  <div class="action-strip">
    <button class="tool-btn" onclick={goUp} disabled={!isConnected || currentPath === '/'} title="Up to Parent Directory">
      <CornerLeftUp size={13} />
    </button>
    <button class="tool-btn" onclick={goHome} disabled={!isConnected} title="Remote Home">
      <Home size={13} />
    </button>
    <button class="tool-btn" onclick={refresh} disabled={!isConnected || isLoading} title="Refresh Directory">
      <RefreshCw size={13} class={isLoading ? 'spinning' : ''} />
    </button>
    <button
      class="tool-btn {followTerminal ? 'active' : ''}"
      onclick={toggleFollowTerminal}
      disabled={!isConnected}
      title={followTerminal ? "Following terminal directory (click to pause)" : "Follow terminal directory (click to enable)"}
    >
      <FolderSync size={13} />
    </button>
    <span class="strip-divider"></span>
    <button class="tool-btn" onclick={openNewFolderModal} disabled={!isConnected} title="New Folder">
      <FolderPlus size={13} />
    </button>
    <button class="tool-btn" onclick={triggerUploadPicker} disabled={!isConnected} title="Upload Files (or Drag & Drop)">
      <Upload size={13} />
    </button>
  </div>

  <!-- Path Breadcrumb / Bar -->
  <div class="path-bar">
    {#if isEditingPath}
      <form onsubmit={(e) => { e.preventDefault(); handlePathSubmit(); }} class="path-form">
        <input
          type="text"
          class="path-input"
          bind:value={pathInput}
          onblur={handlePathSubmit}
        />
      </form>
    {:else}
      <div
        class="path-display"
        onclick={() => { isEditingPath = true; }}
        role="button"
        tabindex="-1"
        onkeydown={(e) => { if (e.key === 'Enter') isEditingPath = true; }}
        title="Click to edit path"
      >
        <span class="path-text">{currentPath || '/'}</span>
      </div>
    {/if}
  </div>

  <!-- Search Filter -->
  <div class="search-strip">
    <Search size={12} class="search-icon" />
    <input
      type="text"
      placeholder="Filter files..."
      class="search-input"
      bind:value={searchQuery}
    />
    {#if searchQuery}
      <button class="search-clear" onclick={() => searchQuery = ''}>
        <X size={11} />
      </button>
    {/if}
  </div>

  <!-- File List Area -->
  <div class="file-list-container">
    {#if isLoading && files.length === 0}
      <div class="state-msg">
        <Loader2 size={16} class="spinning" />
        <span>Connecting to SFTP subsystem...</span>
      </div>
    {:else if errorMessage}
      <div class="state-msg error">
        <AlertCircle size={16} />
        <span>{errorMessage}</span>
        <button class="retry-btn" onclick={refresh}>Retry</button>
      </div>
    {:else if filteredFiles.length === 0}
      <div class="state-msg muted">
        <span>{searchQuery ? 'No matching files' : 'Directory is empty'}</span>
      </div>
    {:else}
      <div class="file-table" role="list">
        {#each filteredFiles as file (file.path)}
          {@const IconComponent = getFileIcon(file)}
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
          <div
            class="file-row {selectedFile?.path === file.path ? 'selected' : ''}"
            onclick={() => handleItemClick(file)}
            ondblclick={() => handleItemDblClick(file)}
            role="listitem"
          >
            <div class="col-main">
              <IconComponent size={14} class="row-icon {file.is_dir ? 'dir-icon' : 'file-icon'}" />
              <span class="file-name" title={file.name}>{file.name}</span>
            </div>
            
            <div class="col-meta">
              {#if !file.is_dir}
                <span class="file-size">{formatBytes(file.size)}</span>
              {/if}
            </div>

            <!-- Hover quick actions -->
            <div class="row-actions">
              {#if !file.is_dir}
                <button
                  class="action-icon-btn"
                  onclick={(e) => { e.stopPropagation(); downloadFile(file); }}
                  title="Download to Downloads"
                >
                  <Download size={11} />
                </button>
              {/if}
              <button
                class="action-icon-btn"
                onclick={(e) => openRenameModal(file, e)}
                title="Rename"
              >
                <Edit2 size={11} />
              </button>
              <button
                class="action-icon-btn danger"
                onclick={(e) => openDeleteModal(file, e)}
                title="Delete"
              >
                <Trash2 size={11} />
              </button>
            </div>
          </div>
        {/each}
      </div>
    {/if}

    <!-- Drag & drop overlay -->
    {#if isDraggingOver}
      <div class="drop-overlay">
        <Upload size={24} />
        <span>Drop files here to upload to {currentPath}</span>
      </div>
    {/if}
  </div>

  <!-- Transfer notification toast at bottom -->
  {#if transferToast}
    <div class="toast-card {transferToast.type}">
      {#if transferToast.type === 'loading'}
        <Loader2 size={12} class="spinning" />
      {:else if transferToast.type === 'success'}
        <CheckCircle2 size={12} />
      {:else}
        <AlertCircle size={12} />
      {/if}
      <div class="toast-content">
        <span class="toast-msg">{transferToast.message}</span>
        {#if transferToast.localPath}
          <button class="toast-link" onclick={() => openPath(transferToast!.localPath!)}>
            Open in Folder
          </button>
        {/if}
      </div>
      <button class="toast-close" onclick={() => transferToast = null}>
        <X size={10} />
      </button>
    </div>
  {/if}

  <!-- Modals -->
  {#if activeModal}
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
    <div class="modal-backdrop" onclick={closeModal} role="dialog" aria-modal="true">
      <div class="modal-card" onclick={(e) => e.stopPropagation()} role="document">
        <div class="modal-header">
          <span class="modal-title">
            {#if activeModal === 'new_folder'}Create Remote Folder
            {:else if activeModal === 'rename'}Rename '{targetFile?.name}'
            {:else}Delete '{targetFile?.name}'
            {/if}
          </span>
          <button class="tool-btn" onclick={closeModal}><X size={12} /></button>
        </div>

        <div class="modal-body">
          {#if activeModal === 'new_folder'}
            <!-- svelte-ignore a11y_autofocus -->
            <input
              type="text"
              placeholder="Folder name"
              class="modal-input"
              bind:value={modalInputValue}
              autofocus
              onkeydown={(e) => { if (e.key === 'Enter') submitModal(); }}
            />
          {:else if activeModal === 'rename'}
            <!-- svelte-ignore a11y_autofocus -->
            <input
              type="text"
              placeholder="New name"
              class="modal-input"
              bind:value={modalInputValue}
              autofocus
              onkeydown={(e) => { if (e.key === 'Enter') submitModal(); }}
            />
          {:else}
            <p class="modal-warning">
              Are you sure you want to permanently delete this {targetFile?.is_dir ? 'directory and all its contents' : 'file'}?
            </p>
          {/if}
        </div>

        <div class="modal-footer">
          <button class="btn secondary" onclick={closeModal}>Cancel</button>
          <button
            class="btn {activeModal === 'delete' ? 'danger' : 'primary'}"
            onclick={submitModal}
          >
            {#if activeModal === 'new_folder'}Create
            {:else if activeModal === 'rename'}Rename
            {:else}Delete
            {/if}
          </button>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .sftp-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: #09090b;
    border-right: 1px solid #27272a;
    font-family: var(--font-sans, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif);
    color: #e4e4e7;
    user-select: none;
    position: relative;
    overflow: hidden;
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 10px;
    background: #111113;
    border-bottom: 1px solid #27272a;
    min-height: 28px;
  }

  .header-title {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: -0.01em;
    color: #f4f4f5;
  }

  .header-icon {
    color: #f59e0b;
  }

  .action-strip {
    display: flex;
    align-items: center;
    gap: 3px;
    padding: 4px 8px;
    background: #111113;
    border-bottom: 1px solid #27272a;
  }

  .tool-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: #a1a1aa;
    cursor: pointer;
    transition: all 0.12s ease;
  }

  .tool-btn:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.08);
    color: #ffffff;
  }

  .tool-btn.active {
    background: rgba(245, 158, 11, 0.15);
    color: #f59e0b;
  }

  .tool-btn.active:hover:not(:disabled) {
    background: rgba(245, 158, 11, 0.25);
    color: #fbbf24;
  }

  .tool-btn:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }

  .strip-divider {
    width: 1px;
    height: 14px;
    background: #27272a;
    margin: 0 4px;
  }

  .path-bar {
    padding: 4px 8px;
    background: #0c0c0e;
    border-bottom: 1px solid #1f1f23;
  }

  .path-display {
    padding: 3px 6px;
    background: #141417;
    border: 1px solid #27272a;
    border-radius: 4px;
    font-family: var(--font-mono, monospace);
    font-size: 11px;
    color: #d4d4d8;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: text;
    transition: border-color 0.12s ease;
  }

  .path-display:hover {
    border-color: #3f3f46;
  }

  .path-input {
    width: 100%;
    padding: 3px 6px;
    background: #18181b;
    border: 1px solid #3b82f6;
    border-radius: 4px;
    font-family: var(--font-mono, monospace);
    font-size: 11px;
    color: #ffffff;
    outline: none;
    box-sizing: border-box;
  }

  .search-strip {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    border-bottom: 1px solid #1f1f23;
    position: relative;
  }

  .search-icon {
    color: #71717a;
    flex-shrink: 0;
  }

  .search-input {
    width: 100%;
    background: transparent;
    border: none;
    outline: none;
    font-size: 11px;
    color: #e4e4e7;
  }

  .search-input::placeholder {
    color: #52525b;
  }

  .search-clear {
    background: transparent;
    border: none;
    color: #71717a;
    cursor: pointer;
    padding: 2px;
  }

  .file-list-container {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    position: relative;
  }

  .file-table {
    display: flex;
    flex-direction: column;
    padding: 2px 0;
  }

  .file-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 8px;
    font-size: 11.5px;
    color: #d4d4d8;
    cursor: default;
    border-radius: 3px;
    margin: 1px 4px;
    position: relative;
    transition: background 0.1s ease;
  }

  .file-row:hover {
    background: rgba(255, 255, 255, 0.05);
    color: #ffffff;
  }

  .file-row.selected {
    background: rgba(59, 130, 246, 0.2);
    color: #ffffff;
  }

  .col-main {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    flex: 1;
  }

  .row-icon {
    flex-shrink: 0;
  }

  .dir-icon {
    color: #f59e0b;
  }

  .file-icon {
    color: #71717a;
  }

  .file-name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: var(--font-sans, sans-serif);
  }

  .col-meta {
    flex-shrink: 0;
    margin-left: 6px;
    font-size: 10px;
    color: #71717a;
    font-family: var(--font-mono, monospace);
  }

  .row-actions {
    display: none;
    align-items: center;
    gap: 3px;
    flex-shrink: 0;
    margin-left: 4px;
  }

  .file-row:hover .row-actions {
    display: flex;
  }

  .file-row:hover .col-meta {
    display: none;
  }

  .action-icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    background: #18181b;
    border: 1px solid #27272a;
    border-radius: 3px;
    color: #a1a1aa;
    cursor: pointer;
    transition: all 0.1s ease;
  }

  .action-icon-btn:hover {
    background: #27272a;
    color: #ffffff;
  }

  .action-icon-btn.danger:hover {
    background: #ef4444;
    border-color: #ef4444;
    color: #ffffff;
  }

  .state-msg {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 30px 16px;
    text-align: center;
    font-size: 11px;
    color: #a1a1aa;
  }

  .state-msg.error {
    color: #ef4444;
  }

  .state-msg.muted {
    color: #52525b;
  }

  .retry-btn {
    margin-top: 6px;
    padding: 3px 10px;
    background: #18181b;
    border: 1px solid #27272a;
    border-radius: 4px;
    color: #e4e4e7;
    font-size: 10px;
    cursor: pointer;
  }

  .spinning {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .drop-overlay {
    position: absolute;
    inset: 0;
    background: rgba(9, 9, 11, 0.9);
    border: 2px dashed #3b82f6;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    color: #3b82f6;
    font-size: 12px;
    font-weight: 500;
    pointer-events: none;
    z-index: 10;
  }

  /* Toast Notification */
  .toast-card {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    background: #18181b;
    border-top: 1px solid #27272a;
    font-size: 11px;
    color: #e4e4e7;
    flex-shrink: 0;
  }

  .toast-card.success {
    color: #10b981;
  }

  .toast-card.error {
    color: #ef4444;
  }

  .toast-content {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .toast-msg {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .toast-link {
    background: transparent;
    border: none;
    color: #3b82f6;
    font-size: 10px;
    text-align: left;
    padding: 0;
    cursor: pointer;
    text-decoration: underline;
  }

  .toast-close {
    background: transparent;
    border: none;
    color: #71717a;
    cursor: pointer;
  }

  /* Modals */
  .modal-backdrop {
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    backdrop-filter: blur(2px);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px;
    z-index: 50;
  }

  .modal-card {
    width: 100%;
    max-width: 280px;
    background: #18181b;
    border: 1px solid #27272a;
    border-radius: 8px;
    padding: 12px;
    box-shadow: 0 10px 25px rgba(0, 0, 0, 0.5);
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 10px;
  }

  .modal-title {
    font-size: 12px;
    font-weight: 600;
    color: #f4f4f5;
  }

  .modal-body {
    margin-bottom: 12px;
  }

  .modal-input {
    width: 100%;
    padding: 6px 8px;
    background: #09090b;
    border: 1px solid #27272a;
    border-radius: 4px;
    color: #ffffff;
    font-size: 11px;
    outline: none;
    box-sizing: border-box;
  }

  .modal-input:focus {
    border-color: #3b82f6;
  }

  .modal-warning {
    font-size: 11px;
    color: #a1a1aa;
    line-height: 1.4;
    margin: 0;
  }

  .modal-footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 6px;
  }

  .btn {
    padding: 4px 10px;
    border-radius: 4px;
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    border: none;
    transition: all 0.1s ease;
  }

  .btn.secondary {
    background: #27272a;
    color: #d4d4d8;
  }

  .btn.secondary:hover {
    background: #3f3f46;
    color: #ffffff;
  }

  .btn.primary {
    background: #2563eb;
    color: #ffffff;
  }

  .btn.primary:hover {
    background: #1d4ed8;
  }

  .btn.danger {
    background: #dc2626;
    color: #ffffff;
  }

  .btn.danger:hover {
    background: #b91c1c;
  }
</style>
