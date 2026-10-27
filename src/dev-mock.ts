// Só em desenvolvimento, fora do Tauri (navegador comum): simula o backend com dados de exemplo
// para ver e testar a interface. `?view=credentials|tunnels|settings|session` abre direto a página.
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';

const rdp = { domain: '', width: 0, height: 0, hidpi: false, clipboard: true, captureKeys: true, gfx: true };
const base = { auth: 'password', keyPath: '', jumpHostId: '', protocol: 'ssh', rdp, credentialId: '', dockerSudo: false, hasPassword: true, hasPassphrase: false };

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

// Kubernetes: dois contextos, alguns namespaces e tabelas como as do servidor.
const kubeContexts = {
  source: 'C:\\Users\\carlos\\.kube\\config',
  current: 'prod-sa-east-1',
  contexts: [
    { name: 'prod-sa-east-1', cluster: 'prod', user: 'carlos', namespace: '', server: 'https://k8s.empresa.com:6443' },
    { name: 'kind-treino', cluster: 'kind-treino', user: 'kind-treino', namespace: '', server: 'https://127.0.0.1:38927' },
  ],
};
const col = (name: string, priority = 0) => ({ name, type: 'string', priority });
const kubeRow = (namespace: string, cells: unknown[]) => ({ name: String(cells[0]), namespace, uid: `${namespace}/${cells[0]}`, cells });
const kubeTables: Record<string, { columns: ReturnType<typeof col>[]; rows: ReturnType<typeof kubeRow>[] }> = {
  pod: {
    columns: [col('Name'), col('Ready'), col('Status'), col('Restarts'), col('Age'), col('IP', 1), col('Node', 1)],
    rows: [
      kubeRow('loja', ['loja-api-7d9f8b6c5-2xkqp', '1/1', 'Running', 0, '2d4h', '10.244.1.12', 'node-a']),
      kubeRow('loja', ['loja-api-7d9f8b6c5-h8mzt', '1/1', 'Running', '1 (3h ago)', '2d4h', '10.244.2.7', 'node-b']),
      kubeRow('loja', ['loja-worker-5c7b9d4f8-qq2lz', '0/1', 'CrashLoopBackOff', '14 (2m ago)', '51m', '10.244.2.9', 'node-b']),
      kubeRow('loja', ['loja-migrate-29844-x7k2d', '0/1', 'Completed', 0, '6h', '10.244.1.30', 'node-a']),
      kubeRow('monitoring', ['prometheus-0', '2/2', 'Running', 0, '12d', '10.244.1.4', 'node-a']),
      kubeRow('monitoring', ['grafana-6f6d9c7b8-5vtnm', '1/1', 'Running', 0, '12d', '10.244.2.3', 'node-b']),
      kubeRow('kube-system', ['coredns-559f6c778d-r2bwf', '1/1', 'Running', '2 (14h ago)', '40d', '10.244.0.3', 'control-plane']),
      kubeRow('kube-system', ['metrics-server-85b7d694d7-9m4cv', '0/1', 'Pending', 0, '8s', '<none>', '<none>']),
    ],
  },
  deployment: {
    columns: [col('Name'), col('Ready'), col('Up-to-date'), col('Available'), col('Age'), col('Images', 1)],
    rows: [
      kubeRow('loja', ['loja-api', '2/2', 2, 2, '30d', 'ghcr.io/empresa/loja-api:1.8.2']),
      kubeRow('loja', ['loja-worker', '0/1', 1, 0, '30d', 'ghcr.io/empresa/loja-api:1.8.2']),
      kubeRow('monitoring', ['grafana', '1/1', 1, 1, '12d', 'grafana/grafana:11.2.0']),
    ],
  },
  node: {
    columns: [col('Name'), col('Status'), col('Roles'), col('Age'), col('Version')],
    rows: [
      kubeRow('', ['control-plane', 'Ready', 'control-plane', '40d', 'v1.34.1']),
      kubeRow('', ['node-a', 'Ready', '<none>', '40d', 'v1.34.1']),
      kubeRow('', ['node-b', 'Ready,SchedulingDisabled', '<none>', '40d', 'v1.34.1']),
    ],
  },
};
const kubeYaml = `apiVersion: v1
kind: Pod
metadata:
  creationTimestamp: "2026-10-25T14:02:11Z"
  labels:
    app: loja-api
    pod-template-hash: 7d9f8b6c5
  name: loja-api-7d9f8b6c5-2xkqp
  namespace: loja
spec:
  containers:
  - image: ghcr.io/empresa/loja-api:1.8.2
    name: api
    ports:
    - containerPort: 8080
      protocol: TCP
    readinessProbe:
      httpGet:
        path: /health
        port: 8080
      periodSeconds: 10
  restartPolicy: Always
status:
  phase: Running
  podIP: 10.244.1.12
  qosClass: BestEffort
  startTime: "2026-10-25T14:02:11Z"
`;

function kubeList(kind: string, namespace: string | null) {
  const table = kubeTables[kind] ?? { columns: [col('Name'), col('Age')], rows: [kubeRow('loja', [`exemplo-${kind.toLowerCase()}`, '3d'])] };
  return { ...table, rows: table.rows.filter((r) => !namespace || !r.namespace || r.namespace === namespace), truncated: false };
}

export function installDevMock() {
  mockWindows('main');
  const params = new URLSearchParams(location.search);
  const view = params.get('view');
  // `?docker=denied`: usuário fora do grupo docker; o sudo pede senha (qualquer uma serve).
  // `&sudo=1`: o primeiro host já está com "Docker com sudo" ligado.
  const denied = params.get('docker') === 'denied';
  if (params.get('sudo')) hosts[0].dockerSudo = true;
  mockIPC((cmd, args) => {
    const a = args as Record<string, any> | undefined;
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
      case 'docker_action':
        if (denied && !a?.sudo) return Promise.reject({ kind: 'permission', message: 'sem permissão para usar o Docker (o usuário não está no grupo docker)' });
        if (denied && !a?.sudo.password) return Promise.reject({ kind: 'needSudoPassword', wrong: false });
        return cmd === 'docker_list' ? containers : null;
      case 'k8s_contexts':
        return kubeContexts;
      case 'k8s_namespaces':
        return ['default', 'kube-system', 'loja', 'monitoring'];
      case 'k8s_list':
        return kubeList(a?.kind, a?.namespace);
      case 'k8s_yaml':
        return kubeYaml;
      case 'k8s_containers':
        return { containers: [{ name: 'api', init: false }, { name: 'istio-proxy', init: false }], default: 'api' };
      case 'k8s_ports':
        return [
          { port: 8080, name: 'http', protocol: 'TCP' },
          { port: 9090, name: 'metrics', protocol: 'TCP' },
        ];
      case 'k8s_forwards':
        return [
          { id: 1, context: 'prod-sa-east-1', namespace: 'loja', kind: 'service', name: 'loja-api', port: 8080, localPort: 8080, connections: 2, error: '' },
          { id: 2, context: 'kind-treino', namespace: 'monitoring', kind: 'pod', name: 'grafana-6f6d9c7b8-5vtnm', port: 3000, localPort: 3001, connections: 0, error: 'nada escutando na porta 3000 do pod' },
        ];
      case 'host_save': {
        const index = hosts.findIndex((h) => h.id === a?.host.id);
        if (index >= 0) hosts[index] = { ...hosts[index], ...a?.host };
        return a?.host;
      }
      default:
        return null;
    }
  });
}
