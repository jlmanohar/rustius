<script lang="ts">
  import { marked } from 'marked';
  import { invoke } from '@tauri-apps/api/core';
  import { appState } from '../stores/appState.svelte';
  import type { Connection, ConnectionType, TreeNode, SecretRef, CredentialProfile } from '../types';
  import {
    Terminal,
    ArrowLeftRight,
    Monitor,
    X,
    Lock,
    Key,
    KeyRound,
    Shield,
    Layers,
    BookmarkPlus,
    Link2,
    Unlink,
    FileText,
    Code,
    CheckSquare,
    Link as LinkIcon,
    Eye,
    EyeOff,
    Copy,
    Check,
  } from '@lucide/svelte';

  let connType = $state<ConnectionType>('ssh');
  let name = $state('');
  let host = $state('');
  let port = $state(22);
  let environment = $state('Production');
  let description = $state('');
  let tagsStr = $state('');
  let parentFolderId = $state<string>('');

  // Documentation / Notes
  let notes = $state('');
  let notesTab = $state<'write' | 'preview'>('write');

  // Auth
  let authType = $state<'password' | 'keyfile'>('password');
  let username = $state('');
  let password = $state('');
  let keyPath = $state('');
  let passphrase = $state('');
  let existingSecretRef = $state<SecretRef | undefined>(undefined);
  let domain = $state('');
  let selectedCredentialId = $state('');

  // Password visibility & copy state
  let showPassword = $state(false);
  let showPassphrase = $state(false);
  let copiedPassword = $state(false);
  let copiedPassphrase = $state(false);

  async function toggleShowPassword() {
    if (showPassword) {
      showPassword = false;
      return;
    }
    if (!password) {
      if (appState.editingConnection?.id) {
        try {
          const resolved = await invoke<string | null>('get_connection_password', {
            connectionId: appState.editingConnection.id,
          });
          if (resolved) {
            password = resolved;
          }
        } catch (e) {
          console.error('Failed to retrieve connection password:', e);
        }
      } else if (selectedProfile?.id) {
        try {
          const resolved = await invoke<string | null>('get_credential_password', {
            credentialId: selectedProfile.id,
          });
          if (resolved) {
            password = resolved;
          }
        } catch (e) {
          console.error('Failed to retrieve profile password:', e);
        }
      }
    }
    showPassword = true;
  }

  async function toggleShowPassphrase() {
    if (showPassphrase) {
      showPassphrase = false;
      return;
    }
    if (!passphrase && appState.editingConnection?.id) {
      try {
        const resolved = await invoke<string | null>('get_connection_password', {
          connectionId: appState.editingConnection.id,
        });
        if (resolved) {
          passphrase = resolved;
        }
      } catch (e) {
        console.error('Failed to retrieve connection passphrase:', e);
      }
    }
    showPassphrase = true;
  }

  async function copyPasswordToClipboard() {
    let textToCopy = password;
    if (!textToCopy && appState.editingConnection?.id) {
      try {
        const resolved = await invoke<string | null>('get_connection_password', {
          connectionId: appState.editingConnection.id,
        });
        if (resolved) {
          textToCopy = resolved;
          password = resolved;
        }
      } catch (e) {
        console.error('Failed to copy password:', e);
      }
    } else if (!textToCopy && selectedProfile?.id) {
      try {
        const resolved = await invoke<string | null>('get_credential_password', {
          credentialId: selectedProfile.id,
        });
        if (resolved) {
          textToCopy = resolved;
          password = resolved;
        }
      } catch (e) {
        console.error('Failed to copy profile password:', e);
      }
    }
    if (textToCopy) {
      await navigator.clipboard.writeText(textToCopy);
      copiedPassword = true;
      setTimeout(() => {
        copiedPassword = false;
      }, 2000);
    }
  }

  async function copyPassphraseToClipboard() {
    let textToCopy = passphrase;
    if (!textToCopy && appState.editingConnection?.id) {
      try {
        const resolved = await invoke<string | null>('get_connection_password', {
          connectionId: appState.editingConnection.id,
        });
        if (resolved) {
          textToCopy = resolved;
          passphrase = resolved;
        }
      } catch (e) {
        console.error('Failed to copy passphrase:', e);
      }
    }
    if (textToCopy) {
      await navigator.clipboard.writeText(textToCopy);
      copiedPassphrase = true;
      setTimeout(() => {
        copiedPassphrase = false;
      }, 2000);
    }
  }

  let selectedProfile = $derived.by(() => {
    if (!selectedCredentialId) return null;
    return appState.config.credentials?.find((p) => p.id === selectedCredentialId) || null;
  });

  $effect(() => {
    if (appState.editingConnection) {
      const c = appState.editingConnection;
      connType = c.connection_type || c.conn_type || 'ssh';
      name = c.name;
      host = c.host;
      port = c.port;
      environment = c.environment || 'Production';
      description = c.description || '';
      tagsStr = c.tags ? c.tags.join(', ') : '';
      notes = c.notes || '';
      notesTab = 'write';
      selectedCredentialId = c.credential_id || '';
      if (c.auth.type === 'keyfile') {
        authType = 'keyfile';
        username = c.auth.username;
        keyPath = c.auth.key_path;
        passphrase = c.auth.passphrase || '';
        existingSecretRef = c.auth.secret_ref;
      } else if (c.auth.type === 'password') {
        authType = 'password';
        username = c.auth.username;
        password = c.auth.password || '';
        existingSecretRef = c.auth.secret_ref;
      }
      if (c.ssh_config) {
        keepalive = c.ssh_config.keepalive_interval_secs;
        legacyCiphers = c.ssh_config.legacy_ciphers;
        legacyKex = c.ssh_config.legacy_kex;
      }
      if (c.rdp_config) {
        rdpWidth = c.rdp_config.width;
        rdpHeight = c.rdp_config.height;
        clipboardSharing = c.rdp_config.clipboard_sharing;
        domain = c.rdp_config.domain || '';
        rdpResolutionMode = (c.rdp_config.resolution_mode as any) || 'auto';
        rdpScaleFactor = c.rdp_config.scale_factor ?? 0;
        rdpEngine = (c.rdp_config.engine as any) || 'ironrdp';
        rdpEgfx = c.rdp_config.egfx ?? false;
      } else {
        domain = '';
        rdpResolutionMode = 'auto';
        rdpScaleFactor = 0;
        rdpEngine = 'ironrdp';
        rdpEgfx = false;
      }
      if (connType === 'rdp' && !domain && username.includes('\\')) {
        const parts = username.split('\\');
        if (parts.length === 2 && parts[0]) {
          domain = parts[0];
          username = parts[1];
        }
      }
    } else {
      existingSecretRef = undefined;
      domain = '';
      selectedCredentialId = '';
      description = '';
      notes = '';
      notesTab = 'write';
      rdpResolutionMode = 'auto';
      rdpScaleFactor = 0;
      rdpEngine = 'ironrdp';
      rdpEgfx = false;
    }
  });

  // SSH advanced
  let legacyCiphers = $state(false);
  let legacyKex = $state(false);
  let keepalive = $state(30);

  // RDP options
  let rdpEngine = $state<'ironrdp' | 'freerdp'>('ironrdp');
  let rdpEgfx = $state(false);
  let rdpResolutionMode = $state<'auto' | 'native' | '1080p' | '720p' | 'custom'>('auto');
  let rdpScaleFactor = $state<number>(0);
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
    if (type === 'rdp') {
      authType = 'password';
    }
  }

  function handleProfileChange(id: string) {
    selectedCredentialId = id;
    if (id) {
      const p = appState.config.credentials?.find((x) => x.id === id);
      if (p) {
        if ('username' in p.auth) {
          username = p.auth.username;
        }
        if (p.domain) domain = p.domain;
        if (p.auth.type === 'keyfile') {
          authType = 'keyfile';
          if ('key_path' in p.auth) keyPath = p.auth.key_path;
        } else {
          authType = 'password';
        }
      }
    }
  }

  async function handleSaveAsProfile() {
    if (!username.trim()) {
      await appState.alert({
        title: 'Missing Username',
        message: 'Username is required to save credentials.',
      });
      return;
    }
    const profileName = await appState.prompt({
      title: 'Save Credential Profile',
      placeholder: 'e.g. Production AD Credentials',
      initialValue: `${name || 'Server'} Credentials`,
      confirmText: 'Save Credential',
    });
    if (!profileName || !profileName.trim()) return;

    try {
      const now = new Date().toISOString();
      const auth =
        authType === 'password'
          ? {
              type: 'password' as const,
              username: username.trim(),
              password: password ? password : undefined,
            }
          : {
              type: 'keyfile' as const,
              username: username.trim(),
              key_path: keyPath.trim(),
              passphrase: passphrase ? passphrase : undefined,
            };

      const newProf: CredentialProfile = {
        id: typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'cred-' + Date.now(),
        name: profileName.trim(),
        domain: domain.trim() || undefined,
        auth,
        created_at: now,
        updated_at: now,
      };
      await appState.addCredential(newProf);
      selectedCredentialId = newProf.id;
    } catch (err: any) {
      console.error('Failed to save credential:', err);
      await appState.alert({
        title: 'Failed to Save Credential',
        message: err?.message || String(err),
      });
    }
  }

  function renderMarkdown(md: string): string {
    if (!md || !md.trim()) return '';
    try {
      return marked.parse(md, { gfm: true, breaks: true }) as string;
    } catch {
      return md;
    }
  }

  function insertMarkdown(prefix: string, suffix: string) {
    const el = document.getElementById('conn-notes-input') as HTMLTextAreaElement | null;
    if (!el) {
      notes += `${prefix}${suffix}`;
      return;
    }
    const start = el.selectionStart;
    const end = el.selectionEnd;
    const selected = notes.substring(start, end);
    const replacement = `${prefix}${selected || 'text'}${suffix}`;
    notes = notes.substring(0, start) + replacement + notes.substring(end);
    setTimeout(() => {
      el.focus();
      el.setSelectionRange(start + prefix.length, start + prefix.length + (selected ? selected.length : 4));
    }, 0);
  }

  async function handleSave(connectNow: boolean = false) {
    if (!name.trim() || !host.trim()) {
      await appState.alert({
        title: 'Missing Required Fields',
        message: 'Session Name and Host address are required.',
      });
      return;
    }

    const tags = tagsStr
      .split(',')
      .map((t) => t.trim())
      .filter((t) => t.length > 0);

    const editing = appState.editingConnection;
    const id = editing ? editing.id : (typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'conn-' + Date.now());
    const createdAt = editing ? editing.created_at : new Date().toISOString();

    let finalUsername = username.trim();
    let finalDomain = domain.trim();

    if (connType === 'rdp') {
      if (!finalDomain && finalUsername.includes('\\')) {
        const parts = finalUsername.split('\\');
        if (parts.length === 2 && parts[0]) {
          finalDomain = parts[0].trim();
          finalUsername = parts[1].trim();
        }
      } else if (finalDomain && finalUsername.includes('\\')) {
        const parts = finalUsername.split('\\');
        if (parts.length === 2) {
          finalUsername = parts[1].trim();
        }
      }
    }

    const auth =
      authType === 'password'
        ? {
            type: 'password' as const,
            username: finalUsername,
            password: password ? password : (existingSecretRef ? undefined : undefined),
            secret_ref: password ? undefined : existingSecretRef,
          }
        : {
            type: 'keyfile' as const,
            username: finalUsername,
            key_path: keyPath.trim(),
            passphrase: passphrase ? passphrase : (existingSecretRef ? undefined : undefined),
            secret_ref: passphrase ? undefined : existingSecretRef,
          };

    const newConnection: Connection = {
      id,
      name: name.trim(),
      conn_type: connType,
      connection_type: connType,
      credential_id: selectedCredentialId ? selectedCredentialId : undefined,
      host: host.trim(),
      port: Number(port),
      environment: environment.trim() || undefined,
      description: description.trim() || undefined,
      tags,
      notes: notes.trim() || undefined,
      created_at: createdAt,
      updated_at: new Date().toISOString(),
      auth,
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
              domain: finalDomain || undefined,
              resolution_mode: rdpResolutionMode,
              scale_factor: rdpScaleFactor > 0 ? rdpScaleFactor : undefined,
              engine: rdpEngine,
              egfx: rdpEngine === 'ironrdp' ? rdpEgfx : undefined,
            }
          : undefined,
    };


    try {
      await appState.addConnection(newConnection, parentFolderId || null);

      if (connectNow) {
        const title = (newConnection.name && newConnection.name.trim()) ? newConnection.name.trim() : newConnection.host;
        appState.openTab({
          title,
          type: newConnection.connection_type,
          connectionId: newConnection.id,
          status: 'connecting',
          badge: newConnection.host,
        });
      }

      appState.editingConnection = null;
      appState.isConnectionModalOpen = false;
    } catch (err: any) {
      console.error('Failed to save connection:', err);
      await appState.alert({
        title: 'Failed to Save Connection',
        message: err?.message || String(err),
      });
    }
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
        <span class="mono-eyebrow">{appState.editingConnection ? 'EDIT CONNECTION' : 'CONFIGURATION'}</span>
        <h2 class="dialog-heading">{appState.editingConnection ? 'Edit Connection' : 'New Connection'}</h2>
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

      <div class="form-cell">
        <label for="conn-desc">Short Description (optional)</label>
        <input
          id="conn-desc"
          type="text"
          placeholder="e.g. Primary DB replica, US-East customer cluster"
          bind:value={description}
          class="input-field"
        />
      </div>

      <!-- Authentication -->
      <div class="section-divider">
        <span class="mono-eyebrow">AUTHENTICATION</span>
        <span class="vault-badge" title="Credentials secured in native OS Keyring (Windows Credential Manager / macOS Keychain)">
          <Shield size={11} />
          <span>OS Keyring Secured</span>
        </span>
      </div>

      <!-- Saved Credentials Selector -->
      <div class="profile-select-container">
        <div class="profile-select-header">
          <label for="cred-profile-select" class="profile-label">
            <KeyRound size={12} />
            <span>Saved Credentials</span>
          </label>
          {#if !selectedCredentialId && username.trim()}
            <button
              type="button"
              class="profile-action-btn"
              onclick={handleSaveAsProfile}
              title="Save current credentials for reuse"
            >
              <BookmarkPlus size={12} />
              <span>Save as Credential</span>
            </button>
          {:else if selectedCredentialId}
            <button
              type="button"
              class="profile-action-btn detach"
              onclick={() => (selectedCredentialId = '')}
              title="Detach credential to use custom values"
            >
              <Unlink size={12} />
              <span>Detach</span>
            </button>
          {/if}
        </div>
        <select
          id="cred-profile-select"
          class="input-field"
          value={selectedCredentialId}
          onchange={(e) => handleProfileChange((e.target as HTMLSelectElement).value)}
        >
          <option value="">Direct / Custom Credentials (No Saved Credential)</option>
          {#if appState.config.credentials && appState.config.credentials.length > 0}
            <optgroup label="Saved Credentials">
              {#each appState.config.credentials as profile (profile.id)}
                <option value={profile.id}>
                  {profile.name} ({profile.auth.username}{profile.domain ? ` @ ${profile.domain}` : ''} · {profile.auth.type})
                </option>
              {/each}
            </optgroup>
          {/if}
        </select>

        {#if selectedProfile}
          <div class="profile-linked-callout">
            <Link2 size={13} />
            <span>
              Inheriting from <strong>{selectedProfile.name}</strong>. Secret is secured in OS Keyring. Changes in Saved Credentials propagate automatically.
            </span>
          </div>
        {/if}
      </div>

      {#if connType !== 'rdp'}
        <div class="auth-tabs">
          <button
            class="auth-tab {authType === 'password' ? 'auth-tab-active' : ''}"
            onclick={() => (authType = 'password')}
            disabled={!!selectedProfile}
          >
            <Lock size={12} />
            <span>Password</span>
          </button>
          <button
            class="auth-tab {authType === 'keyfile' ? 'auth-tab-active' : ''}"
            onclick={() => (authType = 'keyfile')}
            disabled={!!selectedProfile}
          >
            <Key size={12} />
            <span>Private Key</span>
          </button>
        </div>
      {/if}

      <div class="form-grid">
        {#if connType === 'rdp'}
          <div class="form-cell col-1">
            <label for="conn-domain">Domain (optional)</label>
            <input
              id="conn-domain"
              type="text"
              placeholder={selectedProfile?.domain ? `Inherited: ${selectedProfile.domain}` : 'e.g. CORP or WORKGROUP'}
              bind:value={domain}
              disabled={!!selectedProfile}
              class="input-field"
            />
          </div>
        {/if}
        <div class="form-cell col-1">
          <label for="conn-user">Username</label>
          <input
            id="conn-user"
            type="text"
            placeholder={selectedProfile ? `Inherited: ${selectedProfile.auth.username}` : (connType === 'rdp' ? 'e.g. Administrator' : 'e.g. root')}
            bind:value={username}
            disabled={!!selectedProfile}
            class="input-field"
          />
        </div>

        {#if authType === 'password'}
          <div class="form-cell {connType === 'rdp' ? 'col-1' : 'col-2'}">
            <div class="label-row">
              <label for="conn-pass">Password</label>
              {#if password || existingSecretRef?.type === 'keyring' || (selectedProfile && selectedProfile.auth.type === 'password')}
                <button
                  type="button"
                  class="field-copy-btn"
                  title="Copy password to clipboard"
                  onclick={copyPasswordToClipboard}
                >
                  {#if copiedPassword}
                    <Check size={11} />
                    <span>Copied</span>
                  {:else}
                    <Copy size={11} />
                    <span>Copy</span>
                  {/if}
                </button>
              {/if}
            </div>
            <div class="password-input-wrapper">
              <input
                id="conn-pass"
                type={showPassword ? 'text' : 'password'}
                placeholder={selectedProfile ? 'Inherited from Profile (OS Keyring)' : (existingSecretRef?.type === 'keyring' ? 'Stored in OS Keyring (leave empty to keep)' : '••••••••')}
                bind:value={password}
                disabled={!!selectedProfile}
                class="input-field has-toggle"
              />
              <button
                type="button"
                class="password-toggle-btn"
                title={showPassword ? 'Hide password' : 'View saved password'}
                onclick={toggleShowPassword}
              >
                {#if showPassword}
                  <EyeOff size={14} />
                {:else}
                  <Eye size={14} />
                {/if}
              </button>
            </div>
          </div>
        {:else}
          <div class="form-cell col-2">
            <label for="conn-key">Key Path</label>
            <input
              id="conn-key"
              type="text"
              placeholder={selectedProfile?.auth.type === 'keyfile' ? `Inherited: ${selectedProfile.auth.key_path}` : '~/.ssh/id_ed25519'}
              bind:value={keyPath}
              disabled={!!selectedProfile}
              class="input-field"
            />
          </div>
        {/if}
      </div>

      {#if authType === 'keyfile'}
        <div class="form-cell">
          <div class="label-row">
            <label for="conn-passphrase">Passphrase (optional)</label>
            {#if passphrase || existingSecretRef?.type === 'keyring'}
              <button
                type="button"
                class="field-copy-btn"
                title="Copy passphrase to clipboard"
                onclick={copyPassphraseToClipboard}
              >
                {#if copiedPassphrase}
                  <Check size={11} />
                  <span>Copied</span>
                {:else}
                  <Copy size={11} />
                  <span>Copy</span>
                {/if}
              </button>
            {/if}
          </div>
          <div class="password-input-wrapper">
            <input
              id="conn-passphrase"
              type={showPassphrase ? 'text' : 'password'}
              placeholder={selectedProfile ? 'Inherited from Profile (OS Keyring)' : (existingSecretRef?.type === 'keyring' ? 'Stored in OS Keyring (leave empty to keep)' : 'Key passphrase')}
              bind:value={passphrase}
              disabled={!!selectedProfile}
              class="input-field has-toggle"
            />
            <button
              type="button"
              class="password-toggle-btn"
              title={showPassphrase ? 'Hide passphrase' : 'View saved passphrase'}
              onclick={toggleShowPassphrase}
            >
              {#if showPassphrase}
                <EyeOff size={14} />
              {:else}
                <Eye size={14} />
              {/if}
            </button>
          </div>
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
          <div class="form-cell col-2">
            <label for="rdp-engine">RDP Engine</label>
            <select id="rdp-engine" bind:value={rdpEngine} class="input-field">
              <option value="ironrdp">IronRDP (Cross-Platform · Canvas/GPU Render · Pure Rust)</option>
              <option value="freerdp">FreeRDP (Direct3D Native Windows Overlay)</option>
            </select>
          </div>

          <div class="form-cell col-2">
            <label for="rdp-resolution-mode">Display Resolution</label>
            <select id="rdp-resolution-mode" bind:value={rdpResolutionMode} class="input-field">

              <option value="auto">Auto (Fit Window · Larger UI)</option>
              <option value="native">Native Monitor (1440p / 4K / High Res)</option>
              <option value="1080p">1080p (Full HD · 1920×1080)</option>
              <option value="720p">720p (HD · 1280×720)</option>
              <option value="custom">Custom (Width × Height)</option>
            </select>
          </div>

          <div class="form-cell col-1">
            <label for="rdp-scale">Desktop Scaling (DPI)</label>
            <select id="rdp-scale" bind:value={rdpScaleFactor} class="input-field">
              <option value={0}>Auto (Recommended)</option>
              <option value={100}>100% (Standard)</option>
              <option value={125}>125%</option>
              <option value={150}>150% (Recommended for 1440p)</option>
              <option value={175}>175%</option>
              <option value={200}>200% (Recommended for 4K)</option>
            </select>
          </div>

          {#if rdpResolutionMode === 'custom'}
            <div class="form-cell col-1">
              <label for="rdp-w">Custom Width</label>
              <input id="rdp-w" type="number" bind:value={rdpWidth} class="input-field" />
            </div>
            <div class="form-cell col-1">
              <label for="rdp-h">Custom Height</label>
              <input id="rdp-h" type="number" bind:value={rdpHeight} class="input-field" />
            </div>
          {/if}

          <div class="form-cell col-1">
            <label class="check-row" style="margin-top: 24px;">
              <input type="checkbox" bind:checked={clipboardSharing} />
              <span class="check-title">Clipboard Sharing</span>
            </label>
          </div>

          {#if rdpEngine === 'ironrdp'}
            <div class="form-cell col-2">
              <label class="check-row" title="Enable Microsoft Remote Desktop Graphics Pipeline (MS-RDPEGFX). Keep disabled (recommended) for maximum stability.">
                <input type="checkbox" bind:checked={rdpEgfx} />
                <span class="check-title">Enable EGFX Graphics Pipeline (Experimental)</span>
              </label>
            </div>
          {/if}
        </div>
      {/if}

      <!-- Documentation & Notes -->
      <div class="section-divider">
        <span class="mono-eyebrow">NOTES</span>
        <span class="vault-badge" title="Markdown formatting supported">
          <FileText size={11} />
          <span>Markdown Supported</span>
        </span>
      </div>

      <div class="notes-container">
        <div class="notes-header">
          <div class="notes-tabs">
            <button
              type="button"
              class="notes-tab {notesTab === 'write' ? 'active' : ''}"
              onclick={() => (notesTab = 'write')}
            >
              Write
            </button>
            <button
              type="button"
              class="notes-tab {notesTab === 'preview' ? 'active' : ''}"
              onclick={() => (notesTab = 'preview')}
            >
              Preview
            </button>
          </div>

          {#if notesTab === 'write'}
            <div class="notes-toolbar">
              <button
                type="button"
                class="tb-btn"
                title="Bold (**text**)"
                onclick={() => insertMarkdown('**', '**')}
              >
                <strong>B</strong>
              </button>
              <button
                type="button"
                class="tb-btn"
                title="Code block (```bash ... ```)"
                onclick={() => insertMarkdown('```bash\n', '\n```')}
              >
                <Code size={12} />
              </button>
              <button
                type="button"
                class="tb-btn"
                title="Checklist item (- [ ])"
                onclick={() => insertMarkdown('- [ ] ', '')}
              >
                <CheckSquare size={12} />
              </button>
              <button
                type="button"
                class="tb-btn"
                title="Link ([title](url))"
                onclick={() => insertMarkdown('[', '](https://)')}
              >
                <LinkIcon size={12} />
              </button>
            </div>
          {/if}
        </div>

        {#if notesTab === 'write'}
          <textarea
            id="conn-notes-input"
            class="notes-textarea input-field"
            placeholder="# Server Notes&#10;&#10;- Service restart: `sudo systemctl restart service`&#10;- On-call escalation: #devops-alert&#10;- [ ] Checklist tasks..."
            bind:value={notes}
            rows="6"
          ></textarea>
        {:else}
          <div class="notes-preview markdown-body">
            {#if notes.trim()}
              {@html renderMarkdown(notes)}
            {:else}
              <p class="preview-empty">No notes entered yet. Switch to "Write" to add notes, checklists, or commands.</p>
            {/if}
          </div>
        {/if}
      </div>
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
    max-height: min(88vh, calc(100vh - 40px));
    background: #000000;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-floating);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .modal-top {
    flex-shrink: 0;
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    padding: clamp(12px, 1.8vh, 18px) 20px clamp(10px, 1.4vh, 14px) 20px;
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
    flex-shrink: 0;
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
    flex: 1;
    min-height: 0;
    padding: clamp(12px, 2vh, 20px);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: clamp(8px, 1.4vh, 12px);
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
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .vault-badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 10px;
    font-family: var(--font-mono);
    color: var(--body);
    background: var(--canvas-subtle);
    border: 1px solid var(--hairline);
    padding: 2px 8px;
    border-radius: var(--radius-pill);
    letter-spacing: 0.02em;
    transition: all 0.12s ease;
  }

  .vault-badge :global(svg) {
    color: var(--mute);
  }

  .vault-badge:hover {
    border-color: var(--hairline-hover);
    color: var(--ink-secondary);
  }

  .vault-badge:hover :global(svg) {
    color: var(--body);
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

  .profile-select-container {
    display: flex;
    flex-direction: column;
    gap: 6px;
    background: var(--canvas-subtle);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-sm);
    padding: 10px;
  }

  .profile-select-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .profile-label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    font-weight: 500;
    color: var(--ink-secondary);
  }

  .profile-label :global(svg) {
    color: var(--mute);
  }

  .profile-action-btn {
    display: flex;
    align-items: center;
    gap: 4px;
    background: transparent;
    border: 1px solid var(--hairline);
    color: var(--body);
    font-size: 11px;
    padding: 2px 7px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: all 0.12s ease;
  }

  .profile-action-btn:hover {
    border-color: var(--hairline-hover);
    color: var(--ink);
    background: var(--canvas-hover);
  }

  .profile-action-btn.detach {
    color: var(--mute);
  }

  .profile-action-btn.detach:hover {
    color: #ef4444;
    border-color: rgba(239, 68, 68, 0.3);
  }

  .profile-linked-callout {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: var(--radius-xs);
    background: rgba(59, 130, 246, 0.08);
    border: 1px solid rgba(59, 130, 246, 0.2);
    font-size: 11px;
    color: var(--ink-secondary);
    line-height: 1.4;
  }

  .profile-linked-callout strong {
    color: var(--ink);
  }

  .profile-linked-callout :global(svg) {
    color: #3b82f6;
    flex-shrink: 0;
  }

  input:disabled {
    opacity: 0.65;
    cursor: not-allowed;
    background: var(--canvas-subtle);
    border-color: var(--hairline-soft);
  }

  /* Notes & Markdown Styles */
  .notes-container {
    display: flex;
    flex-direction: column;
    gap: 8px;
    background: var(--canvas-subtle);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-sm);
    padding: 10px;
  }

  .notes-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .notes-tabs {
    display: flex;
    gap: 4px;
    background: var(--canvas);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-xs);
    padding: 2px;
  }

  .notes-tab {
    background: transparent;
    border: none;
    font-size: 11px;
    color: var(--mute);
    padding: 3px 10px;
    border-radius: var(--radius-xs);
    cursor: pointer;
    transition: all 0.12s ease;
  }

  .notes-tab.active {
    background: var(--canvas-elevated);
    color: var(--ink);
    font-weight: 500;
  }

  .notes-toolbar {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .tb-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: var(--radius-xs);
    background: transparent;
    border: 1px solid var(--hairline);
    color: var(--body);
    font-size: 11px;
    cursor: pointer;
    transition: all 0.12s ease;
  }

  .tb-btn:hover {
    color: var(--ink);
    border-color: var(--hairline-hover);
    background: var(--canvas-hover);
  }

  .notes-textarea {
    resize: vertical;
    min-height: 110px;
    font-family: var(--font-mono, monospace);
    font-size: 12px;
    line-height: 1.5;
    padding: 8px 10px;
  }

  .notes-preview {
    min-height: 110px;
    max-height: 260px;
    overflow-y: auto;
    background: var(--canvas);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-xs);
    padding: 10px 14px;
    font-size: 12px;
    line-height: 1.6;
    color: var(--ink-secondary);
  }

  .preview-empty {
    color: var(--mute);
    font-style: italic;
    font-size: 11px;
    margin: 8px 0;
  }

  .notes-preview :global(h1),
  .notes-preview :global(h2),
  .notes-preview :global(h3) {
    color: var(--ink);
    font-size: 13px;
    font-weight: 600;
    margin-top: 10px;
    margin-bottom: 6px;
  }

  .notes-preview :global(p) {
    margin-bottom: 6px;
  }

  .notes-preview :global(ul),
  .notes-preview :global(ol) {
    margin: 4px 0 8px 18px;
  }

  .notes-preview :global(li) {
    margin-bottom: 2px;
  }

  .notes-preview :global(code) {
    font-family: var(--font-mono, monospace);
    background: var(--canvas-subtle);
    border: 1px solid var(--hairline);
    padding: 1px 4px;
    border-radius: 3px;
    font-size: 11px;
    color: var(--ink);
  }

  .notes-preview :global(pre) {
    background: var(--canvas-subtle);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-xs);
    padding: 8px 10px;
    overflow-x: auto;
    margin: 6px 0;
  }

  .notes-preview :global(pre code) {
    border: none;
    padding: 0;
    background: transparent;
  }

  .notes-preview :global(a) {
    color: #3b82f6;
    text-decoration: underline;
  }

  .modal-bottom {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding: clamp(10px, 1.4vh, 14px) 20px;
    border-top: 1px solid var(--hairline);
    background: var(--canvas-subtle);
  }

  .label-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 4px;
  }

  .label-row label {
    margin-bottom: 0;
  }

  .field-copy-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: transparent;
    border: none;
    color: var(--mute);
    font-size: 11px;
    cursor: pointer;
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    transition: all 0.12s ease;
  }

  .field-copy-btn:hover {
    color: var(--ink);
    background: var(--surface-hover);
  }

  .password-input-wrapper {
    position: relative;
    display: flex;
    align-items: center;
    width: 100%;
  }

  .password-input-wrapper input.has-toggle {
    padding-right: 32px;
  }

  .password-toggle-btn {
    position: absolute;
    right: 6px;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background: transparent;
    border: none;
    color: var(--mute);
    cursor: pointer;
    border-radius: var(--radius-sm);
    transition: all 0.12s ease;
  }

  .password-toggle-btn:hover {
    color: var(--ink);
    background: rgba(255, 255, 255, 0.08);
  }
</style>
