import { toast } from 'svelte-sonner';
import { backend, type Container, type ContainerAction, type DockerError, type Exec, type HostMetrics, type HostView, type Sudo } from './backend';
import { activeTab, app, newTab, openDialog, refreshCredentials, refreshHosts, saveHost, type PaneKind } from './state.svelte';
import { leaves } from './layout';
import { settings, updateSettings } from './settings.svelte';

/** Terminais em que o painel funciona: SSH, e os locais Linux e WSL (PowerShell e cmd não). */
const SUPPORTED: PaneKind[] = ['ssh', 'unix', 'wsl'];
export const hostPanelSupports = (kind: PaneKind | undefined) => !!kind && SUPPORTED.includes(kind);

/** Painel que o painel do host mostra: o focado, ou o primeiro compatível da aba. */
export function hostTarget(): string | undefined {
  const tab = activeTab();
  if (!tab) return undefined;
  if (hostPanelSupports(app.meta[tab.focused]?.kind)) return tab.focused;
  return leaves(tab.root).find((id) => hostPanelSupports(app.meta[id]?.kind));
}

/** Nome do lugar onde o painel roda: o host SSH ou o terminal local. */
export function paneLabel(paneId: string): string {
  const spec = app.meta[paneId]?.spec;
  return spec?.kind === 'ssh' ? spec.host.label : spec?.kind === 'local' ? spec.profile.name : 'Host';
}

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
  /** Sem permissão no Docker e sem sudo ligado: o painel oferece ligar. */
  offerSudo: boolean;
  /** Senha do sudo digitada (não salva): só em memória, até o painel reconectar. */
  sudoPassword?: string;
  /** O sudo pediu senha e o usuário cancelou: a atualização automática para até "Tentar de novo". */
  paused: boolean;
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
  hostinfo.containers[paneId] = { sessionId, list: null, error: '', busy: {}, offerSudo: false, paused: false };
  return hostinfo.containers[paneId];
}

/** Host salvo do painel (a versão atual, não a cópia de quando ele abriu: o sudo pode ter mudado). */
export function paneHost(paneId: string): HostView | undefined {
  const spec = app.meta[paneId]?.spec;
  return spec?.kind === 'ssh' ? (app.hosts.find((h) => h.id === spec.host.id) ?? spec.host) : undefined;
}

/** O painel usa `sudo docker`? (no host salvo, ou nas configurações para terminais locais). */
export function dockerSudoOn(paneId: string): boolean {
  const spec = app.meta[paneId]?.spec;
  if (spec?.kind === 'local') return settings.dockerSudoLocal.includes(spec.profile.id);
  return !!paneHost(paneId)?.dockerSudo;
}

function sudoFor(paneId: string, state: Containers, extra?: Partial<Sudo>): Sudo | undefined {
  return dockerSudoOn(paneId) ? { hostId: paneHost(paneId)?.id, password: state.sudoPassword, ...extra } : undefined;
}

const isDockerError = (err: unknown): err is DockerError => typeof err === 'object' && err !== null && 'kind' in err;

/**
 * Roda uma chamada ao Docker tratando o sudo: se ele pedir senha, pergunta (uma vez) e tenta de novo.
 * Devolve undefined se o usuário cancelou ou deu erro (o erro fica em `state.error`).
 */
async function withDocker<T>(paneId: string, state: Containers, call: (sudo?: Sudo) => Promise<T>): Promise<T | undefined> {
  let sudo = sudoFor(paneId, state);
  for (;;) {
    try {
      const result = await call(sudo);
      if (sudo?.password) {
        // Salva no cofre: o backend já acha sozinho. Senão, fica só em memória para as próximas chamadas.
        state.sudoPassword = sudo.remember ? undefined : sudo.password;
        if (sudo.remember) void Promise.all([refreshHosts(), refreshCredentials()]);
      }
      Object.assign(state, { error: '', offerSudo: false });
      return result;
    } catch (err) {
      if (hostinfo.containers[paneId] !== state) return undefined;
      if (isDockerError(err) && err.kind === 'needSudoPassword' && sudo) {
        const host = paneHost(paneId);
        state.paused = true;
        const answer = await openDialog<{ value: string; remember: boolean } | null>({
          type: 'secret',
          host,
          field: 'password',
          title: 'Senha do sudo',
          description: `Para rodar o Docker como root em ${host ? `${host.username}@${host.hostname}` : paneLabel(paneId)}.`,
          confirm: 'Continuar',
          error: err.wrong ? 'Senha incorreta.' : undefined,
          // Já há senha salva (a do login): a do sudo é outra e fica só em memória. Terminal
          // local não tem onde salvar.
          noRemember: !host || host.hasPassword,
        });
        if (!answer) {
          state.error = 'O Docker precisa da senha do sudo.';
          return undefined;
        }
        state.paused = false;
        sudo = { ...sudo, password: answer.value, remember: answer.remember };
        continue;
      }
      Object.assign(state, {
        error: isDockerError(err) ? (err.kind === 'needSudoPassword' ? 'O sudo pediu senha.' : err.message) : message(err),
        offerSudo: isDockerError(err) && err.kind === 'permission' && !sudo,
      });
      return undefined;
    }
  }
}

export async function loadContainers(paneId: string, manual = false) {
  const state = containersOf(paneId);
  if (!state || loading.has(paneId) || (state.paused && !manual)) return;
  if (manual) state.paused = false;
  loading.add(paneId);
  try {
    const list = await withDocker(paneId, state, (sudo) => backend.dockerList(state.sessionId, sudo));
    if (list && hostinfo.containers[paneId] === state) state.list = list;
  } finally {
    loading.delete(paneId);
  }
}

/** Liga (com confirmação) ou desliga o `sudo docker` no host do painel (ou no terminal local). */
export async function setDockerSudo(paneId: string, on: boolean) {
  const spec = app.meta[paneId]?.spec;
  if (spec?.kind === 'local') {
    const id = spec.profile.id;
    if (on) {
      const ok = await openDialog<boolean>({
        type: 'confirm',
        title: 'Usar sudo para o Docker?',
        message: `O painel vai rodar "sudo docker" em ${spec.profile.name} — os comandos rodam como root. O Nexo pede a senha do sudo quando precisar (ela fica só na memória).`,
        confirm: 'Usar sudo',
      });
      if (!ok) return;
    }
    const others = settings.dockerSudoLocal.filter((p) => p !== id);
    updateSettings({ dockerSudoLocal: on ? [...others, id] : others });
    return resetContainers(paneId);
  }
  const host = paneHost(paneId);
  if (!host) return;
  if (on) {
    const ok = await openDialog<boolean>({
      type: 'confirm',
      title: 'Usar sudo para o Docker?',
      message:
        `O painel vai rodar "sudo docker" em ${host.label} — os comandos rodam como root. A senha salva do host ` +
        `${host.credentialId ? '(da credencial) ' : ''}é usada para o sudo; sem ela, o Nexo pergunta. Dá para desligar no editor do host.`,
      confirm: 'Usar sudo',
    });
    if (!ok) return;
  }
  const { hasPassword: _p, hasPassphrase: _pp, ...plain } = $state.snapshot(host);
  await saveHost({ ...plain, dockerSudo: on });
  await resetContainers(paneId);
}

async function resetContainers(paneId: string) {
  const state = containersOf(paneId);
  if (state) Object.assign(state, { list: null, error: '', offerSudo: false, sudoPassword: undefined, paused: false });
  await loadContainers(paneId, true);
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
  const error = state.error;
  try {
    const done = await withDocker(paneId, state, (sudo) => backend.dockerAction(state.sessionId, c.id, action, sudo).then(() => true));
    if (done) toast.success(`${c.name} ${verbs[action]}`);
    else if (state.error && state.error !== error) toast.error(`${c.name}: ${state.error}`);
  } finally {
    delete state.busy[c.id];
  }
  await loadContainers(paneId, true);
}

/** Logs (acompanhando) ou shell do container numa aba nova: na conexão SSH do painel ou no terminal local. */
export function openContainerTerminal(paneId: string, c: Container, kind: 'logs' | 'shell') {
  const meta = app.meta[paneId];
  if (!meta || meta.spec.kind === 'rdp' || meta.spec.kind === 'kube') return;
  // Com sudo, o prompt de senha aparece no terminal (no SSH, com a sugestão da senha salva).
  const exec: Exec = { kind: kind === 'logs' ? 'dockerLogs' : 'dockerShell', container: c.id, sudo: dockerSudoOn(paneId) };
  const title = `${kind === 'logs' ? 'logs' : 'shell'}: ${c.name}`;
  if (meta.spec.kind === 'local') newTab({ kind: 'local', profile: meta.spec.profile, exec: { exec, title, parentSession: null } });
  else newTab({ kind: 'ssh', host: meta.spec.host, exec: { exec, title, parentSession: meta.sessionId } });
}
