<script lang="ts">
  import type { TabSession } from '../types';
  import { Monitor, Maximize2 } from '@lucide/svelte';

  let { session }: { session: TabSession } = $props();
</script>

<div class="rdp-frame">
  <!-- Minimalist Status Bar -->
  <div class="rdp-header">
    <div class="header-left">
      <Monitor size={14} class="rdp-icon" />
      <span class="session-name">{session.title}</span>
      <span class="online-dot"></span>
      <span class="mono-eyebrow">1920×1080 · 32-BIT</span>
    </div>
    <div class="header-right">
      <button class="btn btn-secondary btn-compact" title="Toggle Fullscreen">
        <Maximize2 size={12} />
        <span>Fullscreen</span>
      </button>
    </div>
  </div>

  <!-- RDP Canvas Stage -->
  <div class="rdp-viewport">
    <div class="rdp-display-box">
      <div class="display-placeholder">
        <Monitor size={28} class="rdp-big-icon" />
        <span class="display-notice">IronRDP Protocol Stream</span>
        <span class="mono-eyebrow">{session.badge || 'server:3389'}</span>
      </div>
    </div>
  </div>
</div>

<style>
  .rdp-frame {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--canvas);
  }

  .rdp-header {
    height: 38px;
    padding: 0 14px;
    background: var(--canvas);
    border-bottom: 1px solid var(--hairline);
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .session-name {
    font-size: 12px;
    font-weight: 500;
    color: var(--ink);
  }

  .online-dot {
    width: 5px;
    height: 5px;
    border-radius: var(--radius-full);
    background: var(--accent-green);
  }

  .btn-compact {
    height: 26px;
    font-size: 11px;
    padding: 0 8px;
  }

  .rdp-viewport {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: #000000;
  }

  .rdp-display-box {
    width: 90%;
    max-width: 1100px;
    height: 80%;
    background: #0a0a0a;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-md);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .display-placeholder {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }

  .display-notice {
    font-size: 13px;
    font-weight: 500;
    color: var(--body);
  }

  :global(.rdp-icon) {
    color: var(--mute);
  }

  :global(.rdp-big-icon) {
    color: var(--mute);
    opacity: 0.5;
  }
</style>
