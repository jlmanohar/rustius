export type ConnectionType = 'ssh' | 'sftp' | 'rdp';

export type SecretRef =
  | { type: 'keyring'; key: string }
  | { type: 'vault'; key: string }
  | { type: 'plaintext'; value: string }
  | { type: 'ask_on_connect' }
  | { type: 'external'; provider: string; vault_id?: string; item_id: string };

export type VaultBackend = 'auto' | 'master_password' | 'keyring';

export interface VaultStatus {
  is_portable: boolean;
  configured_backend: VaultBackend;
  effective_backend: VaultBackend;
  has_vault_file: boolean;
  is_unlocked: boolean;
}

export type AuthMethod =
  | {
      type: 'password';
      username: string;
      password?: string;
      secret_ref?: SecretRef;
    }
  | {
      type: 'keyfile';
      username: string;
      key_path: string;
      passphrase?: string;
      secret_ref?: SecretRef;
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
  domain?: string;
  resolution_mode?: 'auto' | 'native' | '1080p' | '720p' | 'custom';
  scale_factor?: number;
  engine?: 'ironrdp' | 'freerdp';
  egfx?: boolean;
}

export interface IronRdpFrame {
  x: number;
  y: number;
  width: number;
  height: number;
  desktop_width: number;
  desktop_height: number;
  data: string; // Base64 encoded RGBA pixel bytes
}

export interface RdpExitDiagnostic {
  code: number;
  title: string;
  message: string;
  severity: 'security' | 'auth' | 'error' | 'info';
  raw_error?: string;
}


export interface CredentialProfile {
  id: string;
  name: string;
  domain?: string;
  auth: AuthMethod;
  created_at: string;
  updated_at: string;
}

export interface Connection {
  id: string;
  name: string;
  connection_type: ConnectionType;
  conn_type?: ConnectionType;
  host: string;
  port: number;
  auth: AuthMethod;
  environment?: string;
  description?: string;
  tags: string[];
  notes?: string;
  ssh_config?: SshConfig;
  rdp_config?: RdpConfig;
  credential_id?: string;
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
  | { node_type: 'connection'; id: string; name: string; connection_type: ConnectionType; conn_type?: ConnectionType; host: string; port: number; auth: AuthMethod; environment?: string; description?: string; tags: string[]; notes?: string; ssh_config?: SshConfig; rdp_config?: RdpConfig; credential_id?: string; created_at: string; last_connected?: string };

export interface AppSettings {
  theme: string;
  font_family: string;
  font_size: number;
  default_terminal_type: string;
  show_sidebar: boolean;
  sidebar_width: number;
  close_tab_on_exit?: boolean;
  sftp_follow_terminal_folder?: boolean;
  vault_backend?: VaultBackend;
}

export interface RustiusConfig {
  version: string;
  app_settings: AppSettings;
  credentials?: CredentialProfile[];
  tree: TreeNode[];
}

export type TabType = 'ssh' | 'sftp' | 'rdp' | 'local_terminal';

export interface CredentialOverride {
  username?: string;
  password?: string;
  domain?: string;
}

export interface TabSession {
  id: string;
  title: string;
  type: TabType;
  connectionId?: string;
  status: 'connecting' | 'connected' | 'disconnected' | 'error';
  badge?: string;
  credentialsOverride?: CredentialOverride;
}

export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified?: string;
  permissions?: string;
}

export interface TransferItem {
  id: string;
  source: string;
  dest: string;
  direction: 'upload' | 'download';
  status: 'pending' | 'transferring' | 'completed' | 'failed';
  error?: string;
}

export interface ImportSummary {
  connections_imported: number;
  folders_imported: number;
}

export type ImportFormat = 'auto' | 'mremoteng' | 'mobaxterm';

export interface DialogOptions {
  type: 'prompt' | 'confirm' | 'alert';
  title: string;
  message?: string;
  placeholder?: string;
  initialValue?: string;
  confirmText?: string;
  cancelText?: string;
  isDanger?: boolean;
  inputType?: 'text' | 'password';
}
