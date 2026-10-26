import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface ShellProfile {
  id: string;
  name: string;
  kind: 'powershell' | 'cmd' | 'wsl' | 'unix';
  program: string;
  args: string[];
}

export interface Host {
  id: string;
  label: string;
  hostname: string;
  port: number;
  username: string;
  auth: 'password' | 'key' | 'agent';
  keyPath: string;
  group: string;
  /** Host salvo usado como jump host (ProxyJump); '' = conexão direta. */
  jumpHostId: string;
}

export interface HostView extends Host {
  hasPassword: boolean;
  hasPassphrase: boolean;
}

export interface ConnectOptions {
  password?: string;
  passphrase?: string;
  trustFingerprint?: string;
  remember?: boolean;
}

export type ConnectError =
  | { kind: 'unknownHost'; fingerprint: string; keyType: string }
  | { kind: 'hostKeyChanged'; fingerprint: string; keyType: string; line: number }
  | { kind: 'needPassword' }
  | { kind: 'needPassphrase'; keyPath: string }
  | { kind: 'authFailed'; message: string }
  | { kind: 'other'; message: string };

/** Erro de conexão e o host do caminho (destino ou jump host) em que aconteceu. */
export type ConnectFailure = ConnectError & { hostId: string };

/** Opções de conexão por host (id): o destino e cada jump host podem pedir senha/fingerprint. */
export type RouteOptions = Record<string, ConnectOptions>;

export function asFailure(err: unknown, hostId: string): ConnectFailure {
  if (typeof err === 'object' && err && 'kind' in err) {
    const failure = err as ConnectError & { hostId?: string };
    return { ...failure, hostId: failure.hostId || hostId };
  }
  return { kind: 'other', message: String(err), hostId };
}

export interface SshConfigEntry {
  alias: string;
  hostname: string;
  port: number;
  user: string;
  identityFile: string;
  proxyJump: string;
  /** Já existe no Nexo um host igual. */
  exists: boolean;
}

export interface RemoteEntry {
  name: string;
  path: string;
  isDir: boolean;
  isLink: boolean;
  size: number;
  /** Segundos desde 1970. */
  modified: number | null;
  perms: string;
}

export interface TransferProgress {
  done: number;
  total: number;
  current: string;
}

export type TunnelKind = 'local' | 'remote' | 'dynamic';

export interface Tunnel {
  id: string;
  label: string;
  hostId: string;
  kind: TunnelKind;
  /** Local/dinâmico: onde escutar nesta máquina. Remoto: onde escutar no servidor. */
  bindHost: string;
  bindPort: number;
  /** Local: destino visto a partir do servidor. Remoto: destino visto desta máquina. */
  destHost: string;
  destPort: number;
  autoStart: boolean;
}

export type TunnelStatus =
  | { state: 'connecting' }
  | { state: 'running'; connections: number }
  | { state: 'reconnecting'; attempt: number; message: string }
  | { state: 'error'; message: string };

export type DataHandler = (data: Uint8Array) => void;
export type ExitHandler = (code: number | null) => void;

const report = (err: unknown) => console.error('[nexo]', err);

function channels(onData: DataHandler, onExit: ExitHandler) {
  const data = new Channel<ArrayBuffer>();
  data.onmessage = (buf) => onData(new Uint8Array(buf));
  const exit = new Channel<number | null>();
  exit.onmessage = onExit;
  return { onData: data, onExit: exit };
}

function progressChannel(onProgress: (p: TransferProgress) => void) {
  const channel = new Channel<TransferProgress>();
  channel.onmessage = onProgress;
  return channel;
}

export const backend = {
  listShells: () => invoke<ShellProfile[]>('list_shells'),

  spawnPty: (profileId: string, cols: number, rows: number, onData: DataHandler, onExit: ExitHandler) =>
    invoke<number>('pty_spawn', { profileId, cols, rows, ...channels(onData, onExit) }),

  connectSsh: (
    hostId: string,
    options: RouteOptions,
    cols: number,
    rows: number,
    onData: DataHandler,
    onExit: ExitHandler,
    onRtt: (ms: number) => void,
  ) => {
    const rtt = new Channel<number>();
    rtt.onmessage = onRtt;
    return invoke<number>('ssh_connect', { hostId, options, cols, rows, ...channels(onData, onExit), onRtt: rtt });
  },

  listHosts: () => invoke<HostView[]>('hosts_list'),
  /** password/passphrase: undefined mantém o salvo, '' apaga. */
  saveHost: (host: Host, password?: string, passphrase?: string) =>
    invoke<Host>('host_save', { host, password, passphrase }),
  deleteHost: (id: string) => invoke<void>('host_delete', { id }),
  /** Sem `path`: ~/.ssh/config. */
  sshConfigScan: (path?: string) => invoke<{ path: string; entries: SshConfigEntry[] }>('ssh_config_scan', { path }),
  /** Devolve quantos hosts foram criados (inclui jump hosts de que os escolhidos dependem). */
  sshConfigImport: (path: string, aliases: string[]) => invoke<number>('ssh_config_import', { path, aliases }),

  sftpHome: (id: number) => invoke<string>('sftp_home', { id }),
  sftpList: (id: number, path: string) => invoke<RemoteEntry[]>('sftp_list', { id, path }),
  sftpMkdir: (id: number, path: string) => invoke<void>('sftp_mkdir', { id, path }),
  sftpRename: (id: number, from: string, to: string) => invoke<void>('sftp_rename', { id, from, to }),
  sftpRemove: (id: number, paths: string[]) => invoke<void>('sftp_remove', { id, paths }),
  downloadDir: () => invoke<string>('download_dir'),
  /** Devolve os caminhos locais criados. */
  sftpDownload: (id: number, transferId: number, paths: string[], localDir: string, onProgress: (p: TransferProgress) => void) =>
    invoke<string[]>('sftp_download', { id, transferId, paths, localDir, onProgress: progressChannel(onProgress) }),
  sftpUpload: (id: number, transferId: number, paths: string[], remoteDir: string, onProgress: (p: TransferProgress) => void) =>
    invoke<void>('sftp_upload', { id, transferId, paths, remoteDir, onProgress: progressChannel(onProgress) }),
  /** O backend digita a senha salva do host nas sessões (a senha não passa pelo frontend). */
  sendSecret: (ids: number[], hostId: string) => invoke<void>('session_send_secret', { ids, hostId }),
  listTunnels: () => invoke<Tunnel[]>('tunnels_list'),
  saveTunnel: (tunnel: Tunnel) => invoke<Tunnel>('tunnel_save', { tunnel }),
  deleteTunnel: (id: string) => invoke<void>('tunnel_delete', { id }),
  startTunnel: (id: string, options: RouteOptions) => invoke<void>('tunnel_start', { id, options }),
  stopTunnel: (id: string) => invoke<void>('tunnel_stop', { id }),
  tunnelStatuses: () => invoke<Record<string, TunnelStatus>>('tunnel_statuses'),
  onTunnelStatus: (handler: (event: { id: string; status: TunnelStatus | null }) => void) =>
    listen<{ id: string; status: TunnelStatus | null }>('tunnel-status', (e) => handler(e.payload)),

  cancelTransfer: (transferId: number) => void invoke('transfer_cancel', { transferId }).catch(report),

  // Fire-and-forget: o caminho da tecla não espera resposta.
  write: (ids: number[], data: string) => void invoke('session_write', { ids, data }).catch(report),
  ack: (id: number, bytes: number) => void invoke('session_ack', { id, bytes }).catch(report),
  resize: (id: number, cols: number, rows: number) => void invoke('session_resize', { id, cols, rows }).catch(report),
  kill: (id: number) => void invoke('session_kill', { id }).catch(report),
};
