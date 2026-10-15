<script lang="ts">
  import { appState } from '$lib/stores/appState.svelte';
  import Sidebar from '$lib/components/Sidebar.svelte';
  import TabBar from '$lib/components/TabBar.svelte';
  import ConnectionModal from '$lib/components/ConnectionModal.svelte';
  import CredentialsModal from '$lib/components/CredentialsModal.svelte';
  import SettingsModal from '$lib/components/SettingsModal.svelte';
  import ImportModal from '$lib/components/ImportModal.svelte';
  import DialogModal from '$lib/components/DialogModal.svelte';
  import TerminalView from '$lib/components/TerminalView.svelte';
  import SftpView from '$lib/components/SftpView.svelte';
  import RdpView from '$lib/components/RdpView.svelte';
  import TitleBar from '$lib/components/TitleBar.svelte';
  import SessionNotesDrawer from '$lib/components/SessionNotesDrawer.svelte';
  import VaultUnlockModal from '$lib/components/VaultUnlockModal.svelte';
  import {
    Terminal,
    ArrowLeftRight,
    Monitor,
    Plus,
    Cpu,
    ArrowRight,
    Shield,
    HardDrive,
    Layers
  } from '@lucide/svelte';

  let activeSession = $derived(
    appState.tabs.find((t) => t.id === appState.activeTabId) || null
  );

  function handleKeyDown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.altKey && (e.key === 'n' || e.key === 'N')) {
      e.preventDefault();
      appState.toggleNotesDrawer();
    } else if (e.key === 'Escape' && appState.isNotesDrawerOpen) {
      appState.isNotesDrawerOpen = false;
    }
  }
</script>
<svelte:window onbeforeunload={() => appState.closeAllTabs()} onkeydown={handleKeyDown} />

<div class="window-shell">
  <!-- Native App Title Bar & Window Controls -->
  <TitleBar />

  <div class="app-viewport">
  <!-- Sidebar Navigation -->
  <Sidebar />

  <!-- Main View Area -->
  <main class="main-panel">
    <!-- Session Tabs -->
    <TabBar />

    <!-- Content Workspace -->
    <div class="workspace-panel">
      {#if appState.tabs.length > 0}
        {#each appState.tabs as tab, i (tab.id)}
          <div class="tab-pane {tab.id === appState.activeTabId ? 'active-pane' : ''}">
            {#if tab.type === 'ssh' || tab.type === 'local_terminal'}
              <TerminalView bind:session={appState.tabs[i]} />
            {:else if tab.type === 'sftp'}
              <SftpView session={tab} />
            {:else if tab.type === 'rdp'}
              <RdpView bind:session={appState.tabs[i]} />
            {/if}
          </div>
        {/each}

        <!-- In-Session Notes Drawer -->
        {#if appState.isNotesDrawerOpen}
          <SessionNotesDrawer />
        {/if}
      {:else}
        <!-- Geist Hero & Dashboard -->
        <div class="hero-container">
          <!-- Multi-stop mesh gradient strictly confined to hero backdrop -->
          <div class="hero-mesh"></div>

          <div class="hero-content">
            <img src="/rustius_logo.svg" alt="rustius" class="hero-logo" width="84" height="84" />
            <span class="mono-eyebrow">PORTABLE DESKTOP CLIENT</span>
            <h1 class="hero-heading">Engineered for speed.<br />Built with Rust.</h1>
            <p class="hero-body">
              A single, portable executable for SSH, SFTP, and RDP. Zero Electron bloat.
            </p>

            <!-- Pill CTA -->
            <div class="hero-actions">
              <button
                class="btn btn-primary btn-pill"
                onclick={() => {
                  appState.isConnectionModalOpen = true;
                }}
              >
                <span>New Connection</span>
                <ArrowRight size={14} />
              </button>
              <button
                class="btn btn-secondary btn-pill"
                onclick={() => {
                  appState.openTab({
                    title: 'Local Shell',
                    type: 'local_terminal',
                    status: 'connected',
                    badge: 'POWERSHELL',
                  });
                }}
              >
                <span>Open Local Shell</span>
              </button>
            </div>

            <!-- Hairline Feature Grid -->
            <div class="cards-grid">
              <div
                class="feature-card"
                role="button"
                tabindex="0"
                onclick={() => (appState.isConnectionModalOpen = true)}
                onkeydown={(e) => e.key === 'Enter' && (appState.isConnectionModalOpen = true)}
              >
                <div class="card-head">
                  <Terminal size={16} />
                  <span class="card-type">SSH</span>
                </div>
                <h3 class="card-title">Terminal Sessions</h3>
                <p class="card-desc">xterm.js with support for legacy ciphers and key exchanges.</p>
              </div>

              <div
                class="feature-card"
                role="button"
                tabindex="0"
                onclick={() => (appState.isConnectionModalOpen = true)}
                onkeydown={(e) => e.key === 'Enter' && (appState.isConnectionModalOpen = true)}
              >
                <div class="card-head">
                  <ArrowLeftRight size={16} />
                  <span class="card-type">SFTP</span>
                </div>
                <h3 class="card-title">Dual-Pane Browser</h3>
                <p class="card-desc">Fast local and remote directory file manager with async transfers.</p>
              </div>

              <div
                class="feature-card"
                role="button"
                tabindex="0"
                onclick={() => (appState.isConnectionModalOpen = true)}
                onkeydown={(e) => e.key === 'Enter' && (appState.isConnectionModalOpen = true)}
              >
                <div class="card-head">
                  <Monitor size={16} />
                  <span class="card-type">RDP</span>
                </div>
                <h3 class="card-title">Remote Desktop</h3>
                <p class="card-desc">Hardware-accelerated Remote Desktop powered by FreeRDP.</p>
              </div>

              <div
                class="feature-card"
                role="button"
                tabindex="0"
                onclick={() => {
                  appState.openTab({
                    title: 'Local Shell',
                    type: 'local_terminal',
                    status: 'connected',
                    badge: 'POWERSHELL',
                  });
                }}
                onkeydown={(e) => e.key === 'Enter' && {}}
              >
                <div class="card-head">
                  <Cpu size={16} />
                  <span class="card-type">PTY</span>
                </div>
                <h3 class="card-title">Local Shell</h3>
                <p class="card-desc">Instant pseudo-terminal sessions using cross-platform portable-pty.</p>
              </div>
            </div>

            <!-- Technical Spec Footprint -->
            <div class="specs-bar">
              <div class="spec-node">
                <HardDrive size={13} />
                <span>100% Portable · Config next to .exe</span>
              </div>
              <div class="spec-node">
                <Shield size={13} />
                <span>Memory Safe · Pure Rust</span>
              </div>
              <div class="spec-node">
                <Layers size={13} />
                <span>~11MB Executable · Native WebView</span>
              </div>
            </div>
          </div>
        </div>
      {/if}
    </div>
  </main>
  </div>

  <!-- Connection Configuration Modal -->
  {#if appState.isConnectionModalOpen}
    <ConnectionModal />
  {/if}

  <!-- Application Settings Modal -->
  {#if appState.isSettingsModalOpen}
    <SettingsModal />
  {/if}

  <!-- Credential Profiles Modal -->
  {#if appState.isCredentialsModalOpen}
    <CredentialsModal />
  {/if}

  <!-- Session Migration / Import Modal -->
  {#if appState.isImportModalOpen}
    <ImportModal />
  {/if}

  <!-- Master Password Vault Unlock Modal -->
  <VaultUnlockModal />

  <!-- Desktop Native Dialog Modal (Prompt, Confirm, Alert) -->
  <DialogModal />
</div>

<style>
  .window-shell {
    display: flex;
    flex-direction: column;
    width: 100vw;
    height: 100vh;
    background: var(--canvas);
    overflow: hidden;
  }

  .app-viewport {
    flex: 1;
    display: flex;
    width: 100%;
    min-height: 0;
    background: var(--canvas);
    overflow: hidden;
  }

  .main-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    overflow: hidden;
  }

  .workspace-panel {
    flex: 1;
    position: relative;
    overflow: hidden;
    background: var(--canvas);
  }

  .tab-pane {
    visibility: hidden;
    pointer-events: none;
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    z-index: 0;
    opacity: 0;
  }

  .tab-pane.active-pane {
    visibility: visible;
    pointer-events: auto;
    z-index: 1;
    opacity: 1;
  }

  .hero-container {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: clamp(12px, 2.5vh, 32px);
    position: relative;
    overflow-y: auto;
  }

  /* Multi-stop mesh gradient bloom behind headline (Vercel signature) */
  .hero-mesh {
    position: absolute;
    top: 5%;
    left: 50%;
    transform: translateX(-50%);
    width: min(600px, 80vw);
    height: clamp(120px, 20vh, 220px);
    background: radial-gradient(ellipse at center, rgba(0, 124, 240, 0.12) 0%, rgba(121, 40, 202, 0.12) 40%, rgba(255, 0, 128, 0.08) 70%, transparent 100%);
    filter: blur(50px);
    pointer-events: none;
    z-index: 0;
  }

  .hero-content {
    margin: auto;
    max-width: 740px;
    width: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    z-index: 1;
    gap: clamp(8px, 1.8vh, 16px);
  }

  .hero-logo {
    height: clamp(38px, 6.5vh, 80px);
    width: auto;
    object-fit: contain;
    margin-bottom: clamp(0px, 0.5vh, 4px);
    filter: drop-shadow(0 6px 20px rgba(240, 67, 36, 0.2));
  }

  .mono-eyebrow {
    font-size: clamp(9.5px, 1.2vh, 11px);
    letter-spacing: 0.08em;
  }

  .hero-heading {
    margin: 0;
    font-size: clamp(22px, 3.8vh, 40px);
    font-weight: 600;
    line-height: 1.15;
    letter-spacing: clamp(-1.8px, -0.2vw, -0.8px);
    color: var(--ink);
  }

  .hero-body {
    margin: 0;
    font-size: clamp(12px, 1.5vh, 15px);
    color: var(--body);
    max-width: 480px;
    line-height: 1.45;
  }

  .hero-actions {
    display: flex;
    align-items: center;
    gap: clamp(6px, 1vw, 10px);
    margin-top: clamp(2px, 0.8vh, 8px);
    margin-bottom: clamp(6px, 1.8vh, 20px);
  }

  .cards-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: clamp(6px, 1.2vh, 12px);
    width: 100%;
    margin-bottom: clamp(6px, 1.5vh, 16px);
  }

  .feature-card {
    background: var(--canvas-subtle);
    border: 1px solid var(--hairline);
    border-radius: var(--radius-md);
    padding: clamp(8px, 1.5vh, 18px);
    text-align: left;
    cursor: pointer;
    transition: background 0.12s ease, border-color 0.12s ease;
    display: flex;
    flex-direction: column;
    gap: clamp(2px, 0.5vh, 6px);
  }

  .feature-card:hover {
    background: var(--canvas-hover);
    border-color: var(--hairline-hover);
  }

  .card-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    color: var(--body);
    margin-bottom: clamp(2px, 0.4vh, 4px);
  }

  .card-type {
    font-family: var(--font-mono);
    font-size: clamp(9.5px, 1.1vh, 11px);
    color: var(--mute);
  }

  .card-title {
    margin: 0;
    font-size: clamp(12px, 1.4vh, 14px);
    font-weight: 500;
    letter-spacing: -0.2px;
    color: var(--ink);
  }

  .card-desc {
    margin: 0;
    font-size: clamp(10.5px, 1.2vh, 12px);
    color: var(--body);
    line-height: 1.35;
  }

  .specs-bar {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: clamp(8px, 1.6vw, 16px);
    border-top: 1px solid var(--hairline-soft);
    padding-top: clamp(8px, 1.4vh, 14px);
    width: 100%;
  }

  .spec-node {
    display: flex;
    align-items: center;
    gap: clamp(4px, 0.6vw, 6px);
    font-family: var(--font-mono);
    font-size: clamp(9.5px, 1.1vh, 11px);
    color: var(--mute);
  }
</style>
