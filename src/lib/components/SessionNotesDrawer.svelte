<script lang="ts">
  import { appState } from '../stores/appState.svelte';
  import { marked } from 'marked';
  import DOMPurify from 'dompurify';
  import {
    X,
    Pencil,
    FileText,
    BookOpen,
    Copy,
    Check
  } from '@lucide/svelte';

  let activeConn = $derived(appState.getActiveConnection());
  let copied = $state(false);

  let renderedHtml = $derived.by(() => {
    if (!activeConn?.notes?.trim()) return '';
    try {
      const raw = marked.parse(activeConn.notes, {
        gfm: true,
        breaks: true,
      }) as string;
      return DOMPurify.sanitize(raw, {
        USE_PROFILES: { html: true },
        FORBID_TAGS: ['style', 'form', 'input', 'button', 'template', 'slot'],
      });
    } catch (e) {
      console.error('Failed to parse Markdown:', e);
      return `<p class="error-msg">Failed to parse notes markdown</p>`;
    }
  });

  function closeDrawer() {
    appState.isNotesDrawerOpen = false;
  }

  function handleEdit() {
    if (activeConn) {
      appState.editingConnection = activeConn;
      appState.isConnectionModalOpen = true;
    }
  }

  async function copyAllNotes() {
    if (!activeConn?.notes) return;
    try {
      await navigator.clipboard.writeText(activeConn.notes);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch (err) {
      console.warn('Clipboard write failed:', err);
    }
  }

  function copyOnCodeClick(node: HTMLElement) {
    const handler = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      const pre = target.closest('pre');
      if (pre && pre.textContent) {
        navigator.clipboard.writeText(pre.textContent.trim());
        const originalBg = pre.style.borderColor;
        pre.style.borderColor = 'var(--accent)';
        setTimeout(() => {
          pre.style.borderColor = originalBg;
        }, 800);
      }
    };
    node.addEventListener('click', handler);
    return {
      destroy() {
        node.removeEventListener('click', handler);
      }
    };
  }
</script>

<aside class="notes-drawer" aria-label="Connection Notes">
  <!-- Drawer Header -->
  <div class="drawer-header">
    <div class="header-left">
      <div class="title-row">
        <FileText size={14} class="header-icon" />
        <h2 class="conn-name">{activeConn ? activeConn.name : 'Connection Notes'}</h2>
        {#if activeConn?.environment}
          <span class="env-pill">{activeConn.environment}</span>
        {/if}
      </div>
      <div class="meta-row">
        {#if activeConn}
          <span class="conn-endpoint">{activeConn.host}:{activeConn.port}</span>
        {/if}
        {#if activeConn?.description}
          <span class="conn-desc" title={activeConn.description}>· {activeConn.description}</span>
        {/if}
      </div>
    </div>

    <div class="header-actions">
      {#if activeConn?.notes}
        <button
          class="btn-icon"
          title="Copy raw markdown to clipboard"
          onclick={copyAllNotes}
        >
          {#if copied}
            <Check size={13} class="text-success" />
          {:else}
            <Copy size={13} />
          {/if}
        </button>
      {/if}
      {#if activeConn}
        <button
          class="btn-icon"
          title="Edit connection notes"
          onclick={handleEdit}
        >
          <Pencil size={13} />
        </button>
      {/if}
      <button
        class="btn-icon"
        title="Close notes drawer (Esc)"
        onclick={closeDrawer}
      >
        <X size={14} />
      </button>
    </div>
  </div>

  <!-- Drawer Body -->
  <div class="drawer-body" use:copyOnCodeClick>
    {#if activeConn && activeConn.notes && activeConn.notes.trim()}
      <div class="markdown-body">
        {@html renderedHtml}
      </div>
    {:else}
      <div class="empty-notes-view">
        <div class="empty-icon-wrap">
          <BookOpen size={28} />
        </div>
        <h3 class="empty-title">No notes attached</h3>
        <p class="empty-desc">
          Add notes, documentation, checklists, or command snippets to this connection for instant access.
        </p>
        <button class="btn btn-secondary btn-sm" onclick={handleEdit}>
          <Pencil size={12} />
          <span>Add Notes</span>
        </button>
      </div>
    {/if}
  </div>

  <!-- Drawer Footer Info -->
  <div class="drawer-footer">
    <span class="footer-hint">Tip: Click any code block to copy snippet</span>
    <kbd class="esc-kbd">Esc</kbd>
  </div>
</aside>

<style>
  .notes-drawer {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    width: clamp(280px, 26vw, 380px);
    max-width: 90vw;
    background: var(--canvas-elevated);
    border-left: 1px solid var(--hairline);
    display: flex;
    flex-direction: column;
    z-index: 50;
    box-shadow: -8px 0 32px rgba(0, 0, 0, 0.45);
    animation: slideInRight 0.18s cubic-bezier(0.16, 1, 0.3, 1);
  }

  @keyframes slideInRight {
    from {
      transform: translateX(100%);
      opacity: 0.5;
    }
    to {
      transform: translateX(0);
      opacity: 1;
    }
  }

  .drawer-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 8px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--hairline);
    background: var(--canvas-subtle);
  }

  .header-left {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
    flex: 1;
  }

  .title-row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  :global(.header-icon) {
    color: var(--accent);
    flex-shrink: 0;
  }

  .conn-name {
    font-size: 13px;
    font-weight: 600;
    color: var(--ink);
    letter-spacing: -0.3px;
    margin: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .env-pill {
    font-family: var(--font-mono);
    font-size: 9px;
    font-weight: 600;
    padding: 1px 5px;
    border-radius: 100px;
    background: rgba(255, 255, 255, 0.08);
    color: var(--ink-secondary);
    border: 1px solid var(--hairline);
    flex-shrink: 0;
    text-transform: uppercase;
  }

  .meta-row {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--mute);
    min-width: 0;
  }

  .conn-endpoint {
    font-family: var(--font-mono);
    flex-shrink: 0;
  }

  .conn-desc {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--ink-secondary);
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }

  .btn-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    color: var(--mute);
    cursor: pointer;
    transition: all 0.12s ease;
  }

  .btn-icon:hover {
    background: var(--canvas-hover);
    color: var(--ink);
    border-color: var(--hairline);
  }

  .drawer-body {
    flex: 1;
    overflow-y: auto;
    padding: 16px;
  }

  /* Empty state */
  .empty-notes-view {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    height: 100%;
    padding: 24px;
    gap: 8px;
  }

  .empty-icon-wrap {
    width: 52px;
    height: 52px;
    border-radius: 50%;
    background: var(--canvas-subtle);
    border: 1px solid var(--hairline);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--mute);
    margin-bottom: 4px;
  }

  .empty-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--ink);
    margin: 0;
  }

  .empty-desc {
    font-size: 12px;
    line-height: 1.5;
    color: var(--mute);
    margin: 0 0 12px 0;
    max-width: 240px;
  }

  /* Markdown Body Rendering */
  .markdown-body {
    font-size: 12px;
    line-height: 1.6;
    color: var(--ink-secondary);
    user-select: text;
  }

  :global(.markdown-body h1) {
    font-size: 16px;
    font-weight: 600;
    color: var(--ink);
    margin: 0 0 12px 0;
    padding-bottom: 6px;
    border-bottom: 1px solid var(--hairline);
    letter-spacing: -0.3px;
  }

  :global(.markdown-body h2) {
    font-size: 14px;
    font-weight: 600;
    color: var(--ink);
    margin: 16px 0 8px 0;
    letter-spacing: -0.2px;
  }

  :global(.markdown-body h3) {
    font-size: 12px;
    font-weight: 600;
    color: var(--ink);
    margin: 14px 0 6px 0;
    text-transform: uppercase;
    letter-spacing: 0.4px;
  }

  :global(.markdown-body p) {
    margin: 0 0 10px 0;
  }

  :global(.markdown-body ul),
  :global(.markdown-body ol) {
    margin: 0 0 10px 0;
    padding-left: 20px;
  }

  :global(.markdown-body li) {
    margin-bottom: 4px;
  }

  :global(.markdown-body li input[type='checkbox']) {
    margin-right: 6px;
    vertical-align: middle;
  }

  :global(.markdown-body pre) {
    background: #000000;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-sm);
    padding: 10px 12px;
    margin: 8px 0 12px 0;
    overflow-x: auto;
    font-family: var(--font-mono);
    font-size: 11px;
    line-height: 1.5;
    color: #f1f1f1;
    cursor: copy;
    transition: border-color 0.15s ease;
  }

  :global(.markdown-body pre:hover) {
    border-color: var(--hairline-hover);
  }

  :global(.markdown-body code) {
    font-family: var(--font-mono);
    font-size: 11px;
    background: rgba(255, 255, 255, 0.08);
    padding: 2px 5px;
    border-radius: 3px;
    color: #e5e5e5;
  }

  :global(.markdown-body pre code) {
    background: transparent;
    padding: 0;
    border-radius: 0;
    color: inherit;
  }

  :global(.markdown-body blockquote) {
    border-left: 3px solid var(--accent);
    padding: 4px 0 4px 12px;
    margin: 8px 0 12px 0;
    color: var(--mute);
    font-style: italic;
    background: var(--canvas-subtle);
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
  }

  :global(.markdown-body a) {
    color: var(--accent);
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  :global(.markdown-body table) {
    width: 100%;
    border-collapse: collapse;
    margin: 10px 0;
    font-size: 11px;
  }

  :global(.markdown-body th),
  :global(.markdown-body td) {
    border: 1px solid var(--hairline);
    padding: 6px 8px;
    text-align: left;
  }

  :global(.markdown-body th) {
    background: var(--canvas-subtle);
    color: var(--ink);
    font-weight: 600;
  }

  :global(.markdown-body hr) {
    border: none;
    height: 1px;
    background: var(--hairline);
    margin: 16px 0;
  }

  .drawer-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 14px;
    border-top: 1px solid var(--hairline);
    background: var(--canvas-subtle);
    font-size: 10px;
    color: var(--mute);
  }

  .esc-kbd {
    font-family: var(--font-mono);
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid var(--hairline);
    border-radius: 3px;
    padding: 1px 4px;
  }

  :global(.text-success) {
    color: #00e599;
  }
</style>
