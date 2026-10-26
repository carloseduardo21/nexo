import { toast } from 'svelte-sonner';
import { backend, type Container, type ContainerAction, type HostMetrics } from './backend';
import { app, newTab, openDialog } from './state.svelte';

/** Amostras guardadas para os gráficos (2 s cada: 2 minutos). */
const HISTORY = 60;
export const METRICS_INTERVAL = 2000;
/** `docker stats --no-stream` leva ~1–2 s; mais devagar que as métricas. */
export const CONTAINERS_INTERVAL = 5000;

/** Métricas de um painel SSH: última leitura e o que dá para derivar de duas leituras seguidas. */
export interface Monitor {
  sessionId: number;
  metrics: HostMetrics | null;
  /** Uso de CPU (0–100) entre as duas últimas leituras. */
  cpu: number | null;
  /** Bytes por segundo. */
  rx: number | null;
  tx: number | null;
  cpuHistory: number[];
  memHistory: number[];
  error: string;
  at: number;
}

/** Containers do servidor de um painel SSH. */
export interface Containers {
  sessionId: number;
  list: Container[] | null;
  error: string;
  /** Containers com uma ação em andamento (id → ação). */
  busy: Record<string, ContainerAction>;
}

export const hostinfo = $state({
  tab: 'metrics' as 'metrics' | 'containers',
  showStopped: true,
  monitors: {} as Record<string, Monitor>,
  containers: {} as Record<string, Containers>,
});

const inFlight = new Set<string>();

const message = (err: unknown) => (typeof err === 'string' ? err : err instanceof Error ? err.message : String(err));

const push = (list: number[], value: number) => {
  list.push(value);
  if (list.length > HISTORY) list.shift();
};

/** Lê as métricas do servidor do painel (ignora se a leitura anterior ainda não voltou). */
export async function sample(paneId: string) {
  const sessionId = app.meta[paneId]?.sessionId;
  if (sessionId == null || inFlight.has(paneId)) return;
  let monitor = hostinfo.monitors[paneId];
  if (!monitor || monitor.sessionId !== sessionId) {
    // Primeira leitura, ou o painel reconectou: recomeça (os contadores não batem com os antigos).
    hostinfo.monitors[paneId] = { sessionId, metrics: null, cpu: null, rx: null, tx: null, cpuHistory: [], memHistory: [], error: '', at: 0 };
    monitor = hostinfo.monitors[paneId];
  }
  inFlight.add(paneId);
  try {
    const next = await backend.hostMetrics(sessionId);
    if (hostinfo.monitors[paneId] !== monitor) return;
    const now = performance.now();
    const prev = monitor.metrics;
    if (prev) {
      const total = next.cpuTotal - prev.cpuTotal;
      const busy = total - (next.cpuIdle - prev.cpuIdle);
      monitor.cpu = total > 0 ? Math.max(0, Math.min(100, (busy / total) * 100)) : monitor.cpu;
      const secs = (now - monitor.at) / 1000;
      // Contador zerado (reboot, interface recriada): pula a taxa desta rodada.
      monitor.rx = next.netRx >= prev.netRx ? (next.netRx - prev.netRx) / secs : null;
      monitor.tx = next.netTx >= prev.netTx ? (next.netTx - prev.netTx) / secs : null;
      if (monitor.cpu !== null) push(monitor.cpuHistory, monitor.cpu);
    }
    push(monitor.memHistory, memPercent(next));
    Object.assign(monitor, { metrics: next, at: now, error: '' });
  } catch (err) {
    if (hostinfo.monitors[paneId] === monitor) monitor.error = message(err);
  } finally {
    inFlight.delete(paneId);
  }
}

export const memPercent = (m: HostMetrics) => (m.memTotal > 0 ? ((m.memTotal - m.memAvailable) / m.memTotal) * 100 : 0);

export function forgetPane(paneId: string) {
  delete hostinfo.monitors[paneId];
  delete hostinfo.containers[paneId];
}

// ---------- Containers ----------

const loading = new Set<string>();

/** Containers do painel, criando (ou recriando, se reconectou) o estado. */
function containersOf(paneId: string): Containers | undefined {
  const sessionId = app.meta[paneId]?.sessionId;
  if (sessionId == null) return undefined;
  const current = hostinfo.containers[paneId];
  if (current?.sessionId === sessionId) return current;
  hostinfo.containers[paneId] = { sessionId, list: null, error: '', busy: {} };
  return hostinfo.containers[paneId];
}

export async function loadContainers(paneId: string) {
  const state = containersOf(paneId);
  if (!state || loading.has(paneId)) return;
  loading.add(paneId);
  try {
    const list = await backend.dockerList(state.sessionId);
    if (hostinfo.containers[paneId] === state) Object.assign(state, { list, error: '' });
  } catch (err) {
    if (hostinfo.containers[paneId] === state) state.error = message(err);
  } finally {
    loading.delete(paneId);
  }
}

const verbs: Record<ContainerAction, string> = {
  start: 'iniciado',
  stop: 'parado',
  restart: 'reiniciado',
  pause: 'pausado',
  unpause: 'retomado',
  remove: 'removido',
};

export async function containerAction(paneId: string, c: Container, action: ContainerAction) {
  const state = containersOf(paneId);
  if (!state || state.busy[c.id]) return;
  if (action === 'remove' || action === 'stop') {
    const remove = action === 'remove';
    const ok = await openDialog<boolean>({
      type: 'confirm',
      title: `${remove ? 'Remover' : 'Parar'} ${c.name}?`,
      message: remove
        ? 'O container é apagado do servidor (a imagem e os volumes nomeados continuam).'
        : 'O container recebe SIGTERM e, se não terminar em 10 segundos, SIGKILL.',
      confirm: remove ? 'Remover' : 'Parar',
      danger: true,
    });
    if (!ok) return;
  }
  state.busy[c.id] = action;
  try {
    await backend.dockerAction(state.sessionId, c.id, action);
    toast.success(`${c.name} ${verbs[action]}`);
  } catch (err) {
    toast.error(`${c.name}: ${message(err)}`);
  } finally {
    delete state.busy[c.id];
  }
  await loadContainers(paneId);
}

/** Logs (acompanhando) ou shell do container numa aba nova, na conexão do painel. */
export function openContainerTerminal(paneId: string, c: Container, kind: 'logs' | 'shell') {
  const meta = app.meta[paneId];
  if (meta?.spec.kind !== 'ssh') return;
  newTab({
    kind: 'ssh',
    host: meta.spec.host,
    exec: {
      exec: { kind: kind === 'logs' ? 'dockerLogs' : 'dockerShell', container: c.id },
      title: `${kind === 'logs' ? 'logs' : 'shell'}: ${c.name}`,
      parentSession: meta.sessionId,
    },
  });
}
