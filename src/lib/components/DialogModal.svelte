<script lang="ts">
  import { appState } from '../stores/appState.svelte';
  import {
    X,
    Check,
    AlertTriangle,
    Trash2,
    FolderPlus,
    Info,
    AlertCircle,
  } from '@lucide/svelte';
  import { onMount } from 'svelte';

  let dialog = $derived(appState.activeDialog);
  let inputValue = $state('');
  let inputRef = $state<HTMLInputElement | null>(null);

  $effect(() => {
    if (dialog) {
      inputValue = dialog.options.initialValue || '';
      requestAnimationFrame(() => {
        if (inputRef) {
          inputRef.focus();
          inputRef.select();
        }
      });
    }
  });

  function handleConfirm() {
    if (!dialog) return;
    if (dialog.options.type === 'prompt') {
      const val = inputValue.trim();
      appState.closeDialog(val || null);
    } else if (dialog.options.type === 'confirm') {
      appState.closeDialog(true);
    } else {
      appState.closeDialog(undefined);
    }
  }

  function handleCancel() {
    if (!dialog) return;
    if (dialog.options.type === 'prompt') {
      appState.closeDialog(null);
    } else if (dialog.options.type === 'confirm') {
      appState.closeDialog(false);
    } else {
      appState.closeDialog(undefined);
    }
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      handleCancel();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      handleConfirm();
    }
  }
</script>

<svelte:window onkeydown={dialog ? handleKeyDown : undefined} />

{#if dialog}
  <!-- Scrim backdrop -->
  <div
    class="modal-scrim"
    role="presentation"
    onclick={handleCancel}
  >
    <!-- Modal Card -->
    <div
      class="modal-card {dialog.options.isDanger ? 'danger-card' : ''}"
      role="dialog"
      aria-modal="true"
      aria-labelledby="dialog-title"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
    >
      <!-- Header -->
      <div class="dialog-header">
        <div class="header-left">
          <div class="icon-box {dialog.options.isDanger ? 'danger' : dialog.options.type === 'prompt' ? 'prompt' : 'info'}">
            {#if dialog.options.isDanger}
              <Trash2 size={16} />
            {:else if dialog.options.type === 'prompt'}
              <FolderPlus size={16} />
            {:else if dialog.options.type === 'alert'}
              <AlertCircle size={16} />
            {:else}
              <Info size={16} />
            {/if}
          </div>
          <span id="dialog-title" class="dialog-title">{dialog.options.title}</span>
        </div>
        <button
          type="button"
          class="close-btn"
          title="Close dialog (Esc)"
          onclick={handleCancel}
        >
          <X size={14} />
        </button>
      </div>

      <!-- Body -->
      <div class="dialog-body">
        {#if dialog.options.message}
          <p class="dialog-message">{dialog.options.message}</p>
        {/if}

        {#if dialog.options.type === 'prompt'}
          <div class="input-container">
            <input
              bind:this={inputRef}
              type="text"
              class="input-field dialog-input"
              placeholder={dialog.options.placeholder || 'Enter value...'}
              bind:value={inputValue}
              onkeydown={(e) => {
                if (e.key === 'Enter') {
                  e.stopPropagation();
                  handleConfirm();
                }
              }}
            />
          </div>
        {/if}
      </div>

      <!-- Footer Actions -->
      <div class="dialog-footer">
        {#if dialog.options.type !== 'alert'}
          <button
            type="button"
            class="btn btn-secondary"
            onclick={handleCancel}
          >
            {dialog.options.cancelText || 'Cancel'}
          </button>
        {/if}

        <button
          type="button"
          class="btn {dialog.options.isDanger ? 'btn-danger' : 'btn-primary'}"
          onclick={handleConfirm}
        >
          {#if dialog.options.isDanger}
            <Trash2 size={13} />
          {:else}
            <Check size={13} />
          {/if}
          <span>{dialog.options.confirmText || (dialog.options.type === 'alert' ? 'OK' : 'Confirm')}</span>
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .modal-scrim {
    position: fixed;
    inset: 0;
    z-index: 9999;
    background: rgba(0, 0, 0, 0.72);
    backdrop-filter: blur(6px);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px;
    animation: fadeIn 0.12s ease-out;
  }

  .modal-card {
    width: 100%;
    max-width: 440px;
    background: var(--canvas-elevated);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-md);
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.7), 0 0 0 1px var(--hairline);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    animation: scaleIn 0.14s cubic-bezier(0.16, 1, 0.3, 1);
  }

  .dialog-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px;
    border-bottom: 1px solid var(--hairline);
    background: var(--canvas-subtle);
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .icon-box {
    width: 28px;
    height: 28px;
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--canvas);
    border: 1px solid var(--hairline);
    color: var(--ink);
  }

  .icon-box.prompt {
    color: var(--accent-cyan);
    border-color: rgba(80, 227, 194, 0.25);
    background: rgba(80, 227, 194, 0.08);
  }

  .icon-box.danger {
    color: #ff4d4d;
    border-color: rgba(255, 77, 77, 0.25);
    background: rgba(255, 77, 77, 0.08);
  }

  .icon-box.info {
    color: var(--accent-blue);
    border-color: rgba(0, 112, 243, 0.25);
    background: rgba(0, 112, 243, 0.08);
  }

  .dialog-title {
    font-size: 13.5px;
    font-weight: 600;
    color: var(--ink);
    letter-spacing: -0.2px;
  }

  .close-btn {
    background: transparent;
    border: none;
    color: var(--mute);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border-radius: var(--radius-sm);
    transition: all 0.12s ease;
  }

  .close-btn:hover {
    color: var(--ink);
    background: var(--canvas-hover);
  }

  .dialog-body {
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .dialog-message {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--body);
    word-break: break-word;
  }

  .input-container {
    width: 100%;
  }

  .dialog-input {
    width: 100%;
    font-size: 13px;
    padding: 8px 12px;
    height: 36px;
  }

  .dialog-footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 18px;
    background: var(--canvas-subtle);
    border-top: 1px solid var(--hairline);
  }

  .btn-danger {
    background-color: #e00;
    color: #ffffff;
    border-color: #e00;
  }

  .btn-danger:hover {
    background-color: #c00;
    border-color: #c00;
  }

  .btn-danger:active {
    background-color: #a00;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  @keyframes scaleIn {
    from {
      opacity: 0;
      transform: scale(0.96) translateY(-4px);
    }
    to {
      opacity: 1;
      transform: scale(1) translateY(0);
    }
  }
</style>
