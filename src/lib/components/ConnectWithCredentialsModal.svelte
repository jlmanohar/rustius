<script lang="ts">
  import { onMount } from 'svelte';
  import type { Connection, CredentialOverride } from '../types';
  import {
    X,
    KeyRound,
    User,
    Lock,
    Eye,
    EyeOff,
    Building2,
    ShieldAlert,
    Play
  } from '@lucide/svelte';

  let {
    connection,
    onconnect,
    onclose
  }: {
    connection: Connection;
    onconnect: (override: CredentialOverride) => void;
    onclose: () => void;
  } = $props();

  let username = $state('');
  let password = $state('');
  let domain = $state('');
  let showPassword = $state(false);

  let passwordInputRef = $state<HTMLInputElement | null>(null);
  let usernameInputRef = $state<HTMLInputElement | null>(null);

  const isRdp = $derived((connection.connection_type || connection.conn_type) === 'rdp');

  onMount(() => {
    if (connection.auth && 'username' in connection.auth && connection.auth.username) {
      username = connection.auth.username;
    }
    if (connection.rdp_config?.domain) {
      domain = connection.rdp_config.domain;
    }

    if (username) {
      passwordInputRef?.focus();
    } else {
      usernameInputRef?.focus();
    }
  });

  function handleSubmit(e?: Event) {
    e?.preventDefault();
    onconnect({
      username: username.trim() || undefined,
      password: password || undefined,
      domain: isRdp && domain.trim() ? domain.trim() : undefined,
    });
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      onclose();
    }
  }
</script>

<svelte:window onkeydown={handleKeyDown} />

<div class="modal-scrim" role="presentation" onclick={onclose}>
  <div
    class="modal-card"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => {
      if (e.key === 'Escape') {
        handleKeyDown(e);
      } else {
        e.stopPropagation();
      }
    }}
  >
    <!-- Modal Header -->
    <div class="modal-top">
      <div class="modal-titles">
        <div class="eyebrow-row">
          <KeyRound size={12} class="eyebrow-icon" />
          <span class="mono-eyebrow">ONE-OFF SESSION</span>
        </div>
        <h2 class="dialog-heading">Connect with Different Credentials</h2>
        <p class="dialog-subtitle">
          Enter temporary credentials for <span class="highlight">{connection.name || connection.host}</span> ({connection.host}:{connection.port}).
        </p>
      </div>
      <button class="btn-icon" title="Close" onclick={onclose}>
        <X size={16} />
      </button>
    </div>

    <!-- Security Warning Banner -->
    <div class="security-banner">
      <ShieldAlert size={15} class="banner-icon" />
      <span class="banner-text">
        These credentials will be used <strong>only for this session</strong> and will <strong>never</strong> be saved to disk, vault, or configuration.
      </span>
    </div>

    <!-- Form -->
    <form onsubmit={handleSubmit} class="modal-form">
      <div class="form-group">
        <label for="temp-username">Username</label>
        <div class="input-with-icon">
          <span class="field-icon"><User size={14} /></span>
          <input
            id="temp-username"
            bind:this={usernameInputRef}
            type="text"
            class="input-field with-icon"
            placeholder="e.g. Administrator or root"
            bind:value={username}
            autocomplete="off"
            spellcheck="false"
          />
        </div>
      </div>

      <div class="form-group">
        <label for="temp-password">Password</label>
        <div class="input-with-icon">
          <span class="field-icon"><Lock size={14} /></span>
          <input
            id="temp-password"
            bind:this={passwordInputRef}
            type={showPassword ? 'text' : 'password'}
            class="input-field with-icon has-toggle"
            placeholder="Enter temporary password"
            bind:value={password}
            autocomplete="new-password"
          />
          <button
            type="button"
            class="toggle-visibility-btn"
            title={showPassword ? 'Hide password' : 'Show password'}
            onclick={() => (showPassword = !showPassword)}
            tabindex="-1"
          >
            {#if showPassword}
              <EyeOff size={14} />
            {:else}
              <Eye size={14} />
            {/if}
          </button>
        </div>
      </div>

      {#if isRdp}
        <div class="form-group">
          <label for="temp-domain">Domain (Optional)</label>
          <div class="input-with-icon">
            <span class="field-icon"><Building2 size={14} /></span>
            <input
              id="temp-domain"
              type="text"
              class="input-field with-icon"
              placeholder="e.g. CORP or WORKGROUP"
              bind:value={domain}
              autocomplete="off"
              spellcheck="false"
            />
          </div>
        </div>
      {/if}

      <!-- Actions -->
      <div class="modal-actions">
        <button type="button" class="btn btn-secondary" onclick={onclose}>
          Cancel
        </button>
        <button type="submit" class="btn btn-primary connect-btn">
          <Play size={12} />
          <span>Connect</span>
        </button>
      </div>
    </form>
  </div>
</div>

<style>
  .modal-scrim {
    position: fixed;
    inset: 0;
    z-index: 9999;
    background: rgba(0, 0, 0, 0.75);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 20px;
    animation: fadeIn 0.15s ease-out;
  }

  .modal-card {
    width: 100%;
    max-width: 440px;
    background: var(--canvas-elevated);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-floating);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    animation: scaleIn 0.15s cubic-bezier(0.16, 1, 0.3, 1);
  }

  .modal-top {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    padding: 18px 20px 14px;
    border-bottom: 1px solid var(--hairline-soft);
  }

  .modal-titles {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .eyebrow-row {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--accent-amber);
  }

  :global(.eyebrow-icon) {
    color: var(--accent-amber);
  }

  .mono-eyebrow {
    font-family: var(--font-mono);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.08em;
  }

  .dialog-heading {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    color: var(--ink);
    letter-spacing: -0.2px;
  }

  .dialog-subtitle {
    margin: 2px 0 0;
    font-size: 12px;
    color: var(--body);
    line-height: 1.4;
  }

  .highlight {
    color: var(--ink);
    font-weight: 500;
  }

  .security-banner {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    margin: 14px 20px 0;
    padding: 10px 12px;
    background: rgba(245, 166, 35, 0.08);
    border: 1px solid rgba(245, 166, 35, 0.25);
    border-radius: var(--radius-sm);
    color: #e5a024;
    font-size: 11px;
    line-height: 1.4;
  }

  :global(.banner-icon) {
    flex-shrink: 0;
    margin-top: 1px;
    color: var(--accent-amber);
  }

  .banner-text strong {
    color: #f7b744;
  }

  .modal-form {
    padding: 16px 20px 18px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .form-group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .form-group label {
    font-size: 11.5px;
    font-weight: 500;
    color: var(--body);
  }

  .input-with-icon {
    position: relative;
    display: flex;
    align-items: center;
  }

  .field-icon {
    position: absolute;
    left: 10px;
    color: var(--mute);
    display: flex;
    align-items: center;
    pointer-events: none;
  }

  .input-field.with-icon {
    padding-left: 32px;
  }

  .input-field.has-toggle {
    padding-right: 34px;
  }

  .toggle-visibility-btn {
    position: absolute;
    right: 8px;
    background: transparent;
    border: none;
    color: var(--mute);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 4px;
    border-radius: 4px;
    transition: color 0.12s ease;
  }

  .toggle-visibility-btn:hover {
    color: var(--ink);
  }

  .modal-actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 6px;
    padding-top: 14px;
    border-top: 1px solid var(--hairline-soft);
  }

  .connect-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  @keyframes fadeIn {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  @keyframes scaleIn {
    from {
      opacity: 0;
      transform: scale(0.97);
    }
    to {
      opacity: 1;
      transform: scale(1);
    }
  }
</style>
