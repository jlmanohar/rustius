<script lang="ts">
  import { appState } from '../stores/appState.svelte';
  import { X, Check, Shield, Sliders, Terminal, ArrowLeftRight } from '@lucide/svelte';

  type SettingsCategory = 'general' | 'terminal' | 'sftp' | 'security';

  let activeCategory = $state<SettingsCategory>('general');

  let autoClose = $state(appState.config.app_settings?.close_tab_on_exit ?? false);
  let showSidebar = $state(appState.config.app_settings?.show_sidebar ?? true);
  let sftpFollowTerminal = $state(appState.config.app_settings?.sftp_follow_terminal_folder ?? true);
  let termType = $state(appState.config.app_settings?.default_terminal_type ?? 'xterm-256color');
  let fontSize = $state(appState.config.app_settings?.font_size ?? 13);
  let fontFamily = $state(appState.config.app_settings?.font_family ?? "'Geist Mono', monospace");

  async function handleSave() {
    const updated = {
      ...appState.config.app_settings,
      close_tab_on_exit: autoClose,
      show_sidebar: showSidebar,
      sftp_follow_terminal_folder: sftpFollowTerminal,
      default_terminal_type: termType.trim() || 'xterm-256color',
      font_size: Number(fontSize) || 13,
      font_family: fontFamily.trim() || "'Geist Mono', monospace",
    };

    await appState.saveAppSettings(updated);
    appState.isSettingsModalOpen = false;
  }
</script>

<div
  class="modal-scrim"
  role="presentation"
  onclick={() => (appState.isSettingsModalOpen = false)}
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
        <span class="mono-eyebrow">PREFERENCES</span>
        <h2 class="dialog-heading">Application Settings</h2>
      </div>
      <button
        type="button"
        class="icon-btn-close"
        title="Close"
        onclick={() => (appState.isSettingsModalOpen = false)}
      >
        <X size={14} />
      </button>
    </div>

    <!-- Modal Main: Nav Rail + Content Pane -->
    <div class="modal-main">
      <!-- Left Category Nav Rail -->
      <nav class="settings-nav" aria-label="Settings categories">
        <button
          type="button"
          class="nav-tab {activeCategory === 'general' ? 'active' : ''}"
          onclick={() => (activeCategory = 'general')}
        >
          <Sliders size={13} />
          <span>General</span>
        </button>

        <button
          type="button"
          class="nav-tab {activeCategory === 'terminal' ? 'active' : ''}"
          onclick={() => (activeCategory = 'terminal')}
        >
          <Terminal size={13} />
          <span>Terminal</span>
        </button>

        <button
          type="button"
          class="nav-tab {activeCategory === 'sftp' ? 'active' : ''}"
          onclick={() => (activeCategory = 'sftp')}
        >
          <ArrowLeftRight size={13} />
          <span>SFTP</span>
        </button>

        <button
          type="button"
          class="nav-tab {activeCategory === 'security' ? 'active' : ''}"
          onclick={() => (activeCategory = 'security')}
        >
          <Shield size={13} />
          <span>Security</span>
        </button>
      </nav>

      <!-- Right Content Pane -->
      <div class="settings-content">
        {#if activeCategory === 'general'}
          <div class="category-header">
            <h3 class="category-title">General</h3>
            <p class="category-subtitle">Application behavior, tab lifecycle, and workspace layout.</p>
          </div>

          <div class="settings-group">
            <!-- Setting: Auto-close tab on exit -->
            <div class="setting-row">
              <div class="setting-meta">
                <label for="auto-close-toggle" class="setting-title">Auto-close tab on exit</label>
                <p class="setting-desc">
                  Automatically close the active tab when the SSH or local shell process exits.
                </p>
              </div>
              <div class="setting-control">
                <label class="switch">
                  <input
                    id="auto-close-toggle"
                    type="checkbox"
                    bind:checked={autoClose}
                  />
                  <span class="slider"></span>
                </label>
              </div>
            </div>

            <!-- Setting: Show sidebar -->
            <div class="setting-row">
              <div class="setting-meta">
                <label for="show-sidebar-toggle" class="setting-title">Show connection sidebar</label>
                <p class="setting-desc">
                  Display the left tree navigation sidebar for connections and folders.
                </p>
              </div>
              <div class="setting-control">
                <label class="switch">
                  <input
                    id="show-sidebar-toggle"
                    type="checkbox"
                    bind:checked={showSidebar}
                  />
                  <span class="slider"></span>
                </label>
              </div>
            </div>
          </div>
        {:else if activeCategory === 'terminal'}
          <div class="category-header">
            <h3 class="category-title">Terminal</h3>
            <p class="category-subtitle">Emulation settings, font sizing, and typography for SSH and local shells.</p>
          </div>

          <div class="settings-group">
            <div class="setting-field">
              <label for="term-type-input" class="field-label">DEFAULT TERMINAL TYPE</label>
              <input
                id="term-type-input"
                class="geist-input"
                type="text"
                bind:value={termType}
                placeholder="xterm-256color"
              />
              <span class="field-hint">Defines the $TERM environment variable advertised to remote hosts.</span>
            </div>

            <div class="setting-field">
              <label for="font-size-input" class="field-label">FONT SIZE (PX)</label>
              <input
                id="font-size-input"
                class="geist-input"
                type="number"
                min="10"
                max="24"
                bind:value={fontSize}
              />
            </div>

            <div class="setting-field">
              <label for="font-family-input" class="field-label">FONT FAMILY</label>
              <input
                id="font-family-input"
                class="geist-input"
                type="text"
                bind:value={fontFamily}
                placeholder="'Geist Mono', monospace"
              />
            </div>
          </div>
        {:else if activeCategory === 'sftp'}
          <div class="category-header">
            <h3 class="category-title">SFTP & Files</h3>
            <p class="category-subtitle">File transfer options and terminal folder synchronization.</p>
          </div>

          <div class="settings-group">
            <div class="setting-row">
              <div class="setting-meta">
                <label for="sftp-follow-toggle" class="setting-title">Follow terminal directory in SFTP</label>
                <p class="setting-desc">
                  Automatically synchronize the SFTP file navigator whenever you change directories (cd) in the SSH terminal.
                </p>
              </div>
              <div class="setting-control">
                <label class="switch">
                  <input
                    id="sftp-follow-toggle"
                    type="checkbox"
                    bind:checked={sftpFollowTerminal}
                  />
                  <span class="slider"></span>
                </label>
              </div>
            </div>
          </div>
        {:else if activeCategory === 'security'}
          <div class="category-header">
            <h3 class="category-title">Security & Vault</h3>
            <p class="category-subtitle">Encryption, credentials storage, and operating system keyring integration.</p>
          </div>

          <div class="settings-group">
            <div class="security-card">
              <div class="security-card-header">
                <div class="security-card-title">
                  <Shield size={14} class="sec-icon" />
                  <span>Native OS Keyring Vault</span>
                </div>
                <span class="sec-status-pill">Active</span>
              </div>
              <p class="sec-desc">
                Passkeys, SSH passphrases, and passwords are automatically secured via the host operating system's Credential Manager. Zero plaintext passwords are saved to disk.
              </p>
            </div>
          </div>
        {/if}
      </div>
    </div>

    <!-- Modal Footer -->
    <div class="modal-actions">
      <button
        type="button"
        class="btn btn-secondary"
        onclick={() => (appState.isSettingsModalOpen = false)}
      >
        Cancel
      </button>
      <button type="button" class="btn btn-primary" onclick={handleSave}>
        <Check size={13} />
        <span>Save Preferences</span>
      </button>
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
    width: 600px;
    max-width: 92vw;
    height: 480px;
    max-height: 85vh;
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
    flex-shrink: 0;
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    padding: 14px 18px;
    border-bottom: 1px solid #262626;
    background: #050505;
  }

  .modal-titles {
    display: flex;
    flex-direction: column;
    gap: 3px;
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
    font-size: 14px;
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

  /* Main Split Layout: Nav Rail + Content Pane */
  .modal-main {
    flex: 1;
    min-height: 0;
    display: flex;
    overflow: hidden;
  }

  .settings-nav {
    width: 140px;
    flex-shrink: 0;
    background: #050505;
    border-right: 1px solid #262626;
    padding: 10px 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .nav-tab {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 10px;
    border-radius: 6px;
    background: transparent;
    border: none;
    color: #777777;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    text-align: left;
    transition: all 0.12s ease;
    font-family: inherit;
  }

  .nav-tab:hover {
    color: #ededed;
    background: #141414;
  }

  .nav-tab.active {
    color: #ffffff;
    background: #1a1a1a;
  }

  .settings-content {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    padding: 16px 20px;
    display: flex;
    flex-direction: column;
    gap: 16px;
    background: #000000;
  }

  .category-header {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding-bottom: 12px;
    border-bottom: 1px solid #1a1a1a;
  }

  .category-title {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    color: #ffffff;
    letter-spacing: -0.01em;
  }

  .category-subtitle {
    margin: 0;
    font-size: 11px;
    color: #777777;
    line-height: 1.4;
  }

  .settings-group {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .setting-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding-bottom: 12px;
    border-bottom: 1px solid #141414;
  }

  .setting-row:last-child {
    border-bottom: none;
    padding-bottom: 0;
  }

  .setting-meta {
    display: flex;
    flex-direction: column;
    gap: 3px;
    flex: 1;
    min-width: 0;
  }

  .setting-title {
    font-size: 12.5px;
    font-weight: 500;
    color: #ededed;
    cursor: pointer;
  }

  .setting-desc {
    margin: 0;
    font-size: 11px;
    color: #707070;
    line-height: 1.4;
  }

  .setting-field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .field-label {
    font-family: var(--font-mono, monospace);
    font-size: 10px;
    letter-spacing: 0.06em;
    color: #666666;
  }

  .field-hint {
    font-size: 10.5px;
    color: #555555;
    line-height: 1.35;
  }

  .geist-input {
    background: #0a0a0a;
    border: 1px solid #262626;
    border-radius: 6px;
    padding: 7px 10px;
    color: #ededed;
    font-size: 12px;
    font-family: inherit;
    outline: none;
    transition: border-color 0.15s ease;
  }

  .geist-input:focus {
    border-color: #ededed;
  }

  /* Toggle Switch */
  .switch {
    position: relative;
    display: inline-block;
    width: 34px;
    height: 19px;
    flex-shrink: 0;
  }

  .switch input {
    opacity: 0;
    width: 0;
    height: 0;
  }

  .slider {
    position: absolute;
    cursor: pointer;
    inset: 0;
    background-color: #262626;
    transition: 0.2s;
    border-radius: 20px;
  }

  .slider:before {
    position: absolute;
    content: '';
    height: 13px;
    width: 13px;
    left: 3px;
    bottom: 3px;
    background-color: #888888;
    transition: 0.2s;
    border-radius: 50%;
  }

  input:checked + .slider {
    background-color: #ededed;
  }

  input:checked + .slider:before {
    transform: translateX(15px);
    background-color: #000000;
  }

  .modal-actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
    padding: 12px 18px;
    background: #050505;
    border-top: 1px solid #262626;
  }

  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 14px;
    border-radius: 6px;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    font-family: inherit;
    transition: all 0.15s ease;
  }

  .btn-secondary {
    background: transparent;
    color: #888888;
    border: 1px solid #262626;
  }

  .btn-secondary:hover {
    color: #ededed;
    border-color: #404040;
    background: #141414;
  }

  .btn-primary {
    background: #ededed;
    color: #000000;
    border: 1px solid #ededed;
  }

  .btn-primary:hover {
    background: #ffffff;
  }

  .security-card {
    background: #080808;
    border: 1px solid #262626;
    border-radius: 6px;
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .security-card-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .security-card-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    font-weight: 500;
    color: #ededed;
  }

  :global(.sec-icon) {
    color: #ededed;
  }

  .sec-status-pill {
    font-size: 10px;
    font-family: var(--font-mono, monospace);
    color: #ededed;
    background: #141414;
    border: 1px solid #262626;
    padding: 1px 6px;
    border-radius: 4px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .sec-desc {
    margin: 0;
    font-size: 11px;
    color: #707070;
    line-height: 1.45;
  }
</style>
