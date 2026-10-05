export type ConnectionType = 'ssh' | 'sftp' | 'rdp';

export type AuthMethod =
  | {
      type: 'password';
      username: string;
      password?: string;
    }
  | {
      type: 'keyfile';
      username: string;
      key_path: string;
      passphrase?: string;
    }
  | {
      type: 'agent';
      username: string;
    };

export interface PortForward {
  local_port: number;
  remote_host: string;
  remote_port: number;
}

export interface SshConfig {
  keepalive_interval_secs: number;
  connect_timeout_secs: number;
  legacy_ciphers: boolean;
  legacy_kex: boolean;
  port_forwards: PortForward[];
}

export interface RdpConfig {
  width: number;
  height: number;
  color_depth: number;
  fullscreen: boolean;
  clipboard_sharing: boolean;
}

export interface Connection {
  id: string;
  name: string;
  conn_type: ConnectionType;
  connection_type?: ConnectionType;
  host: string;
  port: number;
  auth: AuthMethod;
  environment?: string;
  tags: string[];
  notes?: string;
  ssh_config?: SshConfig;
  rdp_config?: RdpConfig;
  created_at: string;
  updated_at?: string;
  last_connected?: string;
}

export interface Folder {
  id: string;
  name: string;
  parent_id?: string;
  children: TreeNode[];
  expanded: boolean;
}

export type TreeNode =
  | { node_type: 'folder'; id: string; name: string; parent_id?: string; children: TreeNode[]; expanded: boolean }
  | { node_type: 'connection'; id: string; name: string; conn_type: ConnectionType; host: string; port: number; auth: AuthMethod; environment?: string; tags: string[]; ssh_config?: SshConfig; rdp_config?: RdpConfig; created_at: string; last_connected?: string };

export interface AppSettings {
  theme: string;
  font_family: string;
  font_size: number;
  default_terminal_type: string;
  show_sidebar: boolean;
  sidebar_width: number;
}

export interface RustiusConfig {
  version: string;
  app_settings: AppSettings;
  tree: TreeNode[];
}

export type TabType = 'ssh' | 'sftp' | 'rdp' | 'local_terminal';

export interface TabSession {
  id: string;
  title: string;
  type: TabType;
  connectionId?: string;
  status: 'connecting' | 'connected' | 'disconnected' | 'error';
  badge?: string;
}
