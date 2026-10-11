<script lang="ts">
  import { appState } from '../stores/appState.svelte';
  import type { ImportSummary } from '../types';
  import {
    X,
    Upload,
    FileCode,
    Lock,
    CheckCircle2,
    AlertCircle,
    Eye,
    EyeOff,
    FileSpreadsheet,
    ShieldCheck
  } from '@lucide/svelte';

  type ImportSource = 'mremoteng' | 'mobaxterm';

  let selectedSource = $state<ImportSource>('mremoteng');
  let masterPassword = $state<string>('');
  let showPassword = $state<boolean>(false);
  let filePath = $state<string>('');
  let fileContent = $state<string>('');
  let fileName = $state<string>('');
  let isDragging = $state<boolean>(false);
  let isLoading = $state<boolean>(false);
  let errorMsg = $state<string | null>(null);
  let summary = $state<ImportSummary | null>(null);

  function resetFile() {
    fileName = '';
    filePath = '';
    fileContent = '';
    errorMsg = null;
    summary = null;
  }

  function handleSourceChange(source: ImportSource) {
    selectedSource = source;
    resetFile();
  }

  function onFileSelected(e: Event) {
    const input = e.target as HTMLInputElement;
    if (input.files && input.files[0]) {
      processFile(input.files[0]);
    }
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault();
    isDragging = false;
    if (e.dataTransfer && e.dataTransfer.files && e.dataTransfer.files[0]) {
      processFile(e.dataTransfer.files[0]);
    }
  }

  function processFile(file: File) {
    fileName = file.name;
    errorMsg = null;
    summary = null;

    // Check file extension compatibility
    const ext = file.name.split('.').pop()?.toLowerCase();
    if (selectedSource === 'mremoteng' && ext !== 'xml' && ext !== 'mremoteng') {
      errorMsg = 'Please select an mRemoteNG XML or .mremoteng file (confCons.xml)';
    } else if (selectedSource === 'mobaxterm' && ext !== 'mxtsessions' && ext !== 'ini') {
      errorMsg = 'Please select a MobaXterm .mxtsessions or .ini file';
    }

    const reader = new FileReader();
    reader.onload = (evt) => {
      fileContent = (evt.target?.result as string) || '';
    };
    reader.onerror = () => {
      errorMsg = 'Failed to read the selected file.';
    };
    reader.readAsText(file);
  }

  async function handleImport() {
    if (!fileContent && !filePath) {
      errorMsg = 'Please select a configuration file to import.';
      return;
    }

    isLoading = true;
    errorMsg = null;
    summary = null;

    try {
      let res: ImportSummary;
      if (fileContent) {
        res = await appState.importSessionsContent(
          fileContent,
          selectedSource,
          masterPassword.trim() || undefined
        );
      } else {
        res = await appState.importSessions(
          filePath.trim(),
          selectedSource,
          masterPassword.trim() || undefined
        );
      }

      summary = res;
      if (res.connections_imported === 0 && res.folders_imported === 0) {
        errorMsg = 'No connections or folders could be extracted. Please check the file format and master password.';
      }
    } catch (err: any) {
      console.error('Import error:', err);
      errorMsg = typeof err === 'string' ? err : err.message || 'Import failed. Check password or file contents.';
    } finally {
      isLoading = false;
    }
  }
</script>

<div
  class="modal-scrim"
  role="presentation"
  onclick={() => (appState.isImportModalOpen = false)}
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
        <span class="mono-eyebrow">MIGRATION WIZARD</span>
        <h2 class="dialog-heading">Import Remote Sessions</h2>
      </div>
      <button
        class="icon-btn-close"
        title="Close"
        onclick={() => (appState.isImportModalOpen = false)}
      >
        <X size={14} />
      </button>
    </div>

    <!-- Modal Body -->
    <div class="modal-body">
      <!-- Source App Selector Tabs -->
      <div class="source-selector">
        <button
          type="button"
          class="source-tab {selectedSource === 'mremoteng' ? 'active' : ''}"
          onclick={() => handleSourceChange('mremoteng')}
        >
          <div class="tab-indicator"></div>
          <FileCode size={16} />
          <div class="tab-meta">
            <span class="tab-title">mRemoteNG</span>
            <span class="tab-sub">confCons.xml</span>
          </div>
        </button>

        <button
          type="button"
          class="source-tab {selectedSource === 'mobaxterm' ? 'active' : ''}"
          onclick={() => handleSourceChange('mobaxterm')}
        >
          <div class="tab-indicator"></div>
          <FileSpreadsheet size={16} />
          <div class="tab-meta">
            <span class="tab-title">MobaXterm</span>
            <span class="tab-sub">.mxtsessions / .ini</span>
          </div>
        </button>
      </div>

      <!-- File Drop / Select Area -->
      <div
        class="drop-area {isDragging ? 'dragging' : ''} {fileName ? 'has-file' : ''}"
        role="region"
        aria-label="File upload dropzone"
        ondragover={(e) => {
          e.preventDefault();
          isDragging = true;
        }}
        ondragleave={() => (isDragging = false)}
        ondrop={handleDrop}
      >
        <input
          type="file"
          id="import-file-input"
          class="file-input-hidden"
          accept={selectedSource === 'mremoteng' ? '.xml,.mremoteng' : '.mxtsessions,.ini'}
          onchange={onFileSelected}
        />

        {#if fileName}
          <div class="file-picked-info">
            <FileCode size={24} class="picked-icon" />
            <div class="picked-meta">
              <span class="picked-name">{fileName}</span>
              <span class="picked-size">Ready for decryption & import</span>
            </div>
            <button
              type="button"
              class="btn-text-change"
              onclick={() => {
                const input = document.getElementById('import-file-input') as HTMLInputElement;
                input?.click();
              }}
            >
              Change
            </button>
          </div>
        {:else}
          <label for="import-file-input" class="drop-label">
            <div class="drop-icon-circle">
              <Upload size={18} />
            </div>
            <span class="drop-title">
              Click to browse or drag & drop configuration
            </span>
            <span class="drop-hint">
              {selectedSource === 'mremoteng'
                ? 'Typically located in %AppData%\\mRemoteNG\\confCons.xml'
                : 'Exported .mxtsessions file or MobaXterm.ini'}
            </span>
          </label>
        {/if}
      </div>

      <!-- Master Password Input -->
      <div class="field-group">
        <label for="master-pwd-input" class="field-label">
          <Lock size={12} class="label-icon" />
          <span>MASTER / EXPORT PASSWORD (OPTIONAL)</span>
        </label>
        <div class="pwd-input-wrap">
          <input
            id="master-pwd-input"
            class="geist-input"
            type={showPassword ? 'text' : 'password'}
            bind:value={masterPassword}
            placeholder={selectedSource === 'mremoteng' ? "Leave empty for default 'mR3m'" : "Leave empty to use Windows credentials"}
          />
          <button
            type="button"
            class="toggle-eye"
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
        <p class="field-hint">
          {selectedSource === 'mremoteng'
            ? "mRemoteNG uses default password 'mR3m' unless you protected your connections with a custom password."
            : 'MobaXterm encrypts saved passwords with machine credentials by default, or with your custom master password.'}
        </p>
      </div>

      <!-- Security Guarantee Callout -->
      <div class="security-callout">
        <ShieldCheck size={14} class="sec-icon" />
        <div class="sec-text">
          <span>
            Decrypted credentials are automatically saved into the <strong>OS Keyring</strong>.
            Zero plaintext passwords are stored on disk.
          </span>
        </div>
      </div>

      <!-- Feedback: Error / Success -->
      {#if errorMsg}
        <div class="banner error-banner">
          <AlertCircle size={14} />
          <span>{errorMsg}</span>
        </div>
      {/if}

      {#if summary}
        <div class="banner success-banner">
          <CheckCircle2 size={14} />
          <div class="success-text">
            <strong>Import completed successfully:</strong>
            <span>
              {summary.connections_imported} connection{summary.connections_imported !== 1 ? 's' : ''} and {summary.folders_imported} folder{summary.folders_imported !== 1 ? 's' : ''} added to your workspace.
            </span>
          </div>
        </div>
      {/if}
    </div>

    <!-- Modal Actions -->
    <div class="modal-actions">
      <button
        type="button"
        class="btn btn-secondary"
        onclick={() => (appState.isImportModalOpen = false)}
      >
        {summary ? 'Done' : 'Cancel'}
      </button>

      {#if !summary}
        <button
          type="button"
          class="btn btn-primary"
          disabled={isLoading || (!fileContent && !filePath)}
          onclick={handleImport}
        >
          {#if isLoading}
            <span class="spinner"></span>
            <span>Importing & Decrypting...</span>
          {:else}
            <Upload size={13} />
            <span>Start Import</span>
          {/if}
        </button>
      {/if}
    </div>
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
    width: 480px;
    max-width: 92vw;
    background: #000000;
    border: 1px solid #262626;
    border-radius: 8px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    box-shadow: 0 20px 40px rgba(0, 0, 0, 0.85);
    animation: modalScale 0.15s ease-out;
  }

  @keyframes modalScale {
    from {
      opacity: 0;
      transform: scale(0.97);
    }
    to {
      opacity: 1;
      transform: scale(1);
    }
  }

  .modal-top {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid #262626;
  }

  .modal-titles {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .mono-eyebrow {
    font-family: var(--font-mono, monospace);
    font-size: 10px;
    letter-spacing: 0.08em;
    color: #666666;
    text-transform: uppercase;
  }

  .dialog-heading {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    color: #ededed;
    letter-spacing: -0.01em;
  }

  .icon-btn-close {
    background: transparent;
    border: none;
    color: #666666;
    cursor: pointer;
    padding: 4px;
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: color 0.15s ease;
  }

  .icon-btn-close:hover {
    color: #ededed;
  }

  .modal-body {
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  /* Source selector tabs */
  .source-selector {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    background: #0a0a0a;
    border: 1px solid #1f1f1f;
    border-radius: 6px;
    padding: 4px;
  }

  .source-tab {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 5px;
    color: #888888;
    cursor: pointer;
    transition: all 0.15s ease;
    text-align: left;
  }

  .source-tab:hover {
    color: #d1d1d1;
    background: #141414;
  }

  .source-tab.active {
    background: #1a1a1a;
    border-color: #333333;
    color: #ffffff;
  }

  .tab-meta {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .tab-title {
    font-size: 13px;
    font-weight: 500;
  }

  .tab-sub {
    font-size: 10px;
    font-family: var(--font-mono, monospace);
    color: #666666;
  }

  .source-tab.active .tab-sub {
    color: #888888;
  }

  /* Drop zone */
  .drop-area {
    border: 1px dashed #333333;
    background: #0a0a0a;
    border-radius: 6px;
    padding: 20px 16px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .drop-area:hover,
  .drop-area.dragging {
    border-color: #666666;
    background: #111111;
  }

  .drop-area.has-file {
    border-style: solid;
    border-color: #262626;
    background: #0d0d0d;
    padding: 14px 16px;
  }

  .file-input-hidden {
    display: none;
  }

  .drop-label {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    text-align: center;
  }

  .drop-icon-circle {
    width: 36px;
    height: 36px;
    border-radius: 50%;
    background: #171717;
    border: 1px solid #262626;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #888888;
  }

  .drop-title {
    font-size: 13px;
    font-weight: 500;
    color: #ededed;
  }

  .drop-hint {
    font-size: 11px;
    color: #666666;
  }

  .file-picked-info {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
  }

  :global(.picked-icon) {
    color: #3b82f6;
    flex-shrink: 0;
  }

  .picked-meta {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow: hidden;
  }

  .picked-name {
    font-size: 13px;
    font-weight: 500;
    color: #ffffff;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .picked-size {
    font-size: 11px;
    color: #888888;
  }

  .btn-text-change {
    background: transparent;
    border: none;
    color: #3b82f6;
    font-size: 12px;
    cursor: pointer;
    padding: 4px 8px;
    border-radius: 4px;
    transition: background 0.15s ease;
  }

  .btn-text-change:hover {
    background: rgba(59, 130, 246, 0.1);
  }

  /* Field group */
  .field-group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .field-label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-family: var(--font-mono, monospace);
    font-size: 11px;
    letter-spacing: 0.04em;
    color: #888888;
    text-transform: uppercase;
  }

  .pwd-input-wrap {
    position: relative;
    display: flex;
    align-items: center;
  }

  .geist-input {
    width: 100%;
    background: #0a0a0a;
    border: 1px solid #262626;
    border-radius: 6px;
    padding: 8px 12px;
    font-size: 13px;
    color: #ffffff;
    outline: none;
    transition: border-color 0.15s ease;
  }

  .geist-input:focus {
    border-color: #666666;
  }

  .toggle-eye {
    position: absolute;
    right: 8px;
    background: transparent;
    border: none;
    color: #666666;
    cursor: pointer;
    padding: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .toggle-eye:hover {
    color: #ededed;
  }

  .field-hint {
    margin: 0;
    font-size: 11px;
    color: #666666;
    line-height: 1.4;
  }

  /* Callout */
  .security-callout {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    background: #0a0a0a;
    border: 1px solid #1f1f1f;
    border-radius: 6px;
    padding: 10px 12px;
  }

  :global(.sec-icon) {
    color: #10b981;
    margin-top: 2px;
    flex-shrink: 0;
  }

  .sec-text {
    font-size: 11px;
    color: #888888;
    line-height: 1.4;
  }

  .sec-text strong {
    color: #e5e5e5;
  }

  /* Banners */
  .banner {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border-radius: 6px;
    font-size: 12px;
  }

  .error-banner {
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.3);
    color: #fca5a5;
  }

  .success-banner {
    background: rgba(16, 185, 129, 0.1);
    border: 1px solid rgba(16, 185, 129, 0.3);
    color: #6ee7b7;
  }

  .success-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  /* Modal Actions */
  .modal-actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding: 14px 20px;
    border-top: 1px solid #262626;
    background: #050505;
  }

  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    font-weight: 500;
    padding: 7px 14px;
    border-radius: 5px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .btn-secondary {
    background: transparent;
    border: 1px solid #262626;
    color: #a3a3a3;
  }

  .btn-secondary:hover {
    color: #ffffff;
    border-color: #404040;
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
    opacity: 0.5;
    cursor: not-allowed;
  }

  .spinner {
    width: 12px;
    height: 12px;
    border: 2px solid rgba(0, 0, 0, 0.2);
    border-top-color: #000000;
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
