// Só em desenvolvimento, fora do Tauri (navegador comum): simula o backend com dados de exemplo
// para ver e testar a interface. `?view=credentials|tunnels|settings|session` abre direto a página.
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';

const rdp = { domain: '', width: 0, height: 0, hidpi: false, clipboard: true, captureKeys: true, gfx: true };
const base = { auth: 'password', keyPath: '', jumpHostId: '', protocol: 'ssh', rdp, credentialId: '', hasPassword: true, hasPassphrase: false };

const hosts = [
  { ...base, id: 'h1', label: 'web-01', hostname: '10.0.1.11', port: 22, username: 'deploy', group: 'Produção', credentialId: 'c1' },
  { ...base, id: 'h2', label: 'web-02', hostname: '10.0.1.12', port: 22, username: 'deploy', group: 'Produção', credentialId: 'c1' },
  { ...base, id: 'h3', label: 'db-primary', hostname: '10.0.2.5', port: 22, username: 'postgres', group: 'Produção', jumpHostId: 'h5' },
  { ...base, id: 'h4', label: 'desktop-dev', hostname: '192.168.0.40', port: 3389, username: 'carlos', group: 'Laboratório', protocol: 'rdp' },
  { ...base, id: 'h5', label: 'bastion', hostname: 'bastion.empresa.com', port: 2222, username: 'carlos', group: '', auth: 'key' },
  { ...base, id: 'h6', label: 'pi-casa', hostname: '192.168.0.2', port: 22, username: 'pi', group: '' },
  { ...base, id: 'h7', label: 'k8s-node-1', hostname: '10.10.0.21', port: 22, username: 'ubuntu', group: 'Laboratório', auth: 'agent' },
];
const credentials = [
  { id: 'c1', label: 'Deploy produção', username: 'deploy', auth: 'password', keyPath: '', hasPassword: true, hasPassphrase: false, hasKey: false, publicKey: null, usedBy: ['web-01', 'web-02'] },
  { id: 'c2', label: 'Chave do Nexo', username: 'carlos', auth: 'key', keyPath: '', hasPassword: false, hasPassphrase: false, hasKey: true, publicKey: 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJqk… carlos@nexo', usedBy: [] },
];
const tunnels = [
  { id: 't1', label: 'Postgres produção', hostId: 'h3', kind: 'local', bindHost: '127.0.0.1', bindPort: 5432, destHost: 'localhost', destPort: 5432, autoStart: true },
  { id: 't2', label: '', hostId: 'h5', kind: 'dynamic', bindHost: '127.0.0.1', bindPort: 1080, destHost: '', destPort: 0, autoStart: false },
];
const shells = [
  { id: 'pwsh', name: 'PowerShell', kind: 'powershell', program: 'pwsh.exe', args: [] },
  { id: 'wsl', name: 'Ubuntu (WSL)', kind: 'wsl', program: 'wsl.exe', args: [] },
];

// Contadores que avançam a cada leitura, com uso de CPU e rede oscilando.
let tick = 0;
let busyJiffies = 0;
function metrics() {
  tick++;
  const GB = 1024 ** 3;
  busyJiffies += 8 * (20 + 15 * Math.sin(tick / 4) + Math.random() * 10);
  return {
    hostname: 'web-01',
    kernel: 'Linux 6.1.0-26-amd64',
    os: 'Debian GNU/Linux 12 (bookworm)',
    cpus: 4,
    cpuTotal: tick * 800,
    cpuIdle: tick * 800 - busyJiffies,
    load: [0.84, 0.62, 0.51],
    uptime: 352_000 + tick * 2,
    memTotal: 8 * GB,
    memAvailable: (3.1 + Math.sin(tick / 6) * 0.3) * GB,
    swapTotal: 2 * GB,
    swapFree: 1.7 * GB,
    netRx: tick * 1_400_000,
    netTx: tick * 260_000,
    disks: [
      { mount: '/', device: '/dev/sda1', total: 80 * GB, used: 61 * GB },
      { mount: '/var/lib/postgresql', device: '/dev/sdb1', total: 200 * GB, used: 187 * GB },
      { mount: '/mnt/backup', device: 'nas:/backup', total: 2048 * GB, used: 410 * GB },
    ],
  };
}

const container = (name: string, image: string, state: string, status: string, project = '', cpu: number | null = null, mem = '') => ({
  id: [...name].map((c) => c.charCodeAt(0).toString(16)).join('').padEnd(64, '0').slice(0, 64),
  name,
  image,
  state,
  status,
  ports: '',
  project,
  cpu,
  mem: cpu === null ? null : 1.2,
  memUsage: mem,
});
const containers = [
  container('portainer', 'portainer/portainer-ce:2.21', 'running', 'Up 3 days', '', 0.1, '38.2MiB / 7.6GiB'),
  container('backup-job', 'restic/restic:0.17', 'exited', 'Exited (0) 5 hours ago'),
  container('loja-api-1', 'ghcr.io/empresa/loja-api:1.8.2', 'running', 'Up 2 days (healthy)', 'loja', 12.4, '412.6MiB / 7.6GiB'),
  container('loja-db-1', 'postgres:16', 'running', 'Up 2 days (healthy)', 'loja', 3.2, '1.1GiB / 7.6GiB'),
  container('loja-redis-1', 'redis:7-alpine', 'paused', 'Up 2 days (Paused)', 'loja', null),
  container('loja-worker-1', 'ghcr.io/empresa/loja-api:1.8.2', 'restarting', 'Restarting (1) 4 seconds ago', 'loja', null),
  container('grafana', 'grafana/grafana:11.2.0', 'running', 'Up 6 hours', 'monitoring', 0.8, '96.4MiB / 7.6GiB'),
  container('prometheus', 'prom/prometheus:v2.54.1', 'running', 'Up 6 hours', 'monitoring', 2.1, '220.9MiB / 7.6GiB'),
];

export function installDevMock() {
  mockWindows('main');
  const view = new URLSearchParams(location.search).get('view');
  mockIPC((cmd) => {
    switch (cmd) {
      case 'list_shells':
        return shells;
      case 'hosts_list':
        return hosts;
      case 'credentials_list':
        return credentials;
      case 'tunnels_list':
        return tunnels;
      case 'tunnel_statuses':
        return { t1: { state: 'running', connections: 2 } };
      case 'settings_get':
        return { startWithTerminal: view === 'session', theme: new URLSearchParams(location.search).get('theme') ?? 'dark' };
      case 'plugin:app|version':
        return '0.1.0';
      case 'pty_spawn':
        return 1;
      case 'ssh_connect':
        return 2;
      case 'host_metrics':
        return metrics();
      case 'docker_list':
        return containers;
      default:
        return null;
    }
  });
}
