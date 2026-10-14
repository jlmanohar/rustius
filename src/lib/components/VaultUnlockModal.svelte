<script lang="ts">
  import { appState } from '../stores/appState.svelte';
  import { Shield, Lock, Eye, EyeOff, KeyRound, AlertCircle } from '@lucide/svelte';

  let masterPassword = $state('');
  let showPassword = $state(false);
  let errorMessage = $state('');
  let isSubmitting = $state(false);
  let inputRef = $state<HTMLInputElement | null>(null);

  $effect(() => {
    if (appState.isVaultUnlockModalOpen && inputRef) {
      inputRef.focus();
    }
  });

  async function handleUnlock(e: SubmitEvent) {
    e.preventDefault();
    if (!masterPassword) {
      errorMessage = 'Please enter your master password.';
      return;
    }

    isSubmitting = true;
    errorMessage = '';

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
</script>

{#if appState.isVaultUnlockModalOpen}
  <div class="modal-scrim" role="presentation">
    <div
      class="modal-card"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
    >
      <div class="unlock-header">
        <div class="icon-badge">
          <Lock size={22} class="lock-icon" />
        </div>
        <h2 class="dialog-heading">Unlock Credential Vault</h2>
        <p class="dialog-subtitle">
          Rustius is operating in <strong>Portable Mode</strong> with master password protection. Enter your master password to decrypt your saved credentials.
        </p>
      </div>

      <form onsubmit={handleUnlock} class="unlock-form">
        {#if errorMessage}
          <div class="error-banner">
            <AlertCircle size={14} />
            <span>{errorMessage}</span>
          </div>
        {/if}

        <div class="input-wrapper">
          <KeyRound size={14} class="field-icon" />
          <input
            bind:this={inputRef}
            type={showPassword ? 'text' : 'password'}
            class="password-input"
            placeholder="Enter master password..."
            bind:value={masterPassword}
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

        <div class="actions-row">
          <button
            type="button"
            class="btn btn-secondary"
            onclick={() => (appState.isVaultUnlockModalOpen = false)}
          >
            Skip for now
          </button>
          <button type="submit" class="btn btn-primary" disabled={isSubmitting}>
            {#if isSubmitting}
              Unlocking...
            {:else}
              <Shield size={14} />
              <span>Unlock Vault</span>
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

  .unlock-form {
    display: flex;
    flex-direction: column;
    gap: 16px;
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
