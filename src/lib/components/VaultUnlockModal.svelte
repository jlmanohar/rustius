<script lang="ts">
  import { appState } from '../stores/appState.svelte';
  import { Shield, ShieldCheck, ShieldAlert, Lock, Eye, EyeOff, KeyRound, AlertCircle } from '@lucide/svelte';

  let masterPassword = $state('');
  let confirmPassword = $state('');
  let showPassword = $state(false);
  let showConfirmPassword = $state(false);
  let errorMessage = $state('');
  let isSubmitting = $state(false);
  let inputRef = $state<HTMLInputElement | null>(null);

  let isSetupMode = $derived(!appState.vaultStatus?.has_vault_file);

  $effect(() => {
    if (appState.isVaultUnlockModalOpen) {
      errorMessage = '';
      masterPassword = '';
      confirmPassword = '';
      showPassword = false;
      showConfirmPassword = false;
      if (inputRef) {
        setTimeout(() => inputRef?.focus(), 50);
      }
    }
  });

  async function handleSubmit(e: SubmitEvent) {
    e.preventDefault();
    errorMessage = '';

    if (isSetupMode) {
      if (!masterPassword) {
        errorMessage = 'Please enter a master password.';
        return;
      }
      if (masterPassword.length < 6) {
        errorMessage = 'Master password must be at least 6 characters.';
        return;
      }
      if (masterPassword !== confirmPassword) {
        errorMessage = 'Passwords do not match. Please verify and try again.';
        return;
      }

      isSubmitting = true;
      try {
        await appState.setupMasterPassword(masterPassword);
        masterPassword = '';
        confirmPassword = '';
      } catch (err: any) {
        errorMessage = err?.message || 'Failed to initialize master password vault.';
      } finally {
        isSubmitting = false;
      }
    } else {
      if (!masterPassword) {
        errorMessage = 'Please enter your master password.';
        return;
      }

      isSubmitting = true;
      try {
        const ok = await appState.unlockVault(masterPassword);
        if (!ok) {
          errorMessage = 'Incorrect master password. Please try again.';
        } else {
          masterPassword = '';
        }
      } catch (err: any) {
        errorMessage = err?.message || 'Failed to unlock vault';
      } finally {
        isSubmitting = false;
      }
    }
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      appState.isVaultUnlockModalOpen = false;
      e.stopPropagation();
    }
  }
</script>

{#if appState.isVaultUnlockModalOpen}
  <div
    class="modal-scrim"
    role="presentation"
    onclick={() => {
      appState.isVaultUnlockModalOpen = false;
    }}
  >
    <div
      class="modal-card {isSetupMode ? 'modal-card-setup' : ''}"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={handleKeyDown}
    >
      <div class="unlock-header">
        <div class="icon-badge {isSetupMode ? 'icon-badge-setup' : ''}">
          {#if isSetupMode}
            <ShieldCheck size={24} class="lock-icon" />
          {:else}
            <Lock size={22} class="lock-icon" />
          {/if}
        </div>
        <h2 class="dialog-heading">
          {isSetupMode ? 'Set Up Master Password' : 'Unlock Credential Vault'}
        </h2>
        <p class="dialog-subtitle">
          {#if isSetupMode}
            Welcome to Rustius! Set a Master Password to encrypt and protect your SSH, SFTP, and RDP credentials.
          {:else}
            Rustius is operating in <strong>{appState.vaultStatus?.is_portable ? 'Portable Mode' : 'Vault Mode'}</strong> with master password protection. Enter your master password to decrypt your saved credentials.
          {/if}
        </p>
      </div>

      {#if isSetupMode}
        <div class="security-callout">
          <ShieldAlert size={16} class="callout-icon" />
          <div class="callout-content">
            <span class="callout-title">Zero-Knowledge Local Encryption</span>
            <p class="callout-desc">
              Your credentials are encrypted locally with Argon2id and AES-256-GCM. Rustius cannot recover your data if you forget this password.
            </p>
          </div>
        </div>
      {/if}

      <form onsubmit={handleSubmit} class="unlock-form">
        {#if errorMessage}
          <div class="error-banner">
            <AlertCircle size={14} />
            <span>{errorMessage}</span>
          </div>
        {/if}

        <div class="field-group">
          {#if isSetupMode}
            <label for="master-password-input" class="field-label">MASTER PASSWORD</label>
          {/if}
          <div class="input-wrapper">
            <KeyRound size={14} class="field-icon" />
            <input
              id="master-password-input"
              bind:this={inputRef}
              type={showPassword ? 'text' : 'password'}
              class="password-input"
              placeholder={isSetupMode ? 'Choose master password (min. 6 characters)...' : 'Enter master password...'}
              bind:value={masterPassword}
              autocomplete="new-password"
            />
            <button
              type="button"
              class="toggle-visibility-btn"
              title={showPassword ? 'Hide password' : 'Show password'}
              onclick={() => (showPassword = !showPassword)}
            >
              {#if showPassword}
                <EyeOff size={14} />
              {:else}
                <Eye size={14} />
              {/if}
            </button>
          </div>
        </div>

        {#if isSetupMode}
          <div class="field-group">
            <label for="confirm-password-input" class="field-label">CONFIRM MASTER PASSWORD</label>
            <div class="input-wrapper">
              <KeyRound size={14} class="field-icon" />
              <input
                id="confirm-password-input"
                type={showConfirmPassword ? 'text' : 'password'}
                class="password-input"
                placeholder="Re-enter master password to confirm..."
                bind:value={confirmPassword}
                autocomplete="new-password"
              />
              <button
                type="button"
                class="toggle-visibility-btn"
                title={showConfirmPassword ? 'Hide password' : 'Show password'}
                onclick={() => (showConfirmPassword = !showConfirmPassword)}
              >
                {#if showConfirmPassword}
                  <EyeOff size={14} />
                {:else}
                  <Eye size={14} />
                {/if}
              </button>
            </div>
          </div>
        {/if}

        <div class="actions-row {isSetupMode ? 'actions-row-setup' : ''}">
          {#if isSetupMode}
            <button
              type="button"
              class="btn btn-secondary"
              onclick={() => (appState.isVaultUnlockModalOpen = false)}
            >
              Skip (Use OS Keyring)
            </button>
          {:else}
            <button
              type="button"
              class="btn btn-secondary"
              onclick={() => (appState.isVaultUnlockModalOpen = false)}
            >
              Skip for now
            </button>
          {/if}
          <button
            type="submit"
            class="btn btn-primary"
            disabled={isSubmitting}
          >
            {#if isSubmitting}
              <span>{isSetupMode ? 'Creating Secure Vault...' : 'Unlocking...'}</span>
            {:else}
              {#if isSetupMode}
                <ShieldCheck size={14} />
                <span>Create Secure Vault</span>
              {:else}
                <Shield size={14} />
                <span>Unlock Vault</span>
              {/if}
            {/if}
          </button>
        </div>
      </form>
    </div>
  </div>
{/if}

<style>
  .modal-scrim {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.78);
    backdrop-filter: blur(6px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1100;
  }

  .modal-card {
    width: 440px;
    max-width: 90vw;
    background: #0a0a0a;
    border: 1px solid #262626;
    border-radius: 12px;
    padding: 24px;
    box-shadow: 0 20px 40px rgba(0, 0, 0, 0.7);
    color: #ededed;
  }

  .modal-card-setup {
    width: 460px;
  }

  .unlock-header {
    text-align: center;
    margin-bottom: 20px;
  }

  .icon-badge {
    width: 44px;
    height: 44px;
    margin: 0 auto 12px;
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid #333333;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #ffffff;
  }

  .icon-badge-setup {
    background: rgba(16, 185, 129, 0.1);
    border-color: rgba(16, 185, 129, 0.3);
    color: #10b981;
  }

  .dialog-heading {
    font-size: 1.15rem;
    font-weight: 600;
    color: #ffffff;
    margin: 0 0 6px;
    letter-spacing: -0.02em;
  }

  .dialog-subtitle {
    font-size: 0.83rem;
    color: #888888;
    line-height: 1.45;
    margin: 0;
  }

  .dialog-subtitle strong {
    color: #ffffff;
  }

  .security-callout {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid #222222;
    border-radius: 8px;
    padding: 10px 14px;
    margin-bottom: 16px;
  }

  :global(.callout-icon) {
    color: #f59e0b;
    flex-shrink: 0;
    margin-top: 2px;
  }

  .callout-content {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .callout-title {
    font-size: 0.78rem;
    font-weight: 600;
    color: #e5e5e5;
  }

  .callout-desc {
    font-size: 0.73rem;
    color: #888888;
    line-height: 1.4;
    margin: 0;
  }

  .unlock-form {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .field-group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .field-label {
    font-size: 0.68rem;
    font-weight: 600;
    letter-spacing: 0.04em;
    color: #737373;
  }

  .error-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    background: rgba(239, 68, 68, 0.12);
    border: 1px solid rgba(239, 68, 68, 0.35);
    border-radius: 6px;
    padding: 8px 12px;
    font-size: 0.8rem;
    color: #f87171;
  }

  .input-wrapper {
    position: relative;
    display: flex;
    align-items: center;
    background: #000000;
    border: 1px solid #333333;
    border-radius: 6px;
    transition: border-color 0.15s ease;
  }

  .input-wrapper:focus-within {
    border-color: #666666;
  }

  :global(.field-icon) {
    position: absolute;
    left: 12px;
    color: #666666;
    pointer-events: none;
  }

  .password-input {
    width: 100%;
    background: transparent;
    border: none;
    outline: none;
    padding: 10px 38px 10px 36px;
    font-size: 0.88rem;
    color: #ffffff;
    font-family: inherit;
  }

  .toggle-visibility-btn {
    position: absolute;
    right: 10px;
    background: transparent;
    border: none;
    color: #777777;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 4px;
    border-radius: 4px;
  }

  .toggle-visibility-btn:hover {
    color: #ffffff;
  }

  .actions-row {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    margin-top: 4px;
  }

  .actions-row-setup {
    margin-top: 8px;
  }

  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 8px 16px;
    font-size: 0.85rem;
    font-weight: 500;
    border-radius: 6px;
    cursor: pointer;
    transition: all 0.15s ease;
  }



  .btn-secondary {
    background: transparent;
    border: 1px solid #2a2a2a;
    color: #888888;
  }

  .btn-secondary:hover {
    background: #171717;
    color: #ffffff;
  }

  .btn-primary {
    background: #ffffff;
    border: 1px solid #ffffff;
    color: #000000;
  }

  .btn-primary:hover:not(:disabled) {
    background: #e5e5e5;
  }

  .btn-primary:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
</style>
