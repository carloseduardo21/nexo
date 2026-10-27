// Página Kubernetes: contexto, namespace e tipo escolhidos, a lista visível e o YAML aberto.
import { backend, type KubeContexts, type KubeKind, type KubeTable } from './backend';
import { errorText } from './actions.svelte';
import { settings } from './settings.svelte';

export const LIST_INTERVAL = 5000;

export interface KindInfo {
  kind: KubeKind;
  label: string;
  namespaced: boolean;
}

const kind = (kind: KubeKind, label: string, namespaced = true): KindInfo => ({ kind, label, namespaced });

export const KIND_GROUPS: { label: string; kinds: KindInfo[] }[] = [
  {
    label: 'Workloads',
    kinds: [
      kind('pod', 'Pods'),
      kind('deployment', 'Deployments'),
      kind('statefulSet', 'StatefulSets'),
      kind('daemonSet', 'DaemonSets'),
      kind('replicaSet', 'ReplicaSets'),
      kind('job', 'Jobs'),
      kind('cronJob', 'CronJobs'),
    ],
  },
  { label: 'Rede', kinds: [kind('service', 'Services'), kind('ingress', 'Ingresses'), kind('networkPolicy', 'Network Policies')] },
  {
    label: 'Configuração',
    kinds: [kind('configMap', 'ConfigMaps'), kind('secret', 'Secrets'), kind('serviceAccount', 'Service Accounts'), kind('horizontalPodAutoscaler', 'HPAs')],
  },
  {
    label: 'Armazenamento',
    kinds: [kind('persistentVolumeClaim', 'PVCs'), kind('persistentVolume', 'Persistent Volumes', false), kind('storageClass', 'Storage Classes', false)],
  },
  { label: 'Cluster', kinds: [kind('node', 'Nodes', false), kind('namespace', 'Namespaces', false), kind('event', 'Eventos')] },
];

export const kindInfo = (k: KubeKind) => KIND_GROUPS.flatMap((g) => g.kinds).find((i) => i.kind === k)!;

export interface Detail {
  kind: KubeKind;
  namespace: string;
  name: string;
  yaml: string | null;
  error: string;
  /** Secrets: mostrar os valores. */
  reveal: boolean;
}

export const k8s = $state({
  config: null as KubeContexts | null,
  configError: '',
  context: '',
  /** '' = todos. */
  namespace: '',
  namespaces: [] as string[],
  /** Sem permissão para listar namespaces: dá para digitar um. */
  namespacesError: '',
  kind: 'pod' as KubeKind,
  table: null as KubeTable | null,
  /** O que `table` mostra (contexto, namespace e tipo). */
  tableKey: '',
  listError: '',
  loading: false,
  /** Colunas extras (`-o wide`). */
  wide: false,
  detail: null as Detail | null,
});

// Última escolha (por contexto, o namespace), só como conveniência desta máquina.
const STORE = 'nexo.k8s';
type Saved = { context?: string; kind?: KubeKind; namespaces?: Record<string, string> };

function saved(): Saved {
  try {
    return JSON.parse(localStorage.getItem(STORE) ?? '{}') as Saved;
  } catch {
    return {};
  }
}

function remember() {
  const s = saved();
  s.context = k8s.context;
  s.kind = k8s.kind;
  s.namespaces = { ...s.namespaces, [k8s.context]: k8s.namespace };
  try {
    localStorage.setItem(STORE, JSON.stringify(s));
  } catch {
    // Sem armazenamento: só não lembra.
  }
}

const listKey = () => `${k8s.context}\0${k8s.namespace}\0${k8s.kind}`;

/** Relê o kubeconfig e volta ao contexto anterior (ou ao atual do arquivo). */
export async function loadContexts() {
  try {
    k8s.config = await backend.k8sContexts(settings.kubeconfig);
    k8s.configError = '';
  } catch (err) {
    k8s.config = null;
    k8s.configError = errorText(err);
    return;
  }
  const names = k8s.config.contexts.map((c) => c.name);
  const last = saved();
  if (last.kind && kindInfo(last.kind)) k8s.kind = last.kind;
  const pick = [k8s.context, last.context, k8s.config.current, names[0]].find((n) => n && names.includes(n));
  if (pick) selectContext(pick);
}

/** Troca de contexto (no mesmo, só recarrega os namespaces). */
export function selectContext(name: string) {
  const context = k8s.config?.contexts.find((c) => c.name === name);
  if (!context) return;
  if (k8s.context !== name) {
    k8s.context = name;
    k8s.namespace = saved().namespaces?.[name] ?? context.namespace;
  }
  k8s.namespaces = [];
  k8s.namespacesError = '';
  k8s.detail = null;
  remember();
  void loadNamespaces();
}

async function loadNamespaces() {
  const context = k8s.context;
  try {
    const list = await backend.k8sNamespaces(settings.kubeconfig, context);
    if (k8s.context === context) k8s.namespaces = list;
  } catch (err) {
    if (k8s.context === context) k8s.namespacesError = errorText(err);
  }
}

export function selectNamespace(namespace: string) {
  k8s.namespace = namespace;
  remember();
}

export function selectKind(kind: KubeKind) {
  k8s.kind = kind;
  remember();
}

let inFlight = '';

/** Atualiza a lista visível. Respostas que chegam depois de mudar a escolha são descartadas. */
export async function loadList() {
  if (!k8s.context) return;
  const key = listKey();
  if (inFlight === key) return;
  inFlight = key;
  if (k8s.tableKey !== key) k8s.loading = true;
  try {
    const table = await backend.k8sList(settings.kubeconfig, k8s.context, k8s.kind, k8s.namespace);
    if (listKey() !== key) return;
    k8s.table = table;
    k8s.tableKey = key;
    k8s.listError = '';
  } catch (err) {
    if (listKey() !== key) return;
    k8s.listError = errorText(err);
    if (k8s.tableKey !== key) k8s.table = null;
  } finally {
    if (inFlight === key) inFlight = '';
    if (listKey() === key) k8s.loading = false;
  }
}

export function openDetail(kind: KubeKind, namespace: string, name: string) {
  k8s.detail = { kind, namespace, name, yaml: null, error: '', reveal: false };
  void loadYaml();
}

export async function loadYaml() {
  const d = k8s.detail;
  if (!d) return;
  const { context } = k8s;
  try {
    const yaml = await backend.k8sYaml(settings.kubeconfig, context, d.kind, d.namespace, d.name, d.reveal);
    if (k8s.detail === d) Object.assign(d, { yaml, error: '' });
  } catch (err) {
    if (k8s.detail === d) d.error = errorText(err);
  }
}

export function toggleReveal() {
  const d = k8s.detail;
  if (!d) return;
  d.reveal = !d.reveal;
  void loadYaml();
}

/** "7d10h", "5m30s", "2y" → segundos (para ordenar idades). */
export function parseAge(text: string): number | null {
  const units: Record<string, number> = { s: 1, m: 60, h: 3600, d: 86400, y: 365 * 86400 };
  const parts = [...text.matchAll(/(\d+)([smhdy])/g)];
  if (!parts.length || parts.map((p) => p[0]).join('') !== text.trim()) return null;
  return parts.reduce((sum, [, n, u]) => sum + Number(n) * units[u], 0);
}
