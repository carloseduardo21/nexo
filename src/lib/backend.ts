import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface ShellProfile {
  id: string;
  name: string;
  kind: 'powershell' | 'cmd' | 'wsl' | 'unix';
  program: string;
  args: string[];
}

export type Protocol = 'ssh' | 'rdp';

export interface RdpSettings {
  domain: string;
  /** Resolução fixa; 0×0 = acompanha o tamanho do painel. */
  width: number;
  height: number;
  /** Resolução em pixels físicos (nítido em telas com escala; a fonte remota fica menor). */
  hidpi: boolean;
  /** Área de transferência compartilhada (texto). */
  clipboard: boolean;
  /** Win, Alt+Tab e os atalhos do Nexo vão para o servidor enquanto o painel tem foco (Ctrl+Alt+Home solta). */
  captureKeys: boolean;
  /** Pipeline gráfico (GFX/H.264): mais fluido; desligado volta aos bitmaps. */
  gfx: boolean;
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
  /** Host salvo usado como jump host (ProxyJump); '' = conexão direta. Em RDP, é o gateway SSH. */
  jumpHostId: string;
  protocol: Protocol;
  rdp: RdpSettings;
  /** Credencial salva (usuário, método e segredos vêm dela); '' = dados do próprio host. */
  credentialId: string;
}

export interface Credential {
  id: string;
  label: string;
  username: string;
  auth: 'password' | 'key' | 'agent';
  /** Arquivo da chave; '' com chave guardada no cofre (ou para tentar as padrão). */
  keyPath: string;
}

export interface CredentialView extends Credential {
  hasPassword: boolean;
  hasPassphrase: boolean;
  /** Chave privada guardada no cofre (colada ou gerada). */
  hasKey: boolean;
  /** Linha do authorized_keys da chave guardada. */
  publicKey: string | null;
  /** Nomes dos hosts que usam a credencial. */
  usedBy: string[];
}

/** undefined mantém o salvo, '' apaga, texto troca. */
export interface SecretUpdate {
  password?: string;
  passphrase?: string;
  privateKey?: string;
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
  | { kind: 'certChanged'; fingerprint: string }
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

/** Leitura do servidor (`hostinfo.rs`). Bytes; CPU e rede são contadores desde o boot. */
export interface HostMetrics {
  hostname: string;
  kernel: string;
  os: string;
  cpus: number;
  cpuTotal: number;
  cpuIdle: number;
  load: [number, number, number];
  uptime: number;
  memTotal: number;
  memAvailable: number;
  swapTotal: number;
  swapFree: number;
  netRx: number;
  netTx: number;
  disks: { mount: string; device: string; total: number; used: number }[];
}

export interface Container {
  id: string;
  name: string;
  image: string;
  state: 'running' | 'exited' | 'paused' | 'restarting' | 'created' | 'dead' | string;
  status: string;
  ports: string;
  /** Projeto do Docker Compose ('' se não for de um). */
  project: string;
  cpu: number | null;
  mem: number | null;
  memUsage: string;
}

export type ContainerAction = 'start' | 'stop' | 'restart' | 'pause' | 'unpause' | 'remove';

/** Terminal SSH que roda um comando em vez do shell. */
export type Exec = { kind: 'dockerLogs'; container: string } | { kind: 'dockerShell'; container: string };

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

/** Input de um painel RDP; teclas por scancode (o layout é o do servidor). */
export type RdpInput =
  | { t: 'key'; code: number; down: boolean }
  | { t: 'unicode'; ch: string; down: boolean }
  | { t: 'move'; x: number; y: number }
  | { t: 'button'; button: number; down: boolean }
  | { t: 'wheel'; delta: number; vertical: boolean }
  | { t: 'sync'; caps: boolean; num: boolean; scroll: boolean }
  | { t: 'releaseAll' }
  | { t: 'clipboardSync' };

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
    /** `parent`: sessão cuja conexão é reaproveitada (sem novo login), se ainda estiver aberta. */
    exec?: Exec,
    parent?: number | null,
  ) => {
    const rtt = new Channel<number>();
    rtt.onmessage = onRtt;
    return invoke<number>('ssh_connect', { hostId, options, cols, rows, exec, parent, ...channels(onData, onExit), onRtt: rtt });
  },

  /** onFrame: mensagens binárias de tela (ver `rdp.rs`); onExit: motivo do fim, ou null se foi normal. */
  connectRdp: (
    hostId: string,
    options: RouteOptions,
    width: number,
    height: number,
    onFrame: (buf: ArrayBuffer) => void,
    onExit: (reason: string | null) => void,
  ) => {
    const frame = new Channel<ArrayBuffer>();
    frame.onmessage = onFrame;
    const exit = new Channel<string | null>();
    exit.onmessage = onExit;
    return invoke<number>('rdp_connect', { hostId, options, width, height, onFrame: frame, onExit: exit });
  },
  rdpInput: (id: number, events: RdpInput[]) => void invoke('rdp_input', { id, events }).catch(report),
  /** Captura o teclado (hook do Windows) para a sessão; null solta. Devolve se a captura ficou ativa. */
  rdpCapture: (id: number | null) => invoke<boolean>('rdp_capture', { id }).catch(() => false),
  onRdpRelease: (handler: () => void) => listen('rdp-release', handler),

  listHosts: () => invoke<HostView[]>('hosts_list'),
  /** password/passphrase: undefined mantém o salvo, '' apaga. */
  saveHost: (host: Host, password?: string, passphrase?: string) =>
    invoke<Host>('host_save', { host, password, passphrase }),
  deleteHost: (id: string) => invoke<void>('host_delete', { id }),
  listCredentials: () => invoke<CredentialView[]>('credentials_list'),
  saveCredential: (credential: Credential, secrets: SecretUpdate) => invoke<Credential>('credential_save', { credential, secrets }),
  deleteCredential: (id: string) => invoke<void>('credential_delete', { id }),
  generateKey: (label: string, username: string) => invoke<CredentialView>('credential_generate_key', { label, username }),
  /** Devolvem quantos hosts foram exportados/importados. */
  exportVault: (path: string) => invoke<number>('vault_export', { path }),
  importVault: (path: string) => invoke<number>('vault_import', { path }),
  /** Sem `path`: ~/.ssh/config. */
  sshConfigScan: (path?: string) => invoke<{ path: string; entries: SshConfigEntry[] }>('ssh_config_scan', { path }),
  /** Devolve quantos hosts foram criados (inclui jump hosts de que os escolhidos dependem). */
  sshConfigImport: (path: string, aliases: string[]) => invoke<number>('ssh_config_import', { path, aliases }),

  hostMetrics: (id: number) => invoke<HostMetrics>('host_metrics', { id }),
  dockerList: (id: number) => invoke<Container[]>('docker_list', { id }),
  dockerAction: (id: number, container: string, action: ContainerAction) => invoke<void>('docker_action', { id, container, action }),

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
