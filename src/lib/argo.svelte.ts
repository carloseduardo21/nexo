// Argo CD na página Kubernetes: a lista de Applications (com filtros), o app aberto e as ações.
import { toast } from 'svelte-sonner';
import { backend, type ArgoAction, type ArgoApp, type ArgoApps, type ArgoDetail, type ArgoHealth, type ArgoNode, type ArgoSync, type ArgoTarget } from './backend';
import { ARGO, actionsFor, k8s, kindKeyFor, openDetail, type MenuItem } from './k8s.svelte';
import { openUrl } from '@tauri-apps/plugin-opener';
import { copyText } from './actions.svelte';
import { settings } from './settings.svelte';
import { errorText } from './actions.svelte';
import { openDialog } from './state.svelte';

export const APPS_INTERVAL = 5000;
/** O app aberto é consultado mais vezes (e mais ainda durante uma operação). */
export const APP_INTERVAL = 4000;
export const BUSY_INTERVAL = 1500;

export type Layout = 'tiles' | 'list';
export type Tab = 'tree' | 'resources' | 'history';

export const argo = $state({
  data: null as ArgoApps | null,
  /** Contexto de onde `data` veio. */
  context: '',
  error: '',
  loading: false,
  /** App aberto (null = a lista). */
  open: null as { namespace: string; name: string } | null,
  detail: null as ArgoDetail | null,
  detailError: '',
  projects: [] as string[],
  health: [] as string[],
  sync: [] as string[],
  query: '',
  layout: 'tiles' as Layout,
  tab: 'tree' as Tab,
  /** Diálogo de sync (`only`: já vem com estes recursos escolhidos). */
  syncDialog: null as null | { namespace: string; name: string; only: ArgoTarget[] },
});

// Filtros e layout por contexto, só como conveniência desta máquina.
const STORE = 'nexo.argo';
type Saved = { layout?: Layout; filters?: Record<string, { projects: string[]; health: string[]; sync: string[] }> };

function saved(): Saved {
  try {
    return JSON.parse(localStorage.getItem(STORE) ?? '{}') as Saved;
  } catch {
    return {};
  }
}

export function rememberFilters() {
  const s = saved();
  s.layout = argo.layout;
  s.filters = { ...s.filters, [k8s.context]: { projects: argo.projects, health: argo.health, sync: argo.sync } };
  try {
    localStorage.setItem(STORE, JSON.stringify(s));
  } catch {
    // Sem armazenamento: só não lembra.
  }
}

/** Ao trocar de contexto: os dados e o app aberto eram do outro cluster. */
function enterContext() {
  if (argo.context === k8s.context) return;
  const s = saved();
  const f = s.filters?.[k8s.context];
  Object.assign(argo, {
    data: null,
    context: k8s.context,
    error: '',
    open: null,
    detail: null,
    detailError: '',
    projects: f?.projects ?? [],
    health: f?.health ?? [],
    sync: f?.sync ?? [],
    query: '',
    layout: s.layout ?? argo.layout,
  });
}

let appsInFlight = '';
export async function loadApps() {
  enterContext();
  const context = k8s.context;
  if (!context || appsInFlight === context) return;
  appsInFlight = context;
  if (!argo.data) argo.loading = true;
  try {
    const data = await backend.k8sArgoApps(settings.kubeconfig, context);
    if (k8s.context !== context) return;
    argo.data = data;
    argo.error = '';
  } catch (err) {
    if (k8s.context === context) argo.error = errorText(err);
  } finally {
    if (appsInFlight === context) appsInFlight = '';
    if (k8s.context === context) argo.loading = false;
  }
}

const appKey = (namespace: string, name: string) => `${k8s.context}\0${namespace}\0${name}`;
let appInFlight = '';

export function openApp(namespace: string, name: string) {
  enterContext();
  argo.open = { namespace, name };
  argo.detail = null;
  argo.detailError = '';
  argo.tab = 'tree';
  k8s.detail = null;
  void loadApp();
}

export function closeApp() {
  argo.open = null;
  argo.detail = null;
  k8s.detail = null;
  void loadApps();
}

export async function loadApp() {
  const open = argo.open;
  if (!open) return;
  const key = appKey(open.namespace, open.name);
  if (appInFlight === key) return;
  appInFlight = key;
  try {
    const detail = await backend.k8sArgoApp(settings.kubeconfig, k8s.context, open.namespace, open.name);
    if (argo.open !== open) return;
    argo.detail = detail;
    argo.detailError = '';
  } catch (err) {
    if (argo.open === open) argo.detailError = errorText(err);
  } finally {
    if (appInFlight === key) appInFlight = '';
  }
}

/** O app na lista atual (o aberto usa o detalhe, mais novo). */
export function currentApp(namespace: string, name: string): ArgoApp | undefined {
  if (argo.detail?.app.name === name && argo.detail.app.namespace === namespace) return argo.detail.app;
  return argo.data?.apps.find((a) => a.name === name && a.namespace === namespace);
}

async function run(app: Pick<ArgoApp, 'namespace' | 'name'>, action: ArgoAction, done: string) {
  try {
    await backend.k8sArgoAction(settings.kubeconfig, k8s.context, app.namespace, app.name, action);
    toast.success(done);
    // O controller leva um instante para pegar o pedido.
    setTimeout(() => void (argo.open ? loadApp() : loadApps()), 400);
    return true;
  } catch (err) {
    toast.error(errorText(err));
    return false;
  }
}

export const refresh = (app: ArgoApp, hard = false) =>
  run(app, { type: 'refresh', hard }, hard ? `${app.name}: comparando de novo, sem cache` : `${app.name}: comparando com o git`);

export function openSync(app: ArgoApp, only: ArgoTarget[] = []) {
  argo.syncDialog = { namespace: app.namespace, name: app.name, only };
}

export const sync = (app: Pick<ArgoApp, 'namespace' | 'name'>, action: Extract<ArgoAction, { type: 'sync' }>) =>
  run(app, action, action.dryRun ? `${app.name}: simulação pedida` : `${app.name}: sincronizando`);

export async function terminate(app: ArgoApp) {
  const ok = await openDialog<boolean>({
    type: 'confirm',
    title: `Encerrar a operação de ${app.name}?`,
    message: 'O que já foi aplicado continua aplicado; o resto da sincronização é interrompido.',
    confirm: 'Encerrar',
    danger: true,
  });
  if (ok) await run(app, { type: 'terminate' }, `${app.name}: encerrando a operação`);
}

export async function rollback(app: ArgoApp, id: number, revision: string) {
  const ok = await openDialog<boolean>({
    type: 'confirm',
    title: `Voltar ${app.name} para ${shortRevision(revision)}?`,
    message: `Aplica os manifests da versão #${id} do histórico. O app fica fora de sincronia com o git até o próximo sync.`,
    confirm: 'Voltar versão',
    danger: true,
  });
  if (ok) await run(app, { type: 'rollback', id, prune: false }, `${app.name}: voltando para ${shortRevision(revision)}`);
}

export async function setAutoSync(app: ArgoApp, enabled: boolean, prune = app.prune, selfHeal = app.selfHeal) {
  if (enabled !== app.autoSync) {
    const ok = await openDialog<boolean>(
      enabled
        ? {
            type: 'confirm',
            title: `Ligar o auto-sync de ${app.name}?`,
            message: 'O Argo CD passa a aplicar sozinho cada mudança do git.',
            confirm: 'Ligar',
          }
        : {
            type: 'confirm',
            title: `Desligar o auto-sync de ${app.name}?`,
            message: 'Mudanças no git só chegam ao cluster com um sync manual. Se o app vier de um app-of-apps com auto-sync, o Argo pode religar.',
            confirm: 'Desligar',
          },
    );
    if (!ok) return;
  }
  await run(app, { type: 'autoSync', enabled, prune, selfHeal }, enabled ? `${app.name}: auto-sync ligado` : `${app.name}: auto-sync desligado`);
}

// ---------- Filtros ----------

/** Passa pelos filtros (`skip`: ignora um deles, para contar as opções dele). */
export function matches(app: ArgoApp, skip?: 'health' | 'sync') {
  if (argo.projects.length && !argo.projects.includes(app.project)) return false;
  if (skip !== 'health' && argo.health.length && !argo.health.includes(app.health)) return false;
  if (skip !== 'sync' && argo.sync.length && !argo.sync.includes(app.sync)) return false;
  const q = argo.query.trim().toLowerCase();
  if (!q) return true;
  const fields = [app.name, app.namespace, app.project, app.destNamespace, app.destName, app.destServer, ...app.sources.flatMap((s) => [s.repoUrl, s.path, s.chart]), ...Object.values(app.labels)];
  return fields.some((f) => f.toLowerCase().includes(q));
}

export function toggle(list: 'projects' | 'health' | 'sync', value: string) {
  argo[list] = argo[list].includes(value) ? argo[list].filter((v) => v !== value) : [...argo[list], value];
  rememberFilters();
}

export function clearFilters() {
  argo.projects = [];
  argo.health = [];
  argo.sync = [];
  argo.query = '';
  rememberFilters();
}

// ---------- Texto ----------

export const HEALTH: { value: Exclude<ArgoHealth, ''>; label: string }[] = [
  { value: 'Healthy', label: 'Saudável' },
  { value: 'Progressing', label: 'Progredindo' },
  { value: 'Degraded', label: 'Degradado' },
  { value: 'Suspended', label: 'Suspenso' },
  { value: 'Missing', label: 'Ausente' },
  { value: 'Unknown', label: 'Desconhecido' },
];
export const SYNC: { value: Exclude<ArgoSync, ''>; label: string }[] = [
  { value: 'Synced', label: 'Sincronizado' },
  { value: 'OutOfSync', label: 'Fora de sincronia' },
  { value: 'Unknown', label: 'Desconhecido' },
];
export const healthLabel = (h: string) => HEALTH.find((x) => x.value === h)?.label ?? h;
export const syncLabel = (s: string) => SYNC.find((x) => x.value === s)?.label ?? s;
export const PHASES: Record<string, string> = {
  Running: 'Sincronizando',
  Terminating: 'Encerrando',
  Succeeded: 'Sincronizado',
  Failed: 'Falhou',
  Error: 'Erro',
};

/** Commits viram 7 caracteres; versões de chart ficam como estão. */
export const shortRevision = (r: string) => (/^[0-9a-f]{40}$/.test(r) ? r.slice(0, 7) : r);

/** "há 5 min", "há 3 h"… */
export function ago(iso: string, now = Date.now()) {
  const t = Date.parse(iso);
  if (!iso || isNaN(t)) return '';
  const s = Math.max(0, Math.round((now - t) / 1000));
  if (s < 45) return 'agora';
  if (s < 3600) return `há ${Math.round(s / 60)} min`;
  if (s < 86400) return `há ${Math.round(s / 3600)} h`;
  if (s < 86400 * 60) return `há ${Math.round(s / 86400)} d`;
  return new Date(t).toLocaleDateString();
}

/** "12 s", "3 min 5 s". */
export function duration(from: string, to: string) {
  const [a, b] = [Date.parse(from), Date.parse(to)];
  if (isNaN(a) || isNaN(b)) return '';
  const s = Math.max(0, Math.round((b - a) / 1000));
  return s < 60 ? `${s} s` : `${Math.floor(s / 60)} min${s % 60 ? ` ${s % 60} s` : ''}`;
}

export const fullDate = (iso: string) => (iso && !isNaN(Date.parse(iso)) ? new Date(iso).toLocaleString() : '');

/** Endereço web de um repositório git (https://…, git@host:org/repo.git, ssh://…). */
export function repoWeb(url: string): string | null {
  const ssh = /^(?:ssh:\/\/)?git@([^:/]+)[:/](.+?)(?:\.git)?\/?$/.exec(url);
  if (ssh) return `https://${ssh[1]}/${ssh[2]}`;
  if (/^https?:\/\//.test(url) && !/\.(tgz|tar\.gz)$/.test(url)) return url.replace(/\.git\/?$/, '').replace(/\/$/, '');
  return null;
}

/** org/repo (ou o host, para repositórios de chart). */
export function repoShort(url: string) {
  const web = repoWeb(url);
  if (!web) return url.replace(/^oci:\/\//, '');
  const parts = web.replace(/^https?:\/\//, '').split('/');
  return parts.length >= 3 ? parts.slice(-2).join('/') : parts.join('/');
}

/** Link do commit (GitHub, GitLab, Bitbucket, Gitea). */
export function commitUrl(repo: string, revision: string): string | null {
  const web = repoWeb(repo);
  if (!web || !/^[0-9a-f]{7,40}$/.test(revision)) return null;
  if (web.includes('gitlab')) return `${web}/-/commit/${revision}`;
  if (web.includes('bitbucket.org')) return `${web}/commits/${revision}`;
  return `${web}/commit/${revision}`;
}

/** Link da pasta na revisão (só para fontes git com caminho). */
export function pathUrl(repo: string, revision: string, path: string): string | null {
  const web = repoWeb(repo);
  if (!web || !path) return null;
  const rev = revision || 'HEAD';
  const clean = path.replace(/^\.?\/?/, '');
  if (web.includes('gitlab')) return `${web}/-/tree/${rev}/${clean}`;
  if (web.includes('bitbucket.org')) return `${web}/src/${rev}/${clean}`;
  return `${web}/tree/${rev}/${clean}`;
}

/** Para onde o app aplica: "in-cluster / loja". */
export function destination(app: ArgoApp) {
  const cluster = app.inCluster ? 'in-cluster' : app.destName || app.destServer.replace(/^https?:\/\//, '');
  return app.destNamespace ? `${cluster} / ${app.destNamespace}` : cluster;
}

/** Link do app na interface web do Argo CD. */
export function webUrl(base: string, app: Pick<ArgoApp, 'namespace' | 'name'>) {
  return base ? `${base}/applications/${app.namespace}/${app.name}` : '';
}

/** Ações de um app (menu do cartão, da linha e do cabeçalho). */
export function appMenu(app: ArgoApp, opts: { open?: boolean } = {}): MenuItem[] {
  const url = argo.data?.url;
  return [
    ...(opts.open ? [{ label: 'Abrir', run: () => openApp(app.namespace, app.name) }] : []),
    ...(app.busy ? [] : [{ label: 'Sincronizar…', run: () => openSync(app) }]),
    { label: 'Atualizar', run: () => void refresh(app) },
    { label: 'Atualizar sem cache', run: () => void refresh(app, true) },
    null,
    app.autoSync
      ? { label: 'Desligar auto-sync', run: () => void setAutoSync(app, false) }
      : { label: 'Ligar auto-sync', run: () => void setAutoSync(app, true) },
    ...(app.autoSync
      ? [
          { label: app.prune ? 'Auto-sync sem prune' : 'Auto-sync com prune', run: () => void setAutoSync(app, true, !app.prune, app.selfHeal) },
          { label: app.selfHeal ? 'Desligar self-heal' : 'Ligar self-heal', run: () => void setAutoSync(app, true, app.prune, !app.selfHeal) },
        ]
      : []),
    null,
    ...(url ? [{ label: 'Abrir no Argo CD', run: () => void openUrl(webUrl(url, app)) }] : []),
    { label: 'Copiar nome', run: () => void copyText(app.name) },
  ];
}

// ---------- Recursos do app ----------

export const nodeKey = (n: ArgoNode) => kindKeyFor(n);
export const target = (n: ArgoNode): ArgoTarget => ({ group: n.group, kind: n.kind, namespace: n.namespace, name: n.name });

/** Precisa de atenção: fora de sincronia, sem saúde, com erro ou para remover. */
export const troubled = (n: ArgoNode) =>
  !!n.result || n.requiresPruning || !['Synced', ''].includes(n.sync) || !['Healthy', '', 'Suspended'].includes(n.health);

export function nodeMatches(n: ArgoNode, query: string, problems: boolean) {
  if (problems && !troubled(n)) return false;
  const q = query.trim().toLowerCase();
  return !q || [n.name, n.kind, n.namespace, ...n.info, ...n.images].some((v) => v.toLowerCase().includes(q));
}

/** Menu de um nó da árvore (null = o próprio app). */
export function nodeMenu(app: ArgoApp, n: ArgoNode | null): MenuItem[] {
  if (!n) return [{ label: 'Ver YAML do Application', run: () => openDetail(ARGO, app.namespace, app.name) }, null, ...appMenu(app)];
  const key = nodeKey(n);
  const items: MenuItem[] = [];
  if (key) items.push({ label: 'Ver YAML', run: () => openDetail(key, n.namespace, n.name) });
  if (n.managed && !app.busy) items.push({ label: 'Sincronizar só este…', run: () => openSync(app, [target(n)]) });
  if (key) items.push(null, ...actionsFor(key, n.namespace, n.name));
  else items.push({ label: 'Copiar nome', run: () => void copyText(n.name) });
  return items;
}
