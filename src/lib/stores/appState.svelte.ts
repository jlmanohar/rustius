import type {
  RustiusConfig,
  TabSession,
  Connection,
  TreeNode,
  Folder,
  AppSettings,
  ImportSummary,
  CredentialProfile,
} from '../types';
import { invoke } from '@tauri-apps/api/core';

export class AppStore {
  config = $state<RustiusConfig>({
    version: '1.0.0',
    app_settings: {
      theme: 'dark',
      font_family: 'JetBrains Mono',
      font_size: 14,
      default_terminal_type: 'xterm-256color',
      show_sidebar: true,
      sidebar_width: 280,
    },
    credentials: [],
    tree: [],
  });

  tabs = $state<TabSession[]>([]);
  activeTabId = $state<string | null>(null);
  selectedEnvironment = $state<string>('all');
  searchQuery = $state<string>('');
  
  // Modal states
  isConnectionModalOpen = $state<boolean>(false);
  isFolderModalOpen = $state<boolean>(false);
  isSettingsModalOpen = $state<boolean>(false);
  isImportModalOpen = $state<boolean>(false);
  isCredentialsModalOpen = $state<boolean>(false);
  isNotesDrawerOpen = $state<boolean>(false);
  editingConnection = $state<Connection | null>(null);
  editingCredential = $state<CredentialProfile | null>(null);
  targetFolderId = $state<string | null>(null);

  constructor() {
    this.loadConfig();
  }

  async loadConfig() {
    try {
      const cfg = await invoke<RustiusConfig>('get_config');
      if (cfg) {
        this.config = cfg;
      }
    } catch (e) {
      console.warn('Failed to load config from backend (might be in web preview mode):', e);
      // Populate demo connection if empty in preview
      if (this.config.tree.length === 0) {
        this.config.tree = [
          {
            node_type: 'folder',
            id: 'folder-servers',
            name: 'Cloud Infrastructure',
            expanded: true,
            children: [
              {
                node_type: 'connection',
                id: 'conn-demo-1',
                name: 'Production Gateway',
                conn_type: 'ssh',
                host: 'prod.example.com',
                port: 22,
                auth: { type: 'password', username: 'admin' },
                environment: 'Production',
                tags: ['aws', 'us-east-1'],
                created_at: new Date().toISOString(),
              },
              {
                node_type: 'connection',
                id: 'conn-demo-2',
                name: 'File Storage SFTP',
                conn_type: 'sftp',
                host: 'sftp.example.com',
                port: 22,
                auth: { type: 'password', username: 'deploy' },
                environment: 'Production',
                tags: ['backup', 'assets'],
                created_at: new Date().toISOString(),
              },
            ],
          },
          {
            node_type: 'connection',
            id: 'conn-demo-3',
            name: 'Windows Jump Host',
            conn_type: 'rdp',
            host: 'rdp.example.com',
            port: 3389,
            auth: { type: 'password', username: 'Administrator' },
            environment: 'Staging',
            tags: ['windows', 'gui'],
            created_at: new Date().toISOString(),
          },
        ];
      }
    }
  }

  async addConnection(conn: Connection, parentFolderId?: string | null) {
    try {
      const assignedId = await invoke<string>('add_connection', {
        connection: conn,
        parentFolderId: parentFolderId || null,
      });
      if (assignedId) {
        conn.id = assignedId;
      }
      await this.loadConfig();
    } catch (err) {
      console.error('Failed to add connection via Tauri:', err);
      const isTauri = typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
      if (isTauri) {
        throw err;
      }
      // Fallback local update for preview
      if (parentFolderId) {
        const folder = this.findFolder(this.config.tree, parentFolderId);
        if (folder) {
          folder.children.push({ node_type: 'connection', ...conn });
        }
      } else {
        this.config.tree.push({ node_type: 'connection', ...conn });
      }
    }
  }

  async addFolder(name: string, parentFolderId?: string | null) {
    try {
      await invoke('add_folder', {
        name,
        parentFolderId: parentFolderId || null,
      });
      await this.loadConfig();
    } catch (err) {
      console.error('Failed to add folder via Tauri:', err);
      const newFolder: Folder = {
        id: 'folder-' + Date.now(),
        name,
        parent_id: parentFolderId || undefined,
        children: [],
        expanded: true,
      };
      if (parentFolderId) {
        const parent = this.findFolder(this.config.tree, parentFolderId);
        if (parent) parent.children.push({ node_type: 'folder', ...newFolder });
      } else {
        this.config.tree.push({ node_type: 'folder', ...newFolder });
      }
    }
  }

  async removeNode(id: string) {
    try {
      await invoke('remove_node', { nodeId: id });
      await this.loadConfig();
    } catch (err) {
      console.error('Failed to remove node:', err);
      this.removeNodeLocally(this.config.tree, id);
    }
  }

  async addCredential(cred: CredentialProfile): Promise<string> {
    try {
      const id = await invoke<string>('add_credential', { credential: cred });
      await this.loadConfig();
      return id;
    } catch (err) {
      console.error('Failed to add credential:', err);
      if (!this.config.credentials) {
        this.config.credentials = [];
      }
      this.config.credentials.push(cred);
      return cred.id;
    }
  }

  async updateCredential(cred: CredentialProfile): Promise<void> {
    try {
      await invoke('update_credential', { credential: cred });
      await this.loadConfig();
    } catch (err) {
      console.error('Failed to update credential:', err);
      if (this.config.credentials) {
        const idx = this.config.credentials.findIndex((c) => c.id === cred.id);
        if (idx !== -1) {
          this.config.credentials[idx] = cred;
        }
      }
    }
  }

  async deleteCredential(id: string): Promise<void> {
    try {
      await invoke('delete_credential', { id });
      await this.loadConfig();
    } catch (err) {
      console.error('Failed to delete credential:', err);
      if (this.config.credentials) {
        this.config.credentials = this.config.credentials.filter((c) => c.id !== id);
      }
    }
  }

  private removeNodeLocally(nodes: TreeNode[], id: string): boolean {
    const idx = nodes.findIndex((n) => n.id === id);
    if (idx !== -1) {
      nodes.splice(idx, 1);
      return true;
    }
    for (const n of nodes) {
      if (n.node_type === 'folder' && this.removeNodeLocally(n.children, id)) {
        return true;
      }
    }
    return false;
  }

  private findFolder(nodes: TreeNode[], folderId: string): Folder | null {
    for (const node of nodes) {
      if (node.node_type === 'folder') {
        if (node.id === folderId) return node;
        const sub = this.findFolder(node.children, folderId);
        if (sub) return sub;
      }
    }
    return null;
  }

  findConnection(id?: string | null): Connection | null {
    if (!id) return null;
    const search = (nodes: TreeNode[]): Connection | null => {
      for (const node of nodes) {
        if (node.node_type === 'connection' && node.id === id) {
          return node;
        }
        if (node.node_type === 'folder' && Array.isArray(node.children)) {
          const found = search(node.children);
          if (found) return found;
        }
      }
      return null;
    };
    return search(this.config.tree);
  }

  getActiveConnection(): Connection | null {
    if (!this.activeTabId) return null;
    const tab = this.tabs.find((t) => t.id === this.activeTabId);
    if (!tab || !tab.connectionId) return null;
    return this.findConnection(tab.connectionId);
  }

  toggleNotesDrawer() {
    this.isNotesDrawerOpen = !this.isNotesDrawerOpen;
  }

  openTab(session: Omit<TabSession, 'id'>) {
    const id = 'tab-' + Date.now();
    const newTab: TabSession = { id, ...session };
    this.tabs.push(newTab);
    this.activeTabId = id;
  }

  closeTab(tabId: string) {
    const idx = this.tabs.findIndex((t) => t.id === tabId);
    if (idx !== -1) {
      const tab = this.tabs[idx];
      this.tabs.splice(idx, 1);
      if (this.activeTabId === tabId) {
        this.activeTabId = this.tabs.length > 0 ? this.tabs[Math.max(0, idx - 1)].id : null;
      }

      // Explicitly disconnect backend sessions
      this.terminateSession(tab);
    }
  }

  closeAllTabs() {
    const tabsToClose = [...this.tabs];
    this.tabs = [];
    this.activeTabId = null;
    for (const tab of tabsToClose) {
      this.terminateSession(tab);
    }
  }

  closeOtherTabs(keepTabId: string) {
    const tabsToClose = this.tabs.filter((t) => t.id !== keepTabId);
    this.tabs = this.tabs.filter((t) => t.id === keepTabId);
    this.activeTabId = keepTabId;
    for (const tab of tabsToClose) {
      this.terminateSession(tab);
    }
  }

  closeDisconnectedTabs() {
    const disconnected = this.tabs.filter((t) => t.status === 'disconnected');
    this.tabs = this.tabs.filter((t) => t.status !== 'disconnected');
    if (this.activeTabId && !this.tabs.some((t) => t.id === this.activeTabId)) {
      this.activeTabId = this.tabs.length > 0 ? this.tabs[this.tabs.length - 1].id : null;
    }
    for (const tab of disconnected) {
      this.terminateSession(tab);
    }
  }

  private terminateSession(tab: TabSession) {
    if (tab.type === 'ssh') {
      invoke('ssh_disconnect', { tabId: tab.id }).catch(() => {});
    } else if (tab.type === 'sftp') {
      invoke('sftp_disconnect', { tabId: tab.id }).catch(() => {});
    } else if (tab.type === 'local_terminal') {
      invoke('pty_close', { tabId: tab.id }).catch(() => {});
    } else if (tab.type === 'rdp') {
      invoke('rdp_disconnect', { tabId: tab.id }).catch(() => {});
    }
  }

  async saveAppSettings(settings: AppSettings) {
    try {
      await invoke('save_app_settings', { settings });
      this.config.app_settings = settings;
    } catch (err) {
      console.error('Failed to save settings via Tauri:', err);
      this.config.app_settings = settings;
    }
  }

  async importSessions(
    filePath: string,
    source?: string,
    masterPassword?: string,
    parentFolderId?: string
  ): Promise<ImportSummary> {
    const summary = await invoke<ImportSummary>('import_sessions', {
      filePath,
      source: source || 'auto',
      masterPassword: masterPassword || null,
      parentFolderId: parentFolderId || null,
    });
    await this.loadConfig();
    return summary;
  }

  async importSessionsContent(
    content: string,
    source?: string,
    masterPassword?: string,
    parentFolderId?: string
  ): Promise<ImportSummary> {
    const summary = await invoke<ImportSummary>('import_sessions_content', {
      content,
      source: source || 'auto',
      masterPassword: masterPassword || null,
      parentFolderId: parentFolderId || null,
    });
    await this.loadConfig();
    return summary;
  }
}

export const appState = new AppStore();
