<script lang="ts">
  import { appState } from '../stores/appState.svelte';
  import type { Connection, ConnectionType, TreeNode } from '../types';
  import {
    Terminal,
    ArrowLeftRight,
    Monitor,
    X,
    Lock,
    Key
  } from '@lucide/svelte';

  let connType = $state<ConnectionType>('ssh');
  let name = $state('');
  let host = $state('');
  let port = $state(22);
  let environment = $state('Production');
  let tagsStr = $state('');
  let parentFolderId = $state<string>('');

  // Auth
  let authType = $state<'password' | 'keyfile'>('password');
  let username = $state('');
  let password = $state('');
  let keyPath = $state('');
  let passphrase = $state('');

  // SSH advanced
  let legacyCiphers = $state(false);
  let legacyKex = $state(false);
  let keepalive = $state(30);

  // RDP options
  let rdpWidth = $state(1920);
  let rdpHeight = $state(1080);
  let clipboardSharing = $state(true);

  let availableFolders = $derived.by(() => {
    const list: { id: string; name: string }[] = [];
    const scan = (nodes: TreeNode[]) => {
      for (const n of nodes) {
        if (n.node_type === 'folder') {
          list.push({ id: n.id, name: n.name });
          scan(n.children);
        }
      }
    };
    scan(appState.config.tree);
    return list;
  });

  function handleTypeChange(type: ConnectionType) {
    connType = type;
    port = type === 'rdp' ? 3389 : 22;
  }

  function handleSave(connectNow: boolean = false) {
    if (!name.trim() || !host.trim()) {
      alert('Session Name and Host address are required.');
      return;
    }

    const tags = tagsStr
      .split(',')
      .map((t) => t.trim())
      .filter((t) => t.length > 0);

    const id = typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'conn-' + Date.now();
    const newConnection: Connection = {
      id,
      name: name.trim(),
      conn_type: connType,
      connection_type: connType,
      host: host.trim(),
      port: Number(port),
      environment: environment.trim() || undefined,
      tags,
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
      auth:
        authType === 'password'
          ? {
              type: 'password',
              username: username.trim(),
              password: password || undefined,
            }
          : {
              type: 'keyfile',
              username: username.trim(),
              key_path: keyPath.trim(),
              passphrase: passphrase || undefined,
            },
      ssh_config:
        connType === 'ssh' || connType === 'sftp'
          ? {
              connect_timeout_secs: 15,
              keepalive_interval_secs: keepalive,
              legacy_ciphers: legacyCiphers,
              legacy_kex: legacyKex,
              port_forwards: [],
            }
          : undefined,
      rdp_config:
        connType === 'rdp'
          ? {
              width: rdpWidth,
              height: rdpHeight,
              color_depth: 24,
              fullscreen: false,
              clipboard_sharing: clipboardSharing,
            }
          : undefined,
    };

    appState.addConnection(newConnection, parentFolderId || null);

    if (connectNow) {
      appState.openTab({
        title: newConnection.name,
        type: newConnection.conn_type,
        connectionId: newConnection.id,
        status: 'connected',
        badge: `${newConnection.conn_type.toUpperCase()} · ${newConnection.host}`,
      });
    }

    appState.isConnectionModalOpen = false;
  }
</script>

<div
  class="modal-scrim"
  role="presentation"
  onclick={() => (appState.isConnectionModalOpen = false)}
>
  <div
    class="modal-card"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
  >
    <!-- Modal Header -->
    <div class="modal-top">
      <div class="modal-titles">
        <span class="mono-eyebrow">CONFIGURATION</span>
        <h2 class="dialog-heading">New Connection</h2>
      </div>
      <button
        class="btn-icon"
        title="Close"
        onclick={() => (appState.isConnectionModalOpen = false)}
      >
        <X size={16} />
      </button>
    </div>

    <!-- Protocol Segment Control -->
    <div class="proto-segment">
      <button
        class="segment-item {connType === 'ssh' ? 'segment-active' : ''}"
        onclick={() => handleTypeChange('ssh')}
      >
        <Terminal size={14} />
        <span>SSH</span>
      </button>
      <button
        class="segment-item {connType === 'sftp' ? 'segment-active' : ''}"
        onclick={() => handleTypeChange('sftp')}
      >
        <ArrowLeftRight size={14} />
        <span>SFTP</span>
      </button>
      <button
        class="segment-item {connType === 'rdp' ? 'segment-active' : ''}"
        onclick={() => handleTypeChange('rdp')}
      >
        <Monitor size={14} />
        <span>RDP</span>
      </button>
    </div>

    <div class="modal-scroll-body">
      <!-- General -->
      <div class="form-grid">
        <div class="form-cell col-2">
          <label for="conn-name">Session Name</label>
          <input
            id="conn-name"
            type="text"
            placeholder="e.g. production-gateway"
            bind:value={name}
            class="input-field"
          />
        </div>
        <div class="form-cell col-1">
          <label for="conn-folder">Folder</label>
          <select id="conn-folder" bind:value={parentFolderId} class="input-field">
            <option value="">Root level</option>
            {#each availableFolders as f}
              <option value={f.id}>{f.name}</option>
            {/each}
          </select>
        </div>
      </div>

      <div class="form-grid">
        <div class="form-cell col-2">
          <label for="conn-host">Host / IP</label>
          <input
            id="conn-host"
            type="text"
            placeholder="e.g. 192.168.1.100 or aws.internal"
            bind:value={host}
            class="input-field"
          />
        </div>
        <div class="form-cell col-1">
          <label for="conn-port">Port</label>
          <input id="conn-port" type="number" bind:value={port} class="input-field" />
        </div>
      </div>

      <div class="form-grid">
        <div class="form-cell col-1">
          <label for="conn-env">Environment</label>
          <input
            id="conn-env"
            type="text"
            placeholder="Production, Staging, Local"
            bind:value={environment}
            class="input-field"
          />
        </div>
        <div class="form-cell col-2">
          <label for="conn-tags">Tags</label>
          <input
            id="conn-tags"
            type="text"
            placeholder="web, us-east-1, primary"
            bind:value={tagsStr}
            class="input-field"
          />
        </div>
      </div>

      <!-- Authentication -->
      <div class="section-divider">
        <span class="mono-eyebrow">AUTHENTICATION</span>
      </div>

      <div class="auth-tabs">
        <button
          class="auth-tab {authType === 'password' ? 'auth-tab-active' : ''}"
          onclick={() => (authType = 'password')}
        >
          <Lock size={12} />
          <span>Password</span>
        </button>
        <button
          class="auth-tab {authType === 'keyfile' ? 'auth-tab-active' : ''}"
          onclick={() => (authType = 'keyfile')}
        >
          <Key size={12} />
          <span>Private Key</span>
        </button>
      </div>

      <div class="form-grid">
        <div class="form-cell col-1">
          <label for="conn-user">Username</label>
          <input
            id="conn-user"
            type="text"
            placeholder="e.g. root"
            bind:value={username}
            class="input-field"
          />
        </div>

        {#if authType === 'password'}
          <div class="form-cell col-2">
            <label for="conn-pass">Password</label>
            <input
              id="conn-pass"
              type="password"
              placeholder="••••••••"
              bind:value={password}
              class="input-field"
            />
          </div>
        {:else}
          <div class="form-cell col-2">
            <label for="conn-key">Key Path</label>
            <input
              id="conn-key"
              type="text"
              placeholder="~/.ssh/id_ed25519"
              bind:value={keyPath}
              class="input-field"
            />
          </div>
        {/if}
      </div>

      {#if authType === 'keyfile'}
        <div class="form-cell">
          <label for="conn-passphrase">Passphrase (optional)</label>
          <input
            id="conn-passphrase"
            type="password"
            placeholder="Key passphrase"
            bind:value={passphrase}
            class="input-field"
          />
        </div>
      {/if}

      <!-- Protocol Specific Options -->
      <div class="section-divider">
        <span class="mono-eyebrow">OPTIONS</span>
      </div>

      {#if connType === 'ssh' || connType === 'sftp'}
        <div class="options-group">
          <label class="check-row">
            <input type="checkbox" bind:checked={legacyCiphers} />
            <div class="check-text">
              <span class="check-title">Legacy Ciphers</span>
              <span class="check-desc">Enable 3DES, Blowfish, AES-CBC for legacy hardware</span>
            </div>
          </label>
          <label class="check-row">
            <input type="checkbox" bind:checked={legacyKex} />
            <div class="check-text">
              <span class="check-title">Legacy Key Exchange</span>
              <span class="check-desc">Enable diffie-hellman-group1-sha1</span>
            </div>
          </label>
        </div>
      {:else if connType === 'rdp'}
        <div class="form-grid">
          <div class="form-cell col-1">
            <label for="rdp-w">Resolution Width</label>
            <input id="rdp-w" type="number" bind:value={rdpWidth} class="input-field" />
          </div>
          <div class="form-cell col-1">
            <label for="rdp-h">Resolution Height</label>
            <input id="rdp-h" type="number" bind:value={rdpHeight} class="input-field" />
          </div>
          <div class="form-cell col-1">
            <label class="check-row" style="margin-top: 24px;">
              <input type="checkbox" bind:checked={clipboardSharing} />
              <span class="check-title">Clipboard</span>
            </label>
          </div>
        </div>
      {/if}
    </div>

    <!-- Actions -->
    <div class="modal-bottom">
      <button
        class="btn btn-secondary"
        onclick={() => (appState.isConnectionModalOpen = false)}
      >
        Cancel
      </button>
      <button class="btn btn-secondary" onclick={() => handleSave(false)}>
        Save
      </button>
      <button class="btn btn-primary" onclick={() => handleSave(true)}>
        Connect
      </button>
    </div>
  </div>
</div>

<style>
  .modal-scrim {
    position: fixed;
    top: 0;
    left: 0;
    width: 100vw;
    height: 100vh;
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal-card {
    width: 540px;
    max-width: 95vw;
    max-height: 90vh;
    background: #000000;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-floating);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .modal-top {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    padding: 18px 20px 14px 20px;
    border-bottom: 1px solid var(--hairline);
  }

  .modal-titles {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .dialog-heading {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
    letter-spacing: -0.4px;
    color: var(--ink);
  }

  .proto-segment {
    display: flex;
    padding: 8px 20px;
    gap: 4px;
    background: var(--canvas-subtle);
    border-bottom: 1px solid var(--hairline);
  }

  .segment-item {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 28px;
    border-radius: var(--radius-sm);
    background: transparent;
    border: 1px solid transparent;
    color: var(--mute);
    font-size: 12px;
    font-weight: 500;
    font-family: var(--font-sans);
    cursor: pointer;
    transition: all 0.12s ease;
  }

  .segment-item:hover {
    color: var(--ink-secondary);
  }

  .segment-active {
    background: var(--canvas-elevated);
    border-color: var(--hairline);
    color: var(--ink);
  }

  .modal-scroll-body {
    padding: 20px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .form-grid {
    display: flex;
    gap: 12px;
  }

  .form-cell {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .col-1 { flex: 1; }
  .col-2 { flex: 2; }

  label {
    font-size: 12px;
    font-weight: 500;
    color: var(--body);
  }

  .section-divider {
    padding-top: 8px;
    margin-bottom: 2px;
    border-bottom: 1px solid var(--hairline-soft);
    padding-bottom: 6px;
  }

  .auth-tabs {
    display: flex;
    gap: 6px;
  }

  .auth-tab {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 10px;
    height: 26px;
    font-size: 11px;
    font-family: var(--font-sans);
    border-radius: var(--radius-sm);
    background: var(--canvas-subtle);
    border: 1px solid var(--hairline);
    color: var(--mute);
    cursor: pointer;
    transition: all 0.12s ease;
  }

  .auth-tab:hover {
    color: var(--ink-secondary);
  }

  .auth-tab-active {
    background: var(--canvas-elevated);
    border-color: var(--hairline-hover);
    color: var(--ink);
  }

  .options-group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .check-row {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    cursor: pointer;
    padding: 6px 8px;
    border-radius: var(--radius-sm);
    border: 1px solid transparent;
  }

  .check-row:hover {
    background: var(--canvas-hover);
    border-color: var(--hairline);
  }

  .check-row input {
    margin-top: 2px;
  }

  .check-text {
    display: flex;
    flex-direction: column;
  }

  .check-title {
    font-size: 12px;
    font-weight: 500;
    color: var(--ink-secondary);
  }

  .check-desc {
    font-size: 11px;
    color: var(--mute);
  }

  .modal-bottom {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding: 14px 20px;
    border-top: 1px solid var(--hairline);
    background: var(--canvas-subtle);
  }
</style>
