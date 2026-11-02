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
kubeTables.sealedsecrets = {
  columns: [col('Name'), col('Status'), col('Synced'), col('Age')],
  rows: [
    kubeRow('loja', ['loja-db', '', 'True', '12d']),
    kubeRow('loja', ['loja-registry', '', 'True', '30d']),
    kubeRow('monitoring', ['grafana-admin', 'no key could decrypt secret (admin-password)', 'False', '2h']),
  ],
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

function kubeList(resource: string | { plural: string }, namespace: string | null) {
  const kind = typeof resource === 'string' ? resource : resource.plural;
  const table = kubeTables[kind] ?? { columns: [col('Name'), col('Age')], rows: [kubeRow('loja', [`exemplo-${kind.toLowerCase()}`, '3d'])] };
  return { ...table, rows: table.rows.filter((r) => !namespace || !r.namespace || r.namespace === namespace), truncated: false };
}

// Argo CD: apps em alguns projetos, com estados variados, e a árvore de um deles.
const minutesAgo = (m: number) => new Date(Date.now() - m * 60_000).toISOString();
const GITOPS = 'https://github.com/empresa/gitops.git';
type MockApp = Record<string, any>;
function argoApp(name: string, project: string, path: string, health: string, sync: string, extra: MockApp = {}): MockApp {
  return {
    name,
    namespace: 'argocd',
    project,
    sources: [{ repoUrl: GITOPS, path, chart: '', targetRevision: 'main', ref: '' }],
    destServer: 'https://kubernetes.default.svc',
    destName: '',
    destNamespace: name.split('-')[0],
    inCluster: true,
    sync,
    revisions: ['9f2c4e1a7b3d5f6e8a0c1b2d3e4f5a6b7c8d9e0f'],
    health,
    healthMessage: '',
    autoSync: true,
    prune: true,
    selfHeal: false,
    operation: { phase: 'Succeeded', message: 'successfully synced (all tasks run)', startedAt: minutesAgo(42), finishedAt: minutesAgo(41.8), revision: '9f2c4e1', initiatedBy: '', automated: true, dryRun: false },
    busy: false,
    reconciledAt: minutesAgo(1),
    created: minutesAgo(60 * 24 * 40),
    labels: {},
    conditions: [],
    counts: { total: 8, outOfSync: sync === 'OutOfSync' ? 2 : 0, degraded: health === 'Degraded' ? 1 : 0, progressing: health === 'Progressing' ? 1 : 0, missing: 0 },
    ...extra,
  };
}
const argoApps = [
  argoApp('loja-api', 'pagamentos', 'apps/loja/api', 'Degraded', 'OutOfSync', {
    autoSync: false,
    operation: { phase: 'Failed', message: 'one or more objects failed to apply: Deployment.apps "loja-worker" is invalid', startedAt: minutesAgo(12), finishedAt: minutesAgo(11.9), revision: '9f2c4e1', initiatedBy: 'carlos', automated: false, dryRun: false },
    conditions: [{ type: 'SyncError', message: 'Failed sync attempt to 9f2c4e1: one or more objects failed to apply', time: minutesAgo(12) }],
  }),
  argoApp('loja-checkout', 'pagamentos', 'apps/loja/checkout', 'Healthy', 'Synced'),
  argoApp('loja-notificacoes', 'pagamentos', 'apps/loja/notificacoes', 'Progressing', 'Synced', {
    busy: true,
    operation: { phase: 'Running', message: 'waiting for healthy state of apps/Deployment/notificacoes', startedAt: minutesAgo(0.5), finishedAt: '', revision: '9f2c4e1', initiatedBy: '', automated: true, dryRun: false },
  }),
  argoApp('monitoring-stack', 'plataforma', '', 'Healthy', 'Synced', {
    sources: [
      { repoUrl: 'https://prometheus-community.github.io/helm-charts', path: '', chart: 'kube-prometheus-stack', targetRevision: '65.1.0', ref: '' },
      { repoUrl: GITOPS, path: '', chart: '', targetRevision: 'main', ref: 'values' },
    ],
    revisions: ['65.1.0', '9f2c4e1a7b3d5f6e8a0c1b2d3e4f5a6b7c8d9e0f'],
    counts: { total: 112, outOfSync: 0, degraded: 0, progressing: 0, missing: 0 },
  }),
  argoApp('ingress-nginx', 'plataforma', 'infra/ingress-nginx', 'Healthy', 'Synced', { selfHeal: true }),
  argoApp('cert-manager', 'plataforma', 'infra/cert-manager', 'Healthy', 'OutOfSync', { counts: { total: 47, outOfSync: 1, degraded: 0, progressing: 0, missing: 0 } }),
  argoApp('dados-postgres', 'dados', 'apps/dados/postgres', 'Suspended', 'Synced', { autoSync: false, prune: false }),
  argoApp('dados-etl', 'dados', 'apps/dados/etl', 'Missing', 'OutOfSync', {
    destServer: 'https://k8s-analytics.empresa.com:6443',
    inCluster: false,
    autoSync: false,
    operation: null,
  }),
  argoApp('guestbook', 'default', 'guestbook', 'Healthy', 'Synced', { sources: [{ repoUrl: 'https://github.com/argoproj/argocd-example-apps.git', path: 'guestbook', chart: '', targetRevision: 'HEAD', ref: '' }] }),
];
const argoProjects = [
  { name: 'dados', namespace: 'argocd', description: 'Bancos e pipelines do time de dados' },
  { name: 'default', namespace: 'argocd', description: '' },
  { name: 'pagamentos', namespace: 'argocd', description: 'Loja, checkout e notificações' },
  { name: 'plataforma', namespace: 'argocd', description: 'Infra compartilhada do cluster' },
];

function argoNode(kind: string, name: string, parent: string, extra: MockApp = {}): MockApp {
  const group = { Deployment: 'apps', ReplicaSet: 'apps', StatefulSet: 'apps', Ingress: 'networking.k8s.io', HorizontalPodAutoscaler: 'autoscaling', CronJob: 'batch', Job: 'batch', SealedSecret: 'bitnami.com' }[kind] ?? '';
  const managed = !parent || extra.managed;
  return {
    id: `${group}/${kind}/loja/${name}`,
    parent,
    group,
    version: kind === 'SealedSecret' ? 'v1alpha1' : kind === 'HorizontalPodAutoscaler' ? 'v2' : 'v1',
    kind,
    namespace: 'loja',
    name,
    managed,
    sync: managed ? 'Synced' : '',
    health: ['ConfigMap', 'SealedSecret', 'Secret'].includes(kind) ? '' : 'Healthy',
    healthMessage: '',
    result: '',
    hook: false,
    requiresPruning: false,
    info: [],
    images: [],
    created: minutesAgo(600),
    old: false,
    resource: null,
    ...extra,
  };
}
const RS = 'apps/ReplicaSet/loja/';
const argoNodes = [
  argoNode('ConfigMap', 'loja-config', '', { sync: 'OutOfSync' }),
  argoNode('ConfigMap', 'loja-feature-flags', '', { sync: 'OutOfSync', requiresPruning: true }),
  argoNode('SealedSecret', 'loja-db', ''),
  argoNode('Service', 'loja-api', ''),
  argoNode('Deployment', 'loja-api', ''),
  argoNode('Deployment', 'loja-worker', '', { health: 'Degraded', healthMessage: 'Deployment "loja-worker" exceeded its progress deadline', result: 'SyncFailed: Deployment.apps "loja-worker" is invalid: spec.template.spec.containers[0].image: Required value' }),
  argoNode('CronJob', 'loja-relatorios', ''),
  argoNode('Ingress', 'loja-api', '', { health: 'Progressing', healthMessage: 'aguardando o endereço do load balancer' }),
  argoNode('HorizontalPodAutoscaler', 'loja-api', ''),
  argoNode('ReplicaSet', 'loja-api-7d9f8b6c5', 'apps/Deployment/loja/loja-api', { info: ['rev 4', '2/2 prontos'] }),
  argoNode('ReplicaSet', 'loja-api-6c5d8f7b9', 'apps/Deployment/loja/loja-api', { info: ['rev 3'], old: true, health: '' }),
  argoNode('ReplicaSet', 'loja-worker-5c7b9d4f8', 'apps/Deployment/loja/loja-worker', { info: ['rev 2', '0/1 prontos'], health: 'Progressing' }),
  argoNode('Pod', 'loja-api-7d9f8b6c5-2xkqp', RS + 'loja-api-7d9f8b6c5', { info: ['Running', '1/1 prontos', 'node-a'], images: ['ghcr.io/empresa/loja-api:1.8.2'] }),
  argoNode('Pod', 'loja-api-7d9f8b6c5-h8mzt', RS + 'loja-api-7d9f8b6c5', { info: ['Running', '1/1 prontos', '1 reinícios', 'node-b'], images: ['ghcr.io/empresa/loja-api:1.8.2'] }),
  argoNode('Pod', 'loja-worker-5c7b9d4f8-qq2lz', RS + 'loja-worker-5c7b9d4f8', { health: 'Degraded', healthMessage: 'back-off 5m0s restarting failed container', info: ['CrashLoopBackOff', '0/1 prontos', '14 reinícios', 'node-b'] }),
  argoNode('Job', 'loja-relatorios-29844', 'batch/CronJob/loja/loja-relatorios', { info: ['concluído (1)'] }),
  argoNode('Pod', 'loja-relatorios-29844-x7k2d', 'batch/Job/loja/loja-relatorios-29844', { info: ['Succeeded', '0/1 prontos', 'node-a'] }),
];
const argoHistory = [5, 4, 3, 2, 1].map((id, i) => ({
  id,
  revisions: [['9f2c4e1a7b3d5f6e8a0c1b2d3e4f5a6b7c8d9e0f', '3b1d9a0c', 'e47aa21f', '0c9d8e7f', 'a1b2c3d4'][i]],
  sources: [{ repoUrl: GITOPS, path: 'apps/loja/api', chart: '', targetRevision: 'main', ref: '' }],
  deployedAt: minutesAgo([12, 60 * 26, 60 * 24 * 3, 60 * 24 * 9, 60 * 24 * 30][i]),
  startedAt: minutesAgo([12.2, 60 * 26 + 0.4, 60 * 24 * 3 + 1, 60 * 24 * 9 + 0.3, 60 * 24 * 30 + 0.2][i]),
  initiatedBy: i % 2 ? '' : 'carlos',
  automated: i % 2 === 1,
}));

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
      case 'k8s_custom_resources':
        return [
          { group: 'cert-manager.io', version: 'v1', plural: 'certificates', kind: 'Certificate', namespaced: true },
          { group: 'cert-manager.io', version: 'v1', plural: 'clusterissuers', kind: 'ClusterIssuer', namespaced: false },
          { group: 'monitoring.coreos.com', version: 'v1', plural: 'servicemonitors', kind: 'ServiceMonitor', namespaced: true },
          { group: 'bitnami.com', version: 'v1alpha1', plural: 'sealedsecrets', kind: 'SealedSecret', namespaced: true },
          { group: 'argoproj.io', version: 'v1alpha1', plural: 'applications', kind: 'Application', namespaced: true },
          { group: 'argoproj.io', version: 'v1alpha1', plural: 'appprojects', kind: 'AppProject', namespaced: true },
        ];
      case 'k8s_argo_apps':
        return { apps: argoApps, projects: argoProjects, url: 'https://argocd.empresa.com' };
      case 'k8s_argo_app': {
        const app = argoApps.find((x) => x.name === a?.name) ?? argoApps[0];
        return { app, nodes: app.name === 'loja-api' ? argoNodes : argoNodes.slice(3, 5).concat(argoNodes.slice(9, 14)), history: argoHistory };
      }
      case 'k8s_argo_action':
        return null;
      case 'k8s_sealed':
        return {
          scope: 'strict',
          type: 'Opaque',
          entries: [
            { key: 'DB_USER', value: 'loja', base64: false },
            { key: 'DB_PASSWORD', value: 'c0rr3t0-h0rse', base64: false },
            { key: 'ca.crt', value: null, base64: false },
          ],
          valuesError: '',
          status: '',
          resourceVersion: '4182',
        };
      case 'k8s_secret_plain':
        return { type: 'Opaque', entries: [{ key: 'token', value: 'ghp_exemplo', base64: false }] };
      case 'k8s_seal':
        return 'apiVersion: bitnami.com/v1alpha1\nkind: SealedSecret\n';
      case 'k8s_metrics':
        return Object.fromEntries(
          kubeTables[a?.nodes ? 'node' : 'pod'].rows.map((r, i) => [r.namespace ? `${r.namespace}/${r.name}` : r.name, { cpu: 12 + i * 37, memory: (48 + i * 90) * 1024 ** 2 }]),
        );
      case 'k8s_apply':
        return a?.yaml;
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
      // `?update=1`: há uma versão nova (toast de atualização uns segundos depois de abrir).
      case 'plugin:updater|check':
        return new URLSearchParams(location.search).has('update') ? { rid: 1, currentVersion: '0.3.0', version: '0.4.0', rawJson: {} } : null;
      default:
        return null;
    }
  });
}
