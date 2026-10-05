<script lang="ts">
  import type { TabSession } from '../types';
  import {
    Folder,
    FileText,
    ArrowUp,
    ArrowDown,
    RefreshCw,
    FolderPlus,
    HardDrive,
    Server
  } from '@lucide/svelte';

  let { session }: { session: TabSession } = $props();

  let localFiles = [
    { name: 'Documents', isDir: true, size: '—', modified: '2026-10-01' },
    { name: 'Projects', isDir: true, size: '—', modified: '2026-10-04' },
    { name: 'ssh_config', isDir: false, size: '2.4 KB', modified: '2026-09-12' },
    { name: 'id_ed25519.pub', isDir: false, size: '412 B', modified: '2026-08-10' },
  ];

  let remoteFiles = [
    { name: 'var', isDir: true, size: '—', modified: '2026-09-20' },
    { name: 'etc', isDir: true, size: '—', modified: '2026-09-25' },
    { name: 'nginx.conf', isDir: false, size: '4.8 KB', modified: '2026-10-02' },
    { name: 'docker-compose.yml', isDir: false, size: '1.2 KB', modified: '2026-10-05' },
    { name: 'app_backup.tar.gz', isDir: false, size: '45.2 MB', modified: '2026-10-04' },
  ];
</script>

<div class="sftp-frame">
  <!-- Minimalist Toolbar -->
  <div class="sftp-nav">
    <div class="nav-actions">
      <button class="btn btn-secondary btn-compact">
        <ArrowUp size={13} />
        <span>Upload</span>
      </button>
      <button class="btn btn-secondary btn-compact">
        <ArrowDown size={13} />
        <span>Download</span>
      </button>
      <button class="btn-icon" title="New Folder">
        <FolderPlus size={14} />
      </button>
      <button class="btn-icon" title="Refresh">
        <RefreshCw size={13} />
      </button>
    </div>
    <div class="nav-meta">
      <span class="mono-eyebrow">{session.badge || 'SFTP SESSION'}</span>
    </div>
  </div>

  <!-- Dual Split Panes -->
  <div class="panes-row">
    <!-- Local Pane -->
    <div class="file-pane">
      <div class="pane-bar">
        <HardDrive size={13} class="icon-subtle" />
        <span class="pane-title">Local</span>
        <span class="path-tag">C:\Users\jlmanohar</span>
      </div>
      <div class="file-grid">
        {#each localFiles as file}
          <div class="file-item">
            <span class="file-icon">
              {#if file.isDir}
                <Folder size={14} />
              {:else}
                <FileText size={14} />
              {/if}
            </span>
            <span class="file-name">{file.name}</span>
            <span class="file-size">{file.size}</span>
            <span class="file-date">{file.modified}</span>
          </div>
        {/each}
      </div>
    </div>

    <!-- Remote Pane -->
    <div class="file-pane remote-pane">
      <div class="pane-bar">
        <Server size={13} class="icon-subtle" />
        <span class="pane-title">Remote</span>
        <span class="path-tag">/home/deploy</span>
      </div>
      <div class="file-grid">
        {#each remoteFiles as file}
          <div class="file-item">
            <span class="file-icon">
              {#if file.isDir}
                <Folder size={14} />
              {:else}
                <FileText size={14} />
              {/if}
            </span>
            <span class="file-name">{file.name}</span>
            <span class="file-size">{file.size}</span>
            <span class="file-date">{file.modified}</span>
          </div>
        {/each}
      </div>
    </div>
  </div>
</div>

<style>
  .sftp-frame {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--canvas);
  }

  .sftp-nav {
    height: 42px;
    padding: 0 14px;
    background: var(--canvas);
    border-bottom: 1px solid var(--hairline);
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .nav-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .btn-compact {
    height: 28px;
    font-size: 12px;
    padding: 0 10px;
  }

  .panes-row {
    flex: 1;
    display: flex;
    overflow: hidden;
  }

  .file-pane {
    flex: 1;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--hairline);
    background: var(--canvas);
  }

  .remote-pane {
    border-right: none;
    background: var(--canvas-subtle);
  }

  .pane-bar {
    height: 34px;
    padding: 0 14px;
    background: var(--canvas);
    border-bottom: 1px solid var(--hairline);
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .pane-title {
    font-size: 12px;
    font-weight: 500;
    color: var(--ink);
  }

  .path-tag {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--mute);
    margin-left: auto;
  }

  .file-grid {
    flex: 1;
    overflow-y: auto;
    padding: 6px 8px;
  }

  .file-item {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 8px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background 0.12s ease;
  }

  .file-item:hover {
    background: var(--canvas-hover);
  }

  .file-icon {
    color: var(--mute);
    display: flex;
  }

  .file-name {
    font-size: 12px;
    color: var(--ink-secondary);
    flex: 1;
  }

  .file-size {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--mute);
    width: 65px;
    text-align: right;
  }

  .file-date {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--mute);
    width: 85px;
    text-align: right;
  }

  :global(.icon-subtle) {
    color: var(--mute);
  }
</style>
