import { backend, type HostMetrics } from './backend';
import { app } from './state.svelte';

/** Amostras guardadas para os gráficos (2 s cada: 2 minutos). */
const HISTORY = 60;
export const METRICS_INTERVAL = 2000;

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

export const hostinfo = $state({
  tab: 'metrics' as 'metrics' | 'containers',
  monitors: {} as Record<string, Monitor>,
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

export function forgetMonitor(paneId: string) {
  delete hostinfo.monitors[paneId];
}
