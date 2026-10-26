import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  asFailure,
  type ConnectOptions,
  type Credential,
  type CredentialView,
  type SecretUpdate,
  backend,
  type ConnectFailure,
  type Host,
  type HostView,
  type RouteOptions,
  type ShellProfile,
  type Tunnel,
  type TunnelStatus,
} from './backend';
import { leaves, removeLeaf, splitLeaf, type PaneNode, type SplitDir, type SplitNode } from './layout';
import { loadSettings, settings } from './settings.svelte';
import { toggleSftp } from './sftp.svelte';
import { togglePanel } from './side.svelte';
import { RdpPane } from './rdp';
import { TermPane, type PaneSpec } from './term';

export type PaneKind = ShellProfile['kind'] | 'ssh' | 'rdp';

export interface PaneMeta {
  title: string;
  spec: PaneSpec;
  kind: PaneKind;
  exited: boolean;
  /** Fora do broadcast mesmo com o broadcast da aba ligado. */
  muted: boolean;
  latency: { echo?: number; network?: number };
  /** Id da sessão no backend (null antes de conectar e depois de encerrar). */
  sessionId: number | null;
  /** Prompt de senha visível: onde mostrar as sugestões e qual está selecionada. */
  passwordPrompt: { x: number; y: number; index: number } | null;
}

export interface Tab {
  id: string;
  root: PaneNode;
  focused: string;
  broadcast: boolean;
}

export type Dialog =
  | { type: 'hostForm'; host: HostView | null }
  | { type: 'hostKey'; host: HostView; fingerprint: string; keyType: string }
  | { type: 'certChanged'; host: HostView; fingerprint: string }
  | { type: 'secret'; host: HostView; field: 'password' | 'passphrase'; keyPath?: string; error?: string }
  | { type: 'prompt'; title: string; label: string; value: string; confirm: string }
  | { type: 'confirm'; title: string; message: string; confirm: string; danger?: boolean }
  | { type: 'tunnelForm'; tunnel: Tunnel | null; hostId?: string }
  | { type: 'sshImport' }
  | { type: 'credentialForm'; credential: CredentialView | null }
  | { type: 'generateKey' }
  | { type: 'publicKey'; label: string; publicKey: string }
  /** Busca rápida (Ctrl+K): conectar a um host, abrir terminal, ir para uma página. */
  | { type: 'palette' };

/** Página principal: as de gerenciamento, ou as sessões abertas (abas). */
export type View = 'hosts' | 'credentials' | 'tunnels' | 'settings' | 'session';

export const app = $state({
  view: 'hosts' as View,
  profiles: [] as ShellProfile[],
  hosts: [] as HostView[],
  credentials: [] as CredentialView[],
  tabs: [] as Tab[],
  activeTab: '',
  meta: {} as Record<string, PaneMeta>,
  dialog: null as Dialog | null,
  tunnels: [] as Tunnel[],
  /** Só túneis ativos (ou com erro) aparecem aqui. */
  tunnelStatus: {} as Record<string, TunnelStatus>,
  /** Painel em tela cheia ('' = nenhum). */
  fullscreen: '',
});

// Fora do estado reativo de propósito: instâncias de terminal não devem virar proxies.
const panes = new Map<string, TermPane | RdpPane>();
const paneTab = new Map<string, string>();

let seq = 0;
const uid = (prefix: string) => `${prefix}${++seq}`;

export const getPane = (id: string) => panes.get(id);
export const activeTab = () => app.tabs.find((t) => t.id === app.activeTab);
const tabById = (id: string | undefined) => app.tabs.find((t) => t.id === id);

export const localSpec = (profile: ShellProfile): PaneSpec => ({ kind: 'local', profile });
/** Painel de um host salvo: terminal SSH ou área de trabalho RDP. */
export const hostSpec = (host: HostView): PaneSpec => ({ kind: host.protocol === 'rdp' ? 'rdp' : 'ssh', host: $state.snapshot(host) });

/** Spec para um painel novo "igual" ao focado (nova aba, dividir). Uma segunda sessão RDP derrubaria a primeira. */
function cloneSpec(paneId: string | undefined): PaneSpec | undefined {
  const spec = paneId ? app.meta[paneId]?.spec : undefined;
  if (spec?.kind === 'ssh' && spec.exec) return { kind: 'ssh', host: spec.host }; // logs/shell de container: abre o shell do host
  return spec?.kind === 'rdp' ? app.profiles[0] && localSpec(app.profiles[0]) : spec;
}

/** Painel em tela cheia (ocupa a janela inteira, com a janela em tela cheia). */
export async function toggleFullscreen(paneId: string) {
  const on = app.fullscreen !== paneId;
  app.fullscreen = on ? paneId : '';
  if (on) setFocus(paneId);
  await getCurrentWindow().setFullscreen(on);
  queueMicrotask(() => panes.get(paneId)?.focus());
}

export async function init() {
  // Ctrl+Alt+Home num painel RDP com o teclado capturado: o hook já soltou; o foco sai do painel.
  void backend.onRdpRelease(() => {
    for (const pane of panes.values()) if (pane instanceof RdpPane && pane.hasFocus) pane.release();
  });
  void backend.onTunnelStatus(({ id, status }) => {
    if (status) app.tunnelStatus[id] = status;
    else delete app.tunnelStatus[id];
  });
  [app.profiles, app.hosts, app.credentials, app.tunnels, app.tunnelStatus] = await Promise.all([
    backend.listShells(),
    backend.listHosts(),
    backend.listCredentials(),
    backend.listTunnels(),
    backend.tunnelStatuses(),
    loadSettings(),
  ]);
  if (settings.startWithTerminal && app.tabs.length === 0 && app.profiles.length > 0) newTab();
  for (const tunnel of app.tunnels) if (tunnel.autoStart && !app.tunnelStatus[tunnel.id]) void startTunnel(tunnel.id);
}

function createPane(spec: PaneSpec, tabId: string): string {
  const id = uid('p');
  const title = spec.kind === 'local' ? spec.profile.name : spec.kind === 'ssh' && spec.exec ? spec.exec.title : spec.host.label;
  const kind = spec.kind === 'local' ? spec.profile.kind : spec.kind;
  app.meta[id] = { title, spec, kind, exited: false, muted: false, latency: {}, sessionId: null, passwordPrompt: null };
  paneTab.set(id, tabId);
  if (spec.kind === 'rdp') {
    panes.set(
      id,
      new RdpPane(spec, {
        onFocus: () => setFocus(id),
        onSession: (sessionId) => {
          if (!app.meta[id]) return;
          app.meta[id].sessionId = sessionId;
          if (sessionId !== null) app.meta[id].exited = false;
        },
        onExit: () => {
          if (app.meta[id]) app.meta[id].exited = true;
        },
        onFullscreen: () => toggleFullscreen(id),
        onClose: () => closePane(id),
        resolveSsh,
        hostLabel: (hostId) => app.hosts.find((h) => h.id === hostId)?.label,
      }),
    );
    return id;
  }
  panes.set(
    id,
    new TermPane(spec, {
      onInput: (data) => sendInput(id, data),
      onExit: () => {
        if (app.meta[id]) app.meta[id].exited = true;
      },
      onTitle: (title) => {
        // Logs/shell de container mantêm o nome do container.
        if (app.meta[id] && title && !(spec.kind === 'ssh' && spec.exec)) app.meta[id].title = title;
      },
      onFocus: () => setFocus(id),
      onSession: (sessionId) => {
        if (app.meta[id]) app.meta[id].sessionId = sessionId;
      },
      onLatency: (latency) => {
        if (app.meta[id]) Object.assign(app.meta[id].latency, latency);
      },
      onPasswordPrompt: (at) => {
        const meta = app.meta[id];
        if (meta) meta.passwordPrompt = at && secretCandidates(id).length > 0 ? { ...at, index: 0 } : null;
      },
      onPromptKey: (e) => promptKey(id, e),
      resolveSsh,
      hostLabel: (hostId) => app.hosts.find((h) => h.id === hostId)?.label,
    }),
  );
  return id;
}

/** Caminho quente: cada tecla passa por aqui. */
function sendInput(paneId: string, data: string) {
  const pane = panes.get(paneId);
  if (!(pane instanceof TermPane)) return;
  if (pane.exited) {
    if (data !== '\r') return;
    if (pane.spec.kind === 'ssh') {
      app.meta[paneId].exited = false;
      pane.restart();
    } else closePane(paneId);
    return;
  }
  backend.write(inputTargets(paneId), data);
}

/** Sessões que recebem o que é digitado no painel: ele mesmo, ou todos os da aba no broadcast (menos os excluídos). */
function inputTargets(paneId: string): number[] {
  const tab = tabById(paneTab.get(paneId));
  const ids: number[] = [];
  for (const leaf of tab?.broadcast ? leaves(tab.root) : [paneId]) {
    const target = panes.get(leaf);
    if (target?.sessionId != null && !target.exited && (leaf === paneId || !app.meta[leaf]?.muted)) ids.push(target.sessionId);
  }
  return ids;
}

export function newTab(spec: PaneSpec | undefined = app.profiles[0] && localSpec(app.profiles[0])) {
  if (!spec) return;
  const tabId = uid('t');
  const paneId = createPane(spec, tabId);
  app.tabs.push({ id: tabId, root: { type: 'leaf', id: paneId }, focused: paneId, broadcast: false });
  showTab(tabId);
}

/** Mostra uma aba de sessão (sai das páginas de gerenciamento). */
export function showTab(tabId: string) {
  app.activeTab = tabId;
  app.view = 'session';
}

/** Vai para uma página de gerenciamento (hosts, credenciais…); as sessões continuam abertas. */
export function showView(view: View) {
  app.view = view;
  if (view === 'session' && !app.activeTab && app.tabs[0]) app.activeTab = app.tabs[0].id;
}

export function split(dir: SplitDir) {
  const tab = activeTab();
  if (!tab) return;
  const spec = cloneSpec(tab.focused) ?? localSpec(app.profiles[0]);
  const id = createPane(spec, tab.id);
  tab.root = splitLeaf(tab.root, tab.focused, dir, id);
  tab.focused = id;
}

export function setRatio(node: SplitNode, ratio: number) {
  node.ratio = ratio;
}

export function closePane(paneId: string) {
  const tab = tabById(paneTab.get(paneId));
  if (!tab) return;
  const root = removeLeaf(tab.root, paneId);
  if (!root) return closeTab(tab.id);
  tab.root = root;
  if (tab.focused === paneId) tab.focused = leaves(root)[0];
  disposePane(paneId);
}

export function closeTab(tabId: string) {
  const index = app.tabs.findIndex((t) => t.id === tabId);
  if (index < 0) return;
  const [tab] = app.tabs.splice(index, 1);
  if (app.activeTab === tabId) app.activeTab = app.tabs[Math.min(index, app.tabs.length - 1)]?.id ?? '';
  for (const id of leaves(tab.root)) disposePane(id);
  if (app.tabs.length === 0 && app.view === 'session') app.view = 'hosts';
}

function disposePane(id: string) {
  if (app.fullscreen === id) {
    app.fullscreen = '';
    void getCurrentWindow().setFullscreen(false);
  }
  panes.get(id)?.dispose();
  panes.delete(id);
  paneTab.delete(id);
  delete app.meta[id];
}

export function setFocus(paneId: string) {
  const tab = tabById(paneTab.get(paneId));
  if (!tab) return;
  if (tab.focused !== paneId) tab.focused = paneId;
  if (app.activeTab !== tab.id) app.activeTab = tab.id;
}

export function toggleBroadcast() {
  const tab = activeTab();
  if (tab) tab.broadcast = !tab.broadcast;
}

export function toggleMute(paneId: string) {
  const meta = app.meta[paneId];
  if (meta) meta.muted = !meta.muted;
}

export function cycleTab(delta: number) {
  const n = app.tabs.length;
  if (n < 2) return;
  const index = app.tabs.findIndex((t) => t.id === app.activeTab);
  showTab(app.tabs[(index + delta + n) % n].id);
}

type Direction = 'left' | 'right' | 'up' | 'down';

/** Move o foco para o painel vizinho mais próximo na direção pedida (Alt+setas). */
export function focusDirection(dir: Direction) {
  const tab = activeTab();
  if (!tab) return;
  const rectOf = (id: string) => document.querySelector(`[data-pane="${id}"]`)?.getBoundingClientRect();
  const cur = rectOf(tab.focused);
  if (!cur) return;
  const cx = cur.left + cur.width / 2;
  const cy = cur.top + cur.height / 2;

  let best: string | undefined;
  let bestDist = Infinity;
  for (const id of leaves(tab.root)) {
    const r = id === tab.focused ? undefined : rectOf(id);
    if (!r) continue;
    const inDirection =
      (dir === 'left' && r.right <= cur.left + 1) ||
      (dir === 'right' && r.left >= cur.right - 1) ||
      (dir === 'up' && r.bottom <= cur.top + 1) ||
      (dir === 'down' && r.top >= cur.bottom - 1);
    if (!inDirection) continue;
    const dist = Math.hypot(r.left + r.width / 2 - cx, r.top + r.height / 2 - cy);
    if (dist < bestDist) {
      bestDist = dist;
      best = id;
    }
  }
  if (best) tab.focused = best;
}

const arrowDirections: Record<string, Direction> = {
  ArrowLeft: 'left',
  ArrowRight: 'right',
  ArrowUp: 'up',
  ArrowDown: 'down',
};

// ---------- Senhas salvas em prompts (sudo, su…) ----------

/** Hosts com senha salva, do mais provável ao menos: o próprio host, mesmo usuário, o resto. */
export function secretCandidates(paneId: string): HostView[] {
  const spec = app.meta[paneId]?.spec;
  const own = spec?.kind === 'ssh' ? spec.host : undefined;
  const rank = (h: HostView) => (h.id === own?.id ? 0 : own && h.username === own.username ? 1 : 2);
  return app.hosts
    .filter((h) => h.hasPassword)
    .sort((a, b) => rank(a) - rank(b) || a.label.localeCompare(b.label))
    .slice(0, 8);
}

export async function fillSecret(paneId: string, hostId: string) {
  const pane = panes.get(paneId);
  pane?.dismissPrompt();
  if (pane?.sessionId == null) return;
  pane.focus();
  try {
    // Como digitar: no broadcast, vai para todos os painéis que estão recebendo.
    await backend.sendSecret(inputTargets(paneId), hostId);
  } catch (err) {
    console.error('[nexo]', err);
  }
}

function promptKey(paneId: string, e: KeyboardEvent): boolean {
  const prompt = app.meta[paneId]?.passwordPrompt;
  if (!prompt || e.ctrlKey || e.altKey || e.metaKey) return false;
  const candidates = secretCandidates(paneId);
  switch (e.key) {
    case 'ArrowDown':
      prompt.index = (prompt.index + 1) % candidates.length;
      return true;
    case 'ArrowUp':
      prompt.index = (prompt.index - 1 + candidates.length) % candidates.length;
      return true;
    case 'Tab':
    case 'Enter': {
      const host = candidates[prompt.index];
      if (host) void fillSecret(paneId, host.id);
      return true;
    }
    case 'Escape':
      panes.get(paneId)?.dismissPrompt();
      return true;
    default:
      return false;
  }
}

// ---------- Hosts ----------

export async function refreshHosts() {
  app.hosts = await backend.listHosts();
}

export async function saveHost(host: Host, password?: string, passphrase?: string): Promise<HostView | undefined> {
  const saved = await backend.saveHost(host, password, passphrase);
  // "Usada por" das credenciais muda junto.
  await Promise.all([refreshHosts(), refreshCredentials()]);
  return app.hosts.find((h) => h.id === saved.id);
}

// ---------- Credenciais ----------

export async function refreshCredentials() {
  app.credentials = await backend.listCredentials();
}

export async function saveCredential(credential: Credential, secrets: SecretUpdate) {
  const saved = await backend.saveCredential(credential, secrets);
  // Os hosts mostram se há senha salva, e isso pode vir da credencial.
  await Promise.all([refreshCredentials(), refreshHosts()]);
  return saved;
}

export async function deleteCredential(id: string) {
  await backend.deleteCredential(id);
  await Promise.all([refreshCredentials(), refreshHosts()]);
}

export async function generateKey(label: string, username: string) {
  const view = await backend.generateKey(label, username);
  await refreshCredentials();
  return view;
}

export async function deleteHost(id: string) {
  await backend.deleteHost(id);
  // Os túneis do host são apagados junto.
  for (const t of app.tunnels) if (t.hostId === id) stopTunnel(t.id);
  await Promise.all([refreshHosts(), refreshTunnels()]);
}

export function connectHost(host: HostView) {
  newTab(hostSpec(host));
}

// ---------- Túneis ----------

export async function refreshTunnels() {
  app.tunnels = await backend.listTunnels();
}

export async function saveTunnel(tunnel: Tunnel, start: boolean) {
  const wasRunning = !!app.tunnelStatus[tunnel.id];
  const saved = await backend.saveTunnel(tunnel);
  await refreshTunnels();
  // Salvar para o túnel (a configuração mudou); volta a iniciar se estava rodando.
  if (start || wasRunning) void startTunnel(saved.id);
  return saved;
}

export async function deleteTunnel(id: string) {
  await backend.deleteTunnel(id);
  await refreshTunnels();
}

/** Inicia um túnel, pedindo senha/fingerprint quando preciso (mesmo fluxo do terminal). */
export async function startTunnel(id: string) {
  const tunnel = app.tunnels.find((t) => t.id === id);
  const host = app.hosts.find((h) => h.id === tunnel?.hostId);
  if (!tunnel || !host) return;
  app.tunnelStatus[id] = { state: 'connecting' };
  let options: RouteOptions = {};
  for (;;) {
    try {
      await backend.startTunnel(id, options);
      return;
    } catch (err) {
      const error = asFailure(err, host.id);
      const next = await resolveSsh(error, options);
      if (!next) {
        app.tunnelStatus[id] = { state: 'error', message: 'message' in error ? error.message : 'conexão cancelada' };
        return;
      }
      options = next;
    }
  }
}

export function stopTunnel(id: string) {
  delete app.tunnelStatus[id];
  void backend.stopTunnel(id);
}

// ---------- Diálogos ----------
// Um diálogo por vez; pedidos simultâneos (ex.: vários painéis conectando) entram em fila.

let dialogQueue: Promise<unknown> = Promise.resolve();
let dialogResolve: ((value: unknown) => void) | null = null;

export function openDialog<T>(dialog: Dialog): Promise<T> {
  const result = dialogQueue.then(
    () =>
      new Promise<T>((resolve) => {
        app.dialog = dialog;
        dialogResolve = resolve as (value: unknown) => void;
      }),
  );
  dialogQueue = result.catch(() => {});
  return result;
}

export function closeDialog(value: unknown = null) {
  app.dialog = null;
  const resolve = dialogResolve;
  dialogResolve = null;
  resolve?.(value);
  const focused = activeTab()?.focused;
  if (focused && app.view === 'session') queueMicrotask(() => panes.get(focused)?.focus());
}

/**
 * Traduz erros de conexão em perguntas ao usuário. O erro diz em qual host do caminho
 * aconteceu (destino ou jump host): a resposta vale só para aquele host.
 */
async function resolveSsh(error: ConnectFailure, all: RouteOptions): Promise<RouteOptions | null> {
  const found = app.hosts.find((h) => h.id === error.hostId);
  if (!found) return null;
  const host = $state.snapshot(found);
  const options = all[host.id] ?? {};
  const next = await resolveHost(error, host, options);
  return next && { ...all, [host.id]: next };
}

async function resolveHost(error: ConnectFailure, host: HostView, options: ConnectOptions): Promise<ConnectOptions | null> {
  switch (error.kind) {
    case 'unknownHost': {
      const ok = await openDialog<boolean>({ type: 'hostKey', host, fingerprint: error.fingerprint, keyType: error.keyType });
      return ok ? { ...options, trustFingerprint: error.fingerprint } : null;
    }
    case 'certChanged': {
      const ok = await openDialog<boolean>({ type: 'certChanged', host, fingerprint: error.fingerprint });
      return ok ? { ...options, trustFingerprint: error.fingerprint } : null;
    }
    case 'needPassword':
    case 'needPassphrase':
    case 'authFailed': {
      if (error.kind === 'authFailed' && host.auth !== 'password') return null;
      const field = error.kind === 'needPassphrase' ? 'passphrase' : 'password';
      const answer = await openDialog<{ value: string; remember: boolean } | null>({
        type: 'secret',
        host,
        field,
        keyPath: error.kind === 'needPassphrase' ? error.keyPath : undefined,
        error: error.kind === 'authFailed' ? error.message : undefined,
      });
      if (!answer) return null;
      if (answer.remember) queueMicrotask(() => void Promise.all([refreshHosts(), refreshCredentials()]));
      return { ...options, [field]: answer.value, remember: answer.remember };
    }
    default:
      return null;
  }
}

/** Atalhos globais. Registrado na fase de captura, então roda antes do xterm ver a tecla. */
export function handleShortcut(e: KeyboardEvent) {
  // Diálogos (bits-ui) tratam o próprio Esc.
  if (app.dialog) return;
  const focused = app.view === 'session' ? activeTab()?.focused : undefined;
  let handled = true;

  if (e.ctrlKey && !e.shiftKey && !e.altKey && e.code === 'KeyK') {
    void openDialog({ type: 'palette' });
  } else if (e.ctrlKey && e.shiftKey && !e.altKey) {
    switch (e.code) {
      case 'KeyT': newTab(cloneSpec(focused)); break;
      case 'KeyW': if (focused) closePane(focused); break;
      case 'KeyD': split('row'); break;
      case 'KeyE': split('col'); break;
      case 'KeyB': toggleBroadcast(); break;
      case 'KeyF': toggleSftp(); break;
      case 'KeyM': togglePanel('host'); break;
      case 'KeyC': if (focused) void panes.get(focused)?.copy(); break;
      case 'KeyV': if (focused) void panes.get(focused)?.paste(); break;
      case 'Tab': cycleTab(-1); break;
      default: handled = false;
    }
  } else if (e.ctrlKey && !e.shiftKey && !e.altKey && e.code === 'Tab') {
    cycleTab(1);
  } else if (e.altKey && !e.ctrlKey && !e.shiftKey && e.key in arrowDirections) {
    focusDirection(arrowDirections[e.key]);
  } else {
    handled = false;
  }

  if (handled) {
    e.preventDefault();
    e.stopPropagation();
  }
}
