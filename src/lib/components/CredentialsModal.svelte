<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { appState } from '../stores/appState.svelte';
  import type { CredentialProfile, TreeNode } from '../types';
  import {
    X,
    Plus,
    Key,
    Lock,
    Shield,
    Trash2,
    Edit2,
    Monitor,
    Search,
    Check,
    Layers,
    Eye,
    EyeOff,
    Copy,
  } from '@lucide/svelte';

  // State
  let searchQuery = $state('');
  let isEditing = $state(false);
  let editingId = $state<string | null>(null);

  // Form fields
  let formName = $state('');
  let formType = $state<'domain' | 'password' | 'keyfile'>('domain');
  let formDomain = $state('');
  let formUsername = $state('');
  let formPassword = $state('');
  let formKeyPath = $state('');
  let formPassphrase = $state('');
  let existingSecretRef = $state<any>(undefined);

  // Password visibility & copying
  let revealedPasswords = $state<Record<string, string>>({});
  let copiedId = $state<string | null>(null);
  let showFormPassword = $state(false);
  let showFormPassphrase = $state(false);
  let copiedFormPassword = $state(false);
  let copiedFormPassphrase = $state(false);

  async function toggleRevealProfilePassword(cred: CredentialProfile) {
    if (revealedPasswords[cred.id]) {
      const copy = { ...revealedPasswords };
      delete copy[cred.id];
      revealedPasswords = copy;
      return;
    }

    try {
      const resolved = await invoke<string | null>('get_credential_password', {
        credentialId: cred.id,
      });
      if (resolved) {
        revealedPasswords = { ...revealedPasswords, [cred.id]: resolved };
      } else {
        revealedPasswords = { ...revealedPasswords, [cred.id]: '(No password saved)' };
      }
    } catch (e) {
      console.error('Failed to get credential password:', e);
    }
  }

  async function copyProfilePassword(e: MouseEvent, cred: CredentialProfile) {
    e.stopPropagation();
    let val = revealedPasswords[cred.id];
    if (!val) {
      try {
        const resolved = await invoke<string | null>('get_credential_password', {
          credentialId: cred.id,
        });
        if (resolved) {
          val = resolved;
          revealedPasswords = { ...revealedPasswords, [cred.id]: resolved };
        }
      } catch (err) {
        console.error('Failed to fetch password for copy:', err);
      }
    }
    if (val && val !== '(No password saved)') {
      await navigator.clipboard.writeText(val);
      copiedId = cred.id;
      setTimeout(() => {
        copiedId = null;
      }, 2000);
    }
  }

  async function toggleFormPassword() {
    if (showFormPassword) {
      showFormPassword = false;
      return;
    }
    if (!formPassword && editingId) {
      try {
        const resolved = await invoke<string | null>('get_credential_password', {
          credentialId: editingId,
        });
        if (resolved) {
          formPassword = resolved;
        }
      } catch (e) {
        console.error('Failed to retrieve credential password:', e);
      }
    }
    showFormPassword = true;
  }

  async function toggleFormPassphrase() {
    if (showFormPassphrase) {
      showFormPassphrase = false;
      return;
    }
    if (!formPassphrase && editingId) {
      try {
        const resolved = await invoke<string | null>('get_credential_password', {
          credentialId: editingId,
        });
        if (resolved) {
          formPassphrase = resolved;
        }
      } catch (e) {
        console.error('Failed to retrieve credential passphrase:', e);
      }
    }
    showFormPassphrase = true;
  }

  async function copyFormPassword() {
    let text = formPassword;
    if (!text && editingId) {
      try {
        const resolved = await invoke<string | null>('get_credential_password', {
          credentialId: editingId,
        });
        if (resolved) {
          text = resolved;
          formPassword = resolved;
        }
      } catch (e) {
        console.error('Failed to copy form password:', e);
      }
    }
    if (text) {
      await navigator.clipboard.writeText(text);
      copiedFormPassword = true;
      setTimeout(() => {
        copiedFormPassword = false;
      }, 2000);
    }
  }

  async function copyFormPassphrase() {
    let text = formPassphrase;
    if (!text && editingId) {
      try {
        const resolved = await invoke<string | null>('get_credential_password', {
          credentialId: editingId,
        });
        if (resolved) {
          text = resolved;
          formPassphrase = resolved;
        }
      } catch (e) {
        console.error('Failed to copy form passphrase:', e);
      }
    }
    if (text) {
      await navigator.clipboard.writeText(text);
      copiedFormPassphrase = true;
      setTimeout(() => {
        copiedFormPassphrase = false;
      }, 2000);
    }
  }

  function countLinkedConnections(id: string): number {
    let count = 0;
    const traverse = (nodes: TreeNode[]) => {
      for (const node of nodes) {
        if (node.node_type === 'connection') {
          if (node.credential_id === id) count++;
        } else if (node.node_type === 'folder') {
          traverse(node.children);
        }
      }
    };
    traverse(appState.config.tree);
    return count;
  }

  const filteredProfiles = $derived.by(() => {
    const list = appState.config.credentials || [];
    if (!searchQuery.trim()) return list;
    const q = searchQuery.toLowerCase();
    return list.filter(
      (c) =>
        c.name.toLowerCase().includes(q) ||
        (c.domain && c.domain.toLowerCase().includes(q)) ||
        ('username' in c.auth && c.auth.username.toLowerCase().includes(q))
    );
  });

  function startCreate() {
    isEditing = true;
    editingId = null;
    formName = '';
    formType = 'domain';
    formDomain = '';
    formUsername = '';
    formPassword = '';
    formKeyPath = '';
    formPassphrase = '';
    existingSecretRef = undefined;
    showFormPassword = false;
    showFormPassphrase = false;
  }

  function startEdit(cred: CredentialProfile) {
    isEditing = true;
    editingId = cred.id;
    formName = cred.name;
    formDomain = cred.domain || '';
    existingSecretRef = 'secret_ref' in cred.auth ? cred.auth.secret_ref : undefined;
    showFormPassword = false;
    showFormPassphrase = false;

    if (cred.auth.type === 'keyfile') {
      formType = 'keyfile';
      formUsername = cred.auth.username;
      formKeyPath = cred.auth.key_path;
      formPassphrase = cred.auth.passphrase || '';
      formPassword = '';
    } else if (cred.auth.type === 'password') {
      formUsername = cred.auth.username;
      formPassword = cred.auth.password || '';
      formType = cred.domain ? 'domain' : 'password';
      formKeyPath = '';
      formPassphrase = '';
    }
  }

  function cancelEdit() {
    isEditing = false;
    editingId = null;
    showFormPassword = false;
    showFormPassphrase = false;
  }

  async function handleSave() {
    if (!formName.trim()) {
      await appState.alert({
        title: 'Missing Required Field',
        message: 'Profile Name is required.',
      });
      return;
    }
    if (!formUsername.trim()) {
      await appState.alert({
        title: 'Missing Required Field',
        message: 'Username is required.',
      });
      return;
    }

    const id = editingId || (typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : 'cred-' + Date.now());
    const now = new Date().toISOString();

    let auth: any;
    if (formType === 'keyfile') {
      auth = {
        type: 'keyfile' as const,
        username: formUsername.trim(),
        key_path: formKeyPath.trim(),
        passphrase: formPassphrase ? formPassphrase : (existingSecretRef ? undefined : undefined),
        secret_ref: formPassphrase ? undefined : existingSecretRef,
      };
    } else {
      auth = {
        type: 'password' as const,
        username: formUsername.trim(),
        password: formPassword ? formPassword : (existingSecretRef ? undefined : undefined),
        secret_ref: formPassword ? undefined : existingSecretRef,
      };
    }

    const profile: CredentialProfile = {
      id,
      name: formName.trim(),
      domain: formType === 'domain' && formDomain.trim() ? formDomain.trim() : undefined,
      auth,
      created_at: editingId
        ? (appState.config.credentials?.find((c) => c.id === editingId)?.created_at || now)
        : now,
      updated_at: now,
    };

    if (editingId) {
      await appState.updateCredential(profile);
    } else {
      await appState.addCredential(profile);
    }

    isEditing = false;
    editingId = null;
  }

  async function handleDelete(cred: CredentialProfile) {
    const linked = countLinkedConnections(cred.id);
    const msg = linked > 0
      ? `Delete saved credential "${cred.name}"? ${linked} connected server(s) will be detached.`
      : `Are you sure you want to delete saved credential "${cred.name}"?`;

    const ok = await appState.confirm({
      title: 'Delete Saved Credential',
      message: msg,
      confirmText: 'Delete Credential',
      isDanger: true,
    });

    if (ok) {
      await appState.deleteCredential(cred.id);
    }
  }
</script>

<div
  class="modal-scrim"
  role="presentation"
  onclick={() => (appState.isCredentialsModalOpen = false)}
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
        <span class="mono-eyebrow">VAULT</span>
        <h2 class="dialog-heading">Saved Credentials</h2>
      </div>
      <button
        class="icon-btn-close"
        title="Close"
        onclick={() => (appState.isCredentialsModalOpen = false)}
      >
        <X size={15} />
      </button>
    </div>

    <!-- Modal Content -->
    {#if !isEditing}
      <!-- Profiles List View -->
      <div class="list-controls">
        <div class="search-box">
          <Search size={13} class="search-icon" />
          <input
            type="text"
            placeholder="Search saved credentials by name, user, or domain..."
            bind:value={searchQuery}
            class="search-input"
          />
        </div>
        <button class="btn btn-primary new-btn" onclick={startCreate}>
          <Plus size={13} />
          <span>New Credential</span>
        </button>
      </div>

      <div class="profiles-list">
        {#if filteredProfiles.length === 0}
          <div class="empty-state">
            <Key size={32} class="empty-icon" />
            <p class="empty-title">No Saved Credentials Yet</p>
            <p class="empty-desc">
              Save reusable Windows Active Directory accounts, SSH keys, or Linux passwords to attach them across multiple servers at once.
            </p>
            <button class="btn btn-secondary empty-btn" onclick={startCreate}>
              <Plus size={13} />
              <span>Add Saved Credential</span>
            </button>
          </div>
        {:else}
          {#each filteredProfiles as cred}
            {@const linkedCount = countLinkedConnections(cred.id)}
            <div class="profile-card">
              <div class="profile-icon-col">
                {#if cred.domain}
                  <div class="type-icon-box domain" title="Windows Domain Account">
                    <Monitor size={16} />
                  </div>
                {:else if cred.auth.type === 'keyfile'}
                  <div class="type-icon-box key" title="SSH Key Pair">
                    <Key size={16} />
                  </div>
                {:else}
                  <div class="type-icon-box pass" title="Password Account">
                    <Lock size={16} />
                  </div>
                {/if}
              </div>

              <div class="profile-info-col">
                <div class="profile-header-row">
                  <span class="profile-name">{cred.name}</span>
                  {#if cred.domain}
                    <span class="badge domain-badge">Domain: {cred.domain}</span>
                  {:else if cred.auth.type === 'keyfile'}
                    <span class="badge key-badge">SSH Key</span>
                  {:else}
                    <span class="badge pass-badge">Password</span>
                  {/if}
                </div>

                <div class="profile-meta-row">
                  <span class="user-pill">
                    {cred.domain ? `${cred.domain}\\` : ''}{'username' in cred.auth ? cred.auth.username : 'agent'}
                  </span>
                  {#if cred.auth.type === 'keyfile'}
                    <span class="key-path-pill" title={cred.auth.key_path}>
                      {cred.auth.key_path}
                    </span>
                  {/if}
                  <span class="linked-pill" title="Servers inheriting these credentials">
                    <Layers size={11} />
                    <span>{linkedCount} {linkedCount === 1 ? 'server' : 'servers'} linked</span>
                  </span>
                  {#if revealedPasswords[cred.id]}
                    <div class="revealed-secret-pill" title="Saved secret value">
                      <span class="secret-label">Secret:</span>
                      <span class="secret-value">{revealedPasswords[cred.id]}</span>
                      <button
                        type="button"
                        class="copy-secret-btn"
                        title="Copy to clipboard"
                        onclick={(e) => copyProfilePassword(e, cred)}
                      >
                        {#if copiedId === cred.id}
                          <Check size={11} />
                        {:else}
                          <Copy size={11} />
                        {/if}
                      </button>
                    </div>
                  {/if}
                </div>
              </div>

              <div class="profile-actions-col">
                <button
                  class="action-btn"
                  title={revealedPasswords[cred.id] ? 'Hide Secret' : 'View Saved Password'}
                  onclick={() => toggleRevealProfilePassword(cred)}
                >
                  {#if revealedPasswords[cred.id]}
                    <EyeOff size={13} />
                  {:else}
                    <Eye size={13} />
                  {/if}
                </button>
                <button
                  class="action-btn"
                  title="Copy Password"
                  onclick={(e) => copyProfilePassword(e, cred)}
                >
                  {#if copiedId === cred.id}
                    <Check size={13} />
                  {:else}
                    <Copy size={13} />
                  {/if}
                </button>
                <button
                  class="action-btn"
                  title="Edit Profile"
                  onclick={() => startEdit(cred)}
                >
                  <Edit2 size={13} />
                </button>
                <button
                  class="action-btn danger"
                  title="Delete Profile"
                  onclick={() => handleDelete(cred)}
                >
                  <Trash2 size={13} />
                </button>
              </div>
            </div>
          {/each}
        {/if}
      </div>
    {:else}
      <!-- Create / Edit Form View -->
      <div class="edit-form-body">
        <div class="form-header">
          <span class="form-subheading">{editingId ? 'Edit Saved Credential' : 'New Saved Credential'}</span>
          <span class="vault-indicator">
            <Shield size={11} />
            <span>OS Keyring Secured</span>
          </span>
        </div>

        <div class="form-cell">
          <label for="cred-name">Credential Name</label>
          <input
            id="cred-name"
            type="text"
            placeholder="e.g. Active Directory Production, AWS Ubuntu Key"
            bind:value={formName}
            class="input-field"
          />
        </div>

        <!-- Credential Type Switcher -->
        <div class="type-selector">
          <button
            class="type-btn {formType === 'domain' ? 'active' : ''}"
            onclick={() => (formType = 'domain')}
          >
            <Monitor size={13} />
            <span>Windows Domain (RDP)</span>
          </button>
          <button
            class="type-btn {formType === 'password' ? 'active' : ''}"
            onclick={() => (formType = 'password')}
          >
            <Lock size={13} />
            <span>Linux / Password</span>
          </button>
          <button
            class="type-btn {formType === 'keyfile' ? 'active' : ''}"
            onclick={() => (formType = 'keyfile')}
          >
            <Key size={13} />
            <span>SSH Key Pair</span>
          </button>
        </div>

        <div class="fields-grid">
          {#if formType === 'domain'}
            <div class="form-cell col-1">
              <label for="cred-domain">Windows Domain</label>
              <input
                id="cred-domain"
                type="text"
                placeholder="e.g. CORP or CONTOSO"
                bind:value={formDomain}
                class="input-field"
              />
            </div>
            <div class="form-cell col-1">
              <label for="cred-user">Username</label>
              <input
                id="cred-user"
                type="text"
                placeholder="e.g. Administrator"
                bind:value={formUsername}
                class="input-field"
              />
            </div>
            <div class="form-cell col-2">
              <div class="field-label-row">
                <label for="cred-pass">Domain Password</label>
                {#if formPassword || editingId}
                  <button
                    type="button"
                    class="field-copy-btn"
                    title="Copy Password"
                    onclick={copyFormPassword}
                  >
                    {#if copiedFormPassword}
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
                  id="cred-pass"
                  type={showFormPassword ? 'text' : 'password'}
                  placeholder={existingSecretRef?.type === 'keyring' ? 'Stored in OS Keyring (leave blank to keep)' : '••••••••'}
                  bind:value={formPassword}
                  class="input-field has-toggle"
                />
                <button
                  type="button"
                  class="password-toggle-btn"
                  title={showFormPassword ? 'Hide password' : 'View saved password'}
                  onclick={toggleFormPassword}
                >
                  {#if showFormPassword}
                    <EyeOff size={14} />
                  {:else}
                    <Eye size={14} />
                  {/if}
                </button>
              </div>
            </div>
          {:else if formType === 'password'}
            <div class="form-cell col-1">
              <label for="cred-user">Username</label>
              <input
                id="cred-user"
                type="text"
                placeholder="e.g. root or ubuntu"
                bind:value={formUsername}
                class="input-field"
              />
            </div>
            <div class="form-cell col-1">
              <div class="field-label-row">
                <label for="cred-pass">Password</label>
                {#if formPassword || editingId}
                  <button
                    type="button"
                    class="field-copy-btn"
                    title="Copy Password"
                    onclick={copyFormPassword}
                  >
                    {#if copiedFormPassword}
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
                  id="cred-pass"
                  type={showFormPassword ? 'text' : 'password'}
                  placeholder={existingSecretRef?.type === 'keyring' ? 'Stored in OS Keyring (leave blank to keep)' : '••••••••'}
                  bind:value={formPassword}
                  class="input-field has-toggle"
                />
                <button
                  type="button"
                  class="password-toggle-btn"
                  title={showFormPassword ? 'Hide password' : 'View saved password'}
                  onclick={toggleFormPassword}
                >
                  {#if showFormPassword}
                    <EyeOff size={14} />
                  {:else}
                    <Eye size={14} />
                  {/if}
                </button>
              </div>
            </div>
          {:else}
            <div class="form-cell col-1">
              <label for="cred-user">Username</label>
              <input
                id="cred-user"
                type="text"
                placeholder="e.g. deploy or ec2-user"
                bind:value={formUsername}
                class="input-field"
              />
            </div>
            <div class="form-cell col-2">
              <label for="cred-key">Private Key Path</label>
              <input
                id="cred-key"
                type="text"
                placeholder="~/.ssh/id_ed25519"
                bind:value={formKeyPath}
                class="input-field"
              />
            </div>
            <div class="form-cell col-2">
              <div class="field-label-row">
                <label for="cred-passphrase">Passphrase (optional)</label>
                {#if formPassphrase || editingId}
                  <button
                    type="button"
                    class="field-copy-btn"
                    title="Copy Passphrase"
                    onclick={copyFormPassphrase}
                  >
                    {#if copiedFormPassphrase}
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
                  id="cred-passphrase"
                  type={showFormPassphrase ? 'text' : 'password'}
                  placeholder={existingSecretRef?.type === 'keyring' ? 'Stored in OS Keyring (leave blank to keep)' : 'Key passphrase'}
                  bind:value={formPassphrase}
                  class="input-field has-toggle"
                />
                <button
                  type="button"
                  class="password-toggle-btn"
                  title={showFormPassphrase ? 'Hide passphrase' : 'View saved passphrase'}
                  onclick={toggleFormPassphrase}
                >
                  {#if showFormPassphrase}
                    <EyeOff size={14} />
                  {:else}
                    <Eye size={14} />
                  {/if}
                </button>
              </div>
            </div>
          {/if}
        </div>

        <div class="form-actions">
          <button class="btn btn-secondary" onclick={cancelEdit}>Cancel</button>
          <button class="btn btn-primary" onclick={handleSave}>
            <Check size={13} />
            <span>{editingId ? 'Update Profile' : 'Save Profile'}</span>
          </button>
        </div>
      </div>
    {/if}

    <!-- Modal Footer for list view -->
    {#if !isEditing}
      <div class="modal-bottom">
        <span class="footnote">
          Profiles are securely referenced by ID. Updating a profile automatically rotates credentials for all attached servers.
        </span>
        <button
          class="btn btn-secondary"
          onclick={() => (appState.isCredentialsModalOpen = false)}
        >
          Close
        </button>
      </div>
    {/if}
  </div>
</div>

<style>
  .modal-scrim {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal-card {
    width: 580px;
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
    padding: 16px 20px;
    border-bottom: 1px solid var(--hairline);
  }

  .modal-titles {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .mono-eyebrow {
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.08em;
    color: var(--mute);
    text-transform: uppercase;
  }

  .dialog-heading {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
    color: var(--ink);
    letter-spacing: -0.3px;
  }

  .icon-btn-close {
    background: transparent;
    border: none;
    color: var(--mute);
    cursor: pointer;
    padding: 4px;
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
    transition: color 0.12s ease;
  }

  .icon-btn-close:hover {
    color: var(--ink);
  }

  .list-controls {
    flex-shrink: 0;
    display: flex;
    gap: 10px;
    padding: 14px 20px 8px 20px;
  }

  .search-box {
    flex: 1;
    display: flex;
    align-items: center;
    background: var(--canvas-subtle);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-sm);
    padding: 0 10px;
    gap: 8px;
    height: 32px;
  }

  .search-input {
    flex: 1;
    background: transparent;
    border: none;
    color: var(--ink);
    font-size: 12px;
    outline: none;
  }

  .new-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 12px;
    font-size: 12px;
  }

  .profiles-list {
    flex: 1;
    min-height: 0;
    padding: 10px 20px 16px 20px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .empty-state {
    padding: 40px 20px;
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }

  .empty-state :global(.empty-icon) {
    color: var(--mute);
    margin-bottom: 4px;
    opacity: 0.6;
  }

  .empty-title {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    color: var(--ink);
  }

  .empty-desc {
    margin: 0;
    font-size: 12px;
    color: var(--body);
    max-width: 360px;
    line-height: 1.5;
  }

  .empty-btn {
    margin-top: 10px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    height: 30px;
  }

  .profile-card {
    background: var(--canvas-subtle);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-sm);
    padding: 12px;
    display: flex;
    align-items: center;
    gap: 12px;
    transition: border-color 0.12s ease;
  }

  .profile-card:hover {
    border-color: var(--hairline-hover);
  }

  .type-icon-box {
    width: 32px;
    height: 32px;
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--canvas-elevated);
    border: 1px solid var(--hairline);
    color: var(--ink);
  }

  .type-icon-box.domain {
    color: #60a5fa;
    border-color: rgba(96, 165, 250, 0.2);
  }

  .type-icon-box.key {
    color: #34d399;
    border-color: rgba(52, 211, 153, 0.2);
  }

  .type-icon-box.pass {
    color: #fbbf24;
    border-color: rgba(251, 191, 36, 0.2);
  }

  .profile-info-col {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 6px;
    overflow: hidden;
  }

  .profile-header-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .profile-name {
    font-size: 13px;
    font-weight: 600;
    color: var(--ink);
  }

  .badge {
    font-size: 10px;
    padding: 1px 6px;
    border-radius: 4px;
    font-family: var(--font-mono);
  }

  .domain-badge {
    background: rgba(96, 165, 250, 0.1);
    color: #93c5fd;
    border: 1px solid rgba(96, 165, 250, 0.25);
  }

  .key-badge {
    background: rgba(52, 211, 153, 0.1);
    color: #6ee7b7;
    border: 1px solid rgba(52, 211, 153, 0.25);
  }

  .pass-badge {
    background: rgba(251, 191, 36, 0.1);
    color: #fde68a;
    border: 1px solid rgba(251, 191, 36, 0.25);
  }

  .profile-meta-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
    color: var(--body);
  }

  .user-pill {
    font-family: var(--font-mono);
    color: var(--ink-secondary);
  }

  .key-path-pill {
    font-family: var(--font-mono);
    color: var(--mute);
    max-width: 150px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .linked-pill {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--mute);
    font-size: 11px;
  }

  .profile-actions-col {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .action-btn {
    background: transparent;
    border: 1px solid transparent;
    color: var(--mute);
    padding: 5px;
    border-radius: 4px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.12s ease;
  }

  .action-btn:hover {
    background: var(--canvas-elevated);
    color: var(--ink);
    border-color: var(--hairline);
  }

  .action-btn.danger:hover {
    color: #ef4444;
    border-color: rgba(239, 68, 68, 0.3);
  }

  .edit-form-body {
    padding: 16px 20px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .form-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .form-subheading {
    font-size: 13px;
    font-weight: 600;
    color: var(--ink);
  }

  .vault-indicator {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 10px;
    font-family: var(--font-mono);
    color: #34d399;
    background: rgba(52, 211, 153, 0.1);
    padding: 2px 6px;
    border-radius: var(--radius-pill);
    border: 1px solid rgba(52, 211, 153, 0.2);
  }

  .type-selector {
    display: flex;
    gap: 6px;
    background: var(--canvas-subtle);
    padding: 4px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--hairline);
  }

  .type-btn {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 28px;
    font-size: 11px;
    font-weight: 500;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    color: var(--mute);
    cursor: pointer;
    transition: all 0.12s ease;
  }

  .type-btn.active {
    background: var(--canvas-elevated);
    color: var(--ink);
    border-color: var(--hairline);
  }

  .fields-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
  }

  .form-cell {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
  }

  .col-1 { flex: 1; min-width: 180px; }
  .col-2 { flex: 2; min-width: 280px; }

  label {
    font-size: 11px;
    font-weight: 500;
    color: var(--body);
  }

  .input-field {
    background: var(--canvas-subtle);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-sm);
    color: var(--ink);
    font-size: 12px;
    padding: 7px 10px;
    outline: none;
    transition: border-color 0.12s ease;
  }

  .input-field:focus {
    border-color: var(--hairline-hover);
  }

  .form-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 8px;
  }

  .modal-bottom {
    padding: 12px 20px;
    border-top: 1px solid var(--hairline);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    background: var(--canvas-subtle);
  }

  .footnote {
    font-size: 11px;
    color: var(--mute);
    line-height: 1.4;
  }

  .field-label-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .field-copy-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: transparent;
    border: none;
    color: var(--mute);
    font-size: 10px;
    font-weight: 500;
    cursor: pointer;
    padding: 1px 4px;
    border-radius: var(--radius-sm);
    transition: all 0.12s ease;
  }

  .field-copy-btn:hover {
    color: var(--ink);
    background: rgba(255, 255, 255, 0.05);
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

  .revealed-secret-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: rgba(251, 191, 36, 0.1);
    border: 1px solid rgba(251, 191, 36, 0.25);
    border-radius: 4px;
    padding: 1px 6px;
    font-size: 11px;
    font-family: var(--font-mono);
  }

  .revealed-secret-pill .secret-label {
    color: #fbbf24;
    font-weight: 600;
    font-size: 10px;
    text-transform: uppercase;
  }

  .revealed-secret-pill .secret-value {
    color: #fde68a;
  }

  .copy-secret-btn {
    background: transparent;
    border: none;
    color: #fbbf24;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    transition: opacity 0.12s ease;
  }

  .copy-secret-btn:hover {
    opacity: 0.8;
  }
</style>
