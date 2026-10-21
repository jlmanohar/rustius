<script lang="ts">
  import { appState } from '../stores/appState.svelte';
  import {
    X,
    Check,
    Shield,
    Sliders,
    Terminal,
    ArrowLeftRight,
    Monitor,
    HardDrive,
    Laptop,
    Lock,
    Unlock,
    KeyRound,
    AlertCircle,
    CheckCircle2,
    RotateCcw
  } from '@lucide/svelte';
  import type { VaultBackend } from '$lib/types';

  type SettingsCategory = 'general' | 'terminal' | 'sftp' | 'rdp' | 'security';

  let activeCategory = $state<SettingsCategory>('general');

  let autoClose = $state(appState.config.app_settings?.close_tab_on_exit ?? false);
  let showSidebar = $state(appState.config.app_settings?.show_sidebar ?? true);
  let sftpFollowTerminal = $state(appState.config.app_settings?.sftp_follow_terminal_folder ?? true);
  let termType = $state(appState.config.app_settings?.default_terminal_type ?? 'xterm-256color');
  let fontSize = $state(appState.config.app_settings?.font_size ?? 13);
  let defaultRdpEngine = $state<'ironrdp' | 'freerdp'>(appState.config.app_settings?.default_rdp_engine ?? 'ironrdp');
  let defaultRdpResolutionMode = $state<'auto' | 'native' | '1080p' | '720p' | 'custom'>(appState.config.app_settings?.default_rdp_resolution_mode ?? 'auto');
  let defaultRdpScaleFactor = $state<number>(appState.config.app_settings?.default_rdp_scale_factor ?? 0);
  let defaultRdpClipboardSharing = $state<boolean>(appState.config.app_settings?.default_rdp_clipboard_sharing ?? true);
  let defaultRdpEgfx = $state<boolean>(appState.config.app_settings?.default_rdp_egfx ?? false);
  const MONOSPACE_FONTS = [
    { label: 'Geist Mono (Default)', value: 'Geist Mono' },
    { label: 'JetBrains Mono', value: 'JetBrains Mono' },
    { label: 'Fira Code', value: 'Fira Code' },
    { label: 'Cascadia Mono', value: 'Cascadia Mono' },
    { label: 'Cascadia Code', value: 'Cascadia Code' },
    { label: 'Consolas', value: 'Consolas' },
    { label: 'Source Code Pro', value: 'Source Code Pro' },
    { label: 'SF Mono', value: 'SF Mono' },
    { label: 'Menlo', value: 'Menlo' },
    { label: 'Ubuntu Mono', value: 'Ubuntu Mono' },
    { label: 'DejaVu Sans Mono', value: 'DejaVu Sans Mono' },
    { label: 'Courier New', value: 'Courier New' },
  ];

  function isFontAvailable(fontName: string): boolean {
    if (typeof document === 'undefined') return true;

    try {
      if (document.fonts?.check?.(`16px "${fontName}"`)) {
        return true;
      }
    } catch {}

    try {
      const canvas = document.createElement('canvas');
      const ctx = canvas.getContext('2d');
      if (!ctx) return true;

      const testString = 'mmmmmmmmmmlli!@#$%^&*()1234567890';
      const baseFonts = ['monospace', 'sans-serif', 'serif'];

      for (const base of baseFonts) {
        ctx.font = `72px ${base}`;
        const baseWidth = ctx.measureText(testString).width;

        ctx.font = `72px "${fontName}", ${base}`;
        const testWidth = ctx.measureText(testString).width;

        if (Math.abs(baseWidth - testWidth) > 0.1) {
          return true;
        }
      }
    } catch {}

    return false;
  }

  // Bundled fonts embedded inside the binary (100% offline ready)
  const BUNDLED_FONTS = new Set(['Geist Mono', 'JetBrains Mono', 'Fira Code', 'Source Code Pro']);

  function detectAvailableFonts() {
    return MONOSPACE_FONTS.filter((f) => BUNDLED_FONTS.has(f.value) || isFontAvailable(f.value));
  }

  let availableFonts = $state(detectAvailableFonts());

  $effect(() => {
    if (typeof document !== 'undefined' && 'fonts' in document) {
      document.fonts.ready.then(() => {
        availableFonts = detectAvailableFonts();
      });
    }
  });

  const initialFont = appState.config.app_settings?.font_family?.replace(/['"]/g, '').trim() || 'Geist Mono';
  let fontFamily = $state(initialFont);
  let customFontFamily = $state(initialFont);
  let selectedFontPreset = $state<string>('custom');

  $effect(() => {
    const isPreset = availableFonts.some((f) => f.value.toLowerCase() === fontFamily.toLowerCase());
    const matched = availableFonts.find((f) => f.value.toLowerCase() === fontFamily.toLowerCase())?.value;
    if (isPreset && matched) {
      selectedFontPreset = matched;
    } else {
      selectedFontPreset = 'custom';
      customFontFamily = fontFamily;
    }
  });

  function handleFontPresetSelect(val: string) {
    selectedFontPreset = val;
    if (val !== 'custom') {
      fontFamily = val;
    } else {
      fontFamily = customFontFamily.trim() || 'Geist Mono';
    }
  }
  let selectedVaultBackend = $state<VaultBackend>(appState.config.app_settings?.vault_backend ?? 'auto');
  let isMigrating = $state(false);
  let vaultNotice = $state<{ type: 'success' | 'error'; text: string } | null>(null);

  $effect(() => {
    if (activeCategory === 'security') {
      void appState.fetchVaultStatus();
    }
  });

  async function handleBackendChange(target: VaultBackend) {
    if (target === selectedVaultBackend || isMigrating) return;
    vaultNotice = null;
    isMigrating = true;

    try {
      let masterPw: string | undefined = undefined;

      // If switching to master password mode and vault is not yet set up or locked:
      if (target === 'master_password' || (target === 'auto' && appState.vaultStatus?.is_portable)) {
        if (!appState.vaultStatus?.has_vault_file) {
          const pw = await appState.prompt({
            title: 'Set Master Password',
            message: 'To protect your credentials in the encrypted vault file, please enter a master password:',
            placeholder: 'Master password...',
            inputType: 'password'
          });
          if (!pw) {
            isMigrating = false;
            return;
          }
          masterPw = pw;
        } else if (!appState.vaultStatus?.is_unlocked) {
          const pw = await appState.prompt({
            title: 'Unlock Vault to Migrate',
            message: 'Please enter your master password to unlock and migrate credentials:',
            placeholder: 'Master password...',
            inputType: 'password'
          });
          if (!pw) {
            isMigrating = false;
            return;
          }
          masterPw = pw;
        }
      } else if (target === 'keyring' && appState.vaultStatus?.effective_backend === 'master_password') {
        const ok = await appState.confirm({
          title: 'Migrate to Native OS Keyring',
          message: 'All connection and profile passwords in the vault will be migrated to this computer\'s secure credential manager. Continue?'
        });
        if (!ok) {
          isMigrating = false;
          return;
        }
      }

      await appState.changeVaultBackend(target, masterPw);
      selectedVaultBackend = target;
      vaultNotice = {
        type: 'success',
        text: `Credential storage updated to ${target === 'auto' ? 'Automatic' : target === 'master_password' ? 'Master Password Vault' : 'Native OS Keyring'} and secrets migrated successfully.`
      };
    } catch (err: any) {
      console.error('Failed to change vault backend:', err);
      vaultNotice = {
        type: 'error',
        text: err?.message || String(err) || 'Failed to change credential storage backend.'
      };
    } finally {
      isMigrating = false;
    }
  }

  async function handleLockVault() {
    try {
      await appState.lockVault();
      vaultNotice = { type: 'success', text: 'Vault locked. Passwords are now encrypted.' };
    } catch (err: any) {
      vaultNotice = { type: 'error', text: err?.message || 'Failed to lock vault.' };
    }
  }

  function handleUnlockVault() {
    appState.isVaultUnlockModalOpen = true;
  }

  async function handleChangeMasterPassword() {
    const newPw = await appState.prompt({
      title: 'Change Master Password',
      message: 'Enter a new master password for your encrypted vault:',
      placeholder: 'New master password...',
      inputType: 'password'
    });
    if (!newPw) return;

    try {
      await appState.setMasterPassword(newPw);
      vaultNotice = { type: 'success', text: 'Master password updated successfully.' };
    } catch (err: any) {
      vaultNotice = { type: 'error', text: err?.message || 'Failed to update master password.' };
    }
  }

  async function handleSave() {
    const updated = {
      ...appState.config.app_settings,
      close_tab_on_exit: autoClose,
      show_sidebar: showSidebar,
      sftp_follow_terminal_folder: sftpFollowTerminal,
      default_terminal_type: termType.trim() || 'xterm-256color',
      font_size: Number(fontSize) || 13,
      font_family: fontFamily.replace(/['"]/g, '').trim() || 'Geist Mono',
      vault_backend: selectedVaultBackend,
      default_rdp_engine: defaultRdpEngine,
      default_rdp_resolution_mode: defaultRdpResolutionMode,
      default_rdp_scale_factor: Number(defaultRdpScaleFactor) || 0,
      default_rdp_clipboard_sharing: defaultRdpClipboardSharing,
      default_rdp_egfx: defaultRdpEgfx,
    };

    await appState.saveAppSettings(updated);
    appState.isSettingsModalOpen = false;
  }

  async function handleRestoreDefaults() {
    const confirmed = await appState.confirm({
      title: 'Restore Default Settings',
      message: 'Are you sure you want to restore all application preferences (theme, fonts, terminal, SFTP, RDP) to factory defaults?',
      confirmText: 'Restore Defaults',
    });
    if (!confirmed) return;

    autoClose = false;
    showSidebar = true;
    sftpFollowTerminal = true;
    termType = 'xterm-256color';
    fontSize = 13;
    selectedFontPreset = 'Geist Mono';
    customFontFamily = '';
    fontFamily = 'Geist Mono';
    selectedVaultBackend = 'auto';
    defaultRdpEngine = 'ironrdp';
    defaultRdpResolutionMode = 'auto';
    defaultRdpScaleFactor = 0;
    defaultRdpClipboardSharing = true;
    defaultRdpEgfx = false;

    const updated = {
      ...appState.config.app_settings,
      close_tab_on_exit: false,
      show_sidebar: true,
      sftp_follow_terminal_folder: true,
      default_terminal_type: 'xterm-256color',
      font_size: 13,
      font_family: 'Geist Mono',
      vault_backend: 'auto' as VaultBackend,
      default_rdp_engine: 'ironrdp' as const,
      default_rdp_resolution_mode: 'auto' as const,
      default_rdp_scale_factor: 0,
      default_rdp_clipboard_sharing: true,
      default_rdp_egfx: false,
    };

    await appState.saveAppSettings(updated);
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
          class="nav-tab {activeCategory === 'rdp' ? 'active' : ''}"
          onclick={() => (activeCategory = 'rdp')}
        >
          <Monitor size={13} />
          <span>Remote Desktop</span>
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
              <label for="font-family-select" class="field-label">FONT FAMILY</label>
              <select
                id="font-family-select"
                class="geist-select"
                value={selectedFontPreset}
                onchange={(e) => handleFontPresetSelect(e.currentTarget.value)}
              >
                {#each availableFonts as font}
                  <option value={font.value} style="font-family: '{font.value}', monospace;">
                    {font.label}
                  </option>
                {/each}
                <option value="custom">Custom Font Name...</option>
              </select>
              <span class="field-hint">Select a curated monospace font for terminal sessions.</span>

              {#if selectedFontPreset === 'custom'}
                <div class="custom-font-box">
                  <label for="font-family-custom-input" class="field-label sub-label">CUSTOM FONT FAMILY NAME</label>
                  <input
                    id="font-family-custom-input"
                    class="geist-input"
                    type="text"
                    bind:value={customFontFamily}
                    oninput={() => {
                      fontFamily = customFontFamily.trim() || 'Geist Mono';
                    }}
                    placeholder="e.g. Inconsolata, Hack, Monaco"
                  />
                  <span class="field-hint">Enter the exact name of a font installed on your system.</span>
                </div>
              {/if}
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
        {:else if activeCategory === 'rdp'}
          <div class="category-header">
            <h3 class="category-title">Remote Desktop</h3>
            <p class="category-subtitle">Global defaults for RDP rendering engines, display resolution, and clipboard sharing.</p>
          </div>

          <div class="settings-group">
            <div class="setting-field">
              <label for="default-rdp-engine-select" class="field-label">DEFAULT RDP ENGINE</label>
              <select
                id="default-rdp-engine-select"
                class="geist-select"
                bind:value={defaultRdpEngine}
              >
                <option value="ironrdp">IronRDP (Cross-Platform · Canvas/GPU Render · Pure Rust)</option>
                <option value="freerdp">FreeRDP (Direct3D Native Windows Overlay)</option>
              </select>
              <span class="field-hint">
                IronRDP renders inside the tab canvas with WebGL and supports bidirectional file copy. FreeRDP uses native hardware acceleration. Individual connections can override this default.
              </span>
            </div>

            <div class="setting-field">
              <label for="default-rdp-res-select" class="field-label">DEFAULT DISPLAY RESOLUTION</label>
              <select
                id="default-rdp-res-select"
                class="geist-select"
                bind:value={defaultRdpResolutionMode}
              >
                <option value="auto">Auto (Fit Window · Larger UI)</option>
                <option value="native">Native Monitor (1440p / 4K / High Res)</option>
                <option value="1080p">1080p (Full HD · 1920×1080)</option>
                <option value="720p">720p (HD · 1280×720)</option>
              </select>
              <span class="field-hint">Default desktop resolution for new RDP sessions.</span>
            </div>

            <div class="setting-field">
              <label for="default-rdp-scale-select" class="field-label">DEFAULT DESKTOP SCALING (DPI)</label>
              <select
                id="default-rdp-scale-select"
                class="geist-select"
                bind:value={defaultRdpScaleFactor}
              >
                <option value={0}>Auto (Recommended · System DPI)</option>
                <option value={100}>100% (Standard 96 DPI)</option>
                <option value={125}>125%</option>
                <option value={150}>150% (Recommended for 1440p)</option>
                <option value={175}>175%</option>
                <option value={200}>200% (Recommended for 4K)</option>
              </select>
              <span class="field-hint">Controls remote Windows desktop scale factor (MS-RDPBCGR DPI negotiation).</span>
            </div>

            <div class="setting-row">
              <div class="setting-meta">
                <label for="default-rdp-clipboard-toggle" class="setting-title">Clipboard sharing enabled by default</label>
                <p class="setting-desc">
                  Enable bidirectional text and file copy-paste synchronization between local machine and remote RDP desktop.
                </p>
              </div>
              <div class="setting-control">
                <label class="switch">
                  <input
                    id="default-rdp-clipboard-toggle"
                    type="checkbox"
                    bind:checked={defaultRdpClipboardSharing}
                  />
                  <span class="slider"></span>
                </label>
              </div>
            </div>

            <!-- Setting: Default EGFX Pipeline -->
            <div class="setting-row">
              <div class="setting-meta">
                <label for="default-rdp-egfx-toggle" class="setting-title">Enable EGFX graphics pipeline (Experimental)</label>
                <p class="setting-desc">
                  Enable Microsoft Remote Desktop Graphics Pipeline (MS-RDPEGFX). Keep disabled (recommended) for maximum stability; activate to test progressive ClearCodec rendering.
                </p>
              </div>
              <div class="setting-control">
                <label class="switch">
                  <input
                    id="default-rdp-egfx-toggle"
                    type="checkbox"
                    bind:checked={defaultRdpEgfx}
                  />
                  <span class="slider"></span>
                </label>
              </div>
            </div>
          </div>
        {:else if activeCategory === 'security'}
          <div class="category-header">
            <h3 class="category-title">Security & Credential Storage</h3>
            <p class="category-subtitle">Configure password storage, encrypted master password vault, and OS keyring integration.</p>
          </div>

          {#if vaultNotice}
            <div class="vault-notice-banner {vaultNotice.type}">
              {#if vaultNotice.type === 'success'}
                <CheckCircle2 size={14} />
              {:else}
                <AlertCircle size={14} />
              {/if}
              <span>{vaultNotice.text}</span>
            </div>
          {/if}

          <div class="settings-group">
            <!-- Environment Mode Badge Card -->
            <div class="env-mode-card">
              <div class="env-icon-box">
                {#if appState.vaultStatus?.is_portable}
                  <HardDrive size={18} class="env-icon" />
                {:else}
                  <Laptop size={18} class="env-icon" />
                {/if}
              </div>
              <div class="env-info">
                <div class="env-title-row">
                  <span class="env-title">
                    {appState.vaultStatus?.is_portable ? 'Portable Mode Active' : 'Installed Mode Active'}
                  </span>
                  <span class="env-badge {appState.vaultStatus?.is_portable ? 'badge-portable' : 'badge-installed'}">
                    {appState.vaultStatus?.is_portable ? 'USB / Data Directory' : 'System AppData'}
                  </span>
                </div>
                <p class="env-desc">
                  {#if appState.vaultStatus?.is_portable}
                    Rustius detected a portable data folder next to the executable. Storage defaults to an encrypted <code>vault.enc</code> file, ensuring saved passwords travel with your USB drive.
                  {:else}
                    Rustius is running from standard system installation directories. Storage defaults to your operating system's native secure keyring (Windows Credential Manager / Keychain).
                  {/if}
                </p>
              </div>
            </div>

            <!-- Storage Backend Selection -->
            <div class="storage-section">
              <div class="section-label-row">
                <span class="field-label">CREDENTIAL STORAGE BACKEND</span>
                {#if isMigrating}
                  <span class="migrating-pill">Migrating secrets...</span>
                {/if}
              </div>

              <div class="backend-options-grid">
                <!-- Option 1: Auto -->
                <div
                  class="backend-card {selectedVaultBackend === 'auto' ? 'selected' : ''} {isMigrating ? 'disabled' : ''}"
                  role="button"
                  tabindex="0"
                  onclick={() => { void handleBackendChange('auto'); }}
                  onkeydown={(e) => e.key === 'Enter' && void handleBackendChange('auto')}
                >
                  <div class="backend-card-head">
                    <div class="radio-indicator">
                      {#if selectedVaultBackend === 'auto'}
                        <div class="radio-dot"></div>
                      {/if}
                    </div>
                    <div class="backend-name">
                      <span>Automatic</span>
                      <span class="rec-badge">Recommended</span>
                    </div>
                  </div>
                  <p class="backend-desc">
                    Intelligently adapts: uses <strong>Master Password Vault</strong> in Portable Mode, and <strong>Native OS Keyring</strong> in Installed Mode.
                  </p>
                  <div class="backend-footer">
                    <span class="effective-text">
                      Currently resolves to:
                      <strong>{appState.vaultStatus?.effective_backend === 'master_password' ? 'Master Password Vault' : 'Native OS Keyring'}</strong>
                    </span>
                  </div>
                </div>

                <!-- Option 2: Master Password Vault -->
                <div
                  class="backend-card {selectedVaultBackend === 'master_password' ? 'selected' : ''} {isMigrating ? 'disabled' : ''}"
                  role="button"
                  tabindex="0"
                  onclick={() => { void handleBackendChange('master_password'); }}
                  onkeydown={(e) => e.key === 'Enter' && void handleBackendChange('master_password')}
                >
                  <div class="backend-card-head">
                    <div class="radio-indicator">
                      {#if selectedVaultBackend === 'master_password'}
                        <div class="radio-dot"></div>
                      {/if}
                    </div>
                    <div class="backend-name">
                      <Lock size={13} />
                      <span>Master Password Vault</span>
                    </div>
                  </div>
                  <p class="backend-desc">
                    Encrypts all passwords into <code>vault.enc</code> using Argon2id and AES-256-GCM. Completely self-contained and portable across machines.
                  </p>
                </div>

                <!-- Option 3: OS Keyring -->
                <div
                  class="backend-card {selectedVaultBackend === 'keyring' ? 'selected' : ''} {isMigrating ? 'disabled' : ''}"
                  role="button"
                  tabindex="0"
                  onclick={() => { void handleBackendChange('keyring'); }}
                  onkeydown={(e) => e.key === 'Enter' && void handleBackendChange('keyring')}
                >
                  <div class="backend-card-head">
                    <div class="radio-indicator">
                      {#if selectedVaultBackend === 'keyring'}
                        <div class="radio-dot"></div>
                      {/if}
                    </div>
                    <div class="backend-name">
                      <Shield size={13} />
                      <span>Native OS Keyring</span>
                    </div>
                  </div>
                  <p class="backend-desc">
                    Delegates to Windows Credential Manager, macOS Keychain, or Linux Secret Service. Tied to this local user account.
                  </p>
                </div>
              </div>
            </div>

            <!-- Vault Status & Actions Panel (when effective backend is master_password) -->
            {#if appState.vaultStatus?.effective_backend === 'master_password'}
              <div class="vault-management-panel">
                <div class="vault-panel-header">
                  <div class="vault-status-indicator">
                    {#if !appState.vaultStatus.has_vault_file}
                      <span class="status-dot dot-gray"></span>
                      <span class="vault-status-label">No Master Password Set</span>
                    {:else if appState.vaultStatus.is_unlocked}
                      <span class="status-dot dot-green"></span>
                      <span class="vault-status-label">Vault Unlocked (Active Session)</span>
                    {:else}
                      <span class="status-dot dot-amber"></span>
                      <span class="vault-status-label">Vault Locked</span>
                    {/if}
                  </div>

                  <div class="vault-action-buttons">
                    {#if !appState.vaultStatus.has_vault_file}
                      <button
                        type="button"
                        class="btn-vault-action"
                        onclick={() => { void handleChangeMasterPassword(); }}
                      >
                        <KeyRound size={12} />
                        <span>Set Master Password</span>
                      </button>
                    {:else if appState.vaultStatus.is_unlocked}
                      <button
                        type="button"
                        class="btn-vault-action"
                        onclick={() => { void handleChangeMasterPassword(); }}
                      >
                        <KeyRound size={12} />
                        <span>Change Password</span>
                      </button>
                      <button
                        type="button"
                        class="btn-vault-action btn-lock"
                        onclick={() => { void handleLockVault(); }}
                      >
                        <Lock size={12} />
                        <span>Lock Vault</span>
                      </button>
                    {:else}
                      <button
                        type="button"
                        class="btn-vault-action btn-unlock"
                        onclick={handleUnlockVault}
                      >
                        <Unlock size={12} />
                        <span>Unlock Vault...</span>
                      </button>
                    {/if}
                  </div>
                </div>
              </div>
            {/if}
          </div>
        {/if}
      </div>
    </div>

    <!-- Modal Footer -->
    <div class="modal-actions">
      <button
        type="button"
        class="btn btn-subtle"
        title="Reset all settings to factory defaults"
        onclick={handleRestoreDefaults}
      >
        <RotateCcw size={13} />
        <span>Restore Defaults</span>
      </button>

      <div class="modal-actions-right">
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

  .geist-input,
  .geist-select {
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

  .geist-select {
    appearance: none;
    background-image: url("data:image/svg+xml;charset=UTF-8,%3csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%23888888' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3e%3cpolyline points='6 9 12 15 18 9'%3e%3c/polyline%3e%3c/svg%3e");
    background-repeat: no-repeat;
    background-position: right 10px center;
    background-size: 12px;
    padding-right: 28px;
    cursor: pointer;
  }

  .geist-select option {
    background: #111111;
    color: #ededed;
    padding: 6px 10px;
  }

  .geist-input:focus,
  .geist-select:focus {
    border-color: #ededed;
  }

  .custom-font-box {
    margin-top: 8px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    animation: fadeIn 0.15s ease-out;
  }

  .sub-label {
    font-size: 9.5px;
    color: #777777;
    margin-top: 4px;
    letter-spacing: 0.04em;
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
    justify-content: space-between;
    gap: 10px;
    padding: 12px 18px;
    background: #050505;
    border-top: 1px solid #262626;
  }

  .modal-actions-right {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .btn-subtle {
    background: transparent;
    color: #707070;
    border: 1px solid transparent;
    padding: 6px 10px;
  }

  .btn-subtle:hover {
    color: #ededed;
    background: #141414;
    border-color: #262626;
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

  /* Vault & Security Management */
  .vault-notice-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-radius: 6px;
    font-size: 11.5px;
    margin-bottom: 12px;
  }

  .vault-notice-banner.success {
    background: rgba(34, 197, 94, 0.1);
    border: 1px solid rgba(34, 197, 94, 0.3);
    color: #4ade80;
  }

  .vault-notice-banner.error {
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.3);
    color: #f87171;
  }

  .env-mode-card {
    background: #080808;
    border: 1px solid #222222;
    border-radius: 8px;
    padding: 12px 14px;
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }

  .env-icon-box {
    width: 34px;
    height: 34px;
    border-radius: 6px;
    background: #141414;
    border: 1px solid #2a2a2a;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    color: #ededed;
  }

  .env-info {
    flex: 1;
    min-width: 0;
  }

  .env-title-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 4px;
  }

  .env-title {
    font-size: 12.5px;
    font-weight: 600;
    color: #ffffff;
  }

  .env-badge {
    font-size: 9.5px;
    font-family: var(--font-mono, monospace);
    padding: 2px 7px;
    border-radius: 10px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .badge-portable {
    background: rgba(59, 130, 246, 0.12);
    border: 1px solid rgba(59, 130, 246, 0.35);
    color: #60a5fa;
  }

  .badge-installed {
    background: rgba(168, 85, 247, 0.12);
    border: 1px solid rgba(168, 85, 247, 0.35);
    color: #c084fc;
  }

  .env-desc {
    margin: 0;
    font-size: 11px;
    color: #777777;
    line-height: 1.45;
  }

  .env-desc code {
    background: #141414;
    padding: 1px 4px;
    border-radius: 3px;
    color: #ccc;
    font-size: 10.5px;
  }

  .storage-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 6px;
  }

  .section-label-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .migrating-pill {
    font-size: 10px;
    font-family: var(--font-mono, monospace);
    color: #60a5fa;
    animation: pulse 1.5s infinite;
  }

  .backend-options-grid {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .backend-card {
    background: #080808;
    border: 1px solid #222222;
    border-radius: 8px;
    padding: 11px 13px;
    cursor: pointer;
    transition: all 0.15s ease;
    outline: none;
  }

  .backend-card:hover:not(.disabled) {
    border-color: #3f3f3f;
    background: #0d0d0d;
  }

  .backend-card.selected {
    border-color: #ffffff;
    background: #121212;
  }

  .backend-card.disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .backend-card-head {
    display: flex;
    align-items: center;
    gap: 9px;
    margin-bottom: 4px;
  }

  .radio-indicator {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 1px solid #444444;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .backend-card.selected .radio-indicator {
    border-color: #ffffff;
  }

  .radio-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #ffffff;
  }

  .backend-name {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 12px;
    font-weight: 600;
    color: #ededed;
  }

  .rec-badge {
    font-size: 9px;
    font-family: var(--font-mono, monospace);
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid #333333;
    color: #aaaaaa;
    padding: 1px 5px;
    border-radius: 4px;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .backend-desc {
    margin: 0;
    padding-left: 23px;
    font-size: 11px;
    color: #777777;
    line-height: 1.4;
  }

  .backend-desc strong {
    color: #cccccc;
  }

  .backend-desc code {
    background: #141414;
    padding: 1px 4px;
    border-radius: 3px;
    color: #ccc;
    font-size: 10.5px;
  }

  .backend-footer {
    padding-left: 23px;
    margin-top: 5px;
  }

  .effective-text {
    font-size: 10.5px;
    color: #888888;
  }

  .effective-text strong {
    color: #ffffff;
  }

  .vault-management-panel {
    background: #080808;
    border: 1px solid #222222;
    border-radius: 8px;
    padding: 10px 14px;
    margin-top: 4px;
  }

  .vault-panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .vault-status-indicator {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .status-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }

  .dot-green {
    background: #22c55e;
    box-shadow: 0 0 6px rgba(34, 197, 94, 0.6);
  }

  .dot-amber {
    background: #f59e0b;
    box-shadow: 0 0 6px rgba(245, 158, 11, 0.6);
  }

  .dot-gray {
    background: #6b7280;
  }

  .vault-status-label {
    font-size: 11.5px;
    font-weight: 500;
    color: #ededed;
  }

  .vault-action-buttons {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .btn-vault-action {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    background: #141414;
    border: 1px solid #2a2a2a;
    color: #ededed;
    padding: 4px 10px;
    border-radius: 5px;
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .btn-vault-action:hover {
    background: #202020;
    border-color: #444444;
  }

  .btn-lock:hover {
    border-color: rgba(239, 68, 68, 0.5);
    color: #f87171;
  }

  .btn-unlock {
    background: #ededed;
    color: #000000;
    border-color: #ededed;
  }

  .btn-unlock:hover {
    background: #ffffff;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.5; }
  }
</style>
