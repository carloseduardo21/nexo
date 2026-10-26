import { open as pickFiles } from '@tauri-apps/plugin-dialog';
import { revealItemInDir } from '@tauri-apps/plugin-opener';
import { backend, type RemoteEntry, type TransferProgress } from './backend';
import { leaves } from './layout';
import { activeTab, app, openDialog } from './state.svelte';
import { side, togglePanel } from './side.svelte';

/** Estado do navegador de arquivos de um painel SSH. */
export interface Browser {
  sessionId: number;
  path: string;
  entries: RemoteEntry[];
  loading: boolean;
  error: string;
  selected: string[];
  /** Último item clicado, base para Shift+clique. */
  anchor: string | null;
}

export interface Transfer {
  id: number;
  kind: 'download' | 'upload';
  label: string;
  done: number;
  total: number;
  current: string;
  status: 'running' | 'done' | 'error' | 'canceled';
  error?: string;
  /** Downloads: caminhos locais criados (para "mostrar na pasta"). */
  result?: string[];
}

const WIDTH_KEY = 'nexo.sftp.width';

function storedWidth() {
  try {
    const value = Number(localStorage.getItem(WIDTH_KEY));
    return value >= 260 && value <= 900 ? value : 360;
  } catch {
    return 360;
  }
}

export const sftp = $state({
  width: storedWidth(),
  showHidden: false,
  browsers: {} as Record<string, Browser>,
  transfers: [] as Transfer[],
});

export function setWidth(width: number) {
  sftp.width = Math.round(Math.min(900, Math.max(260, width)));
  try {
    localStorage.setItem(WIDTH_KEY, String(sftp.width));
  } catch {
    // sem storage: só não lembra a largura
  }
}

export const toggleSftp = () => togglePanel('sftp');
export const closeSftp = () => (side.panel = null);

/** Painel SSH que os painéis laterais mostram: o focado, ou o primeiro SSH da aba. */
export function sftpTarget(): string | undefined {
  const tab = activeTab();
  if (!tab) return undefined;
  if (app.meta[tab.focused]?.kind === 'ssh') return tab.focused;
  return leaves(tab.root).find((id) => app.meta[id]?.kind === 'ssh');
}

const byName = new Intl.Collator('pt-BR', { numeric: true, sensitivity: 'base' });

function sortEntries(entries: RemoteEntry[]) {
  return entries.sort((a, b) => Number(b.isDir) - Number(a.isDir) || byName.compare(a.name, b.name));
}

const message = (err: unknown) => (typeof err === 'string' ? err : err instanceof Error ? err.message : String(err));

export function parentOf(path: string) {
  const trimmed = path.replace(/\/+$/, '');
  const index = trimmed.lastIndexOf('/');
  return index <= 0 ? '/' : trimmed.slice(0, index);
}

export const joinPath = (dir: string, name: string) => (dir.endsWith('/') ? dir + name : `${dir}/${name}`);

/** Garante um navegador para o painel, ligado à sessão atual (reabre ao reconectar). */
export function ensureBrowser(paneId: string) {
  const sessionId = app.meta[paneId]?.sessionId;
  if (sessionId == null) return;
  const current = sftp.browsers[paneId];
  if (current?.sessionId === sessionId) return;
  // Reconectou: tenta voltar para a mesma pasta.
  const previousPath = current?.path;
  sftp.browsers[paneId] = { sessionId, path: previousPath ?? '', entries: [], loading: true, error: '', selected: [], anchor: null };
  void (async () => {
    try {
      const path = previousPath || (await backend.sftpHome(sessionId));
      await navigate(paneId, path);
    } catch (err) {
      const browser = sftp.browsers[paneId];
      if (browser) Object.assign(browser, { loading: false, error: message(err) });
    }
  })();
}

export function forgetBrowser(paneId: string) {
  delete sftp.browsers[paneId];
}

export async function navigate(paneId: string, path: string, keepSelection = false) {
  const browser = sftp.browsers[paneId];
  if (!browser) return;
  browser.loading = true;
  try {
    const entries = sortEntries(await backend.sftpList(browser.sessionId, path));
    // O painel pode ter sido fechado ou reconectado enquanto esperávamos.
    if (sftp.browsers[paneId] !== browser) return;
    const names = new Set(entries.map((e) => e.path));
    Object.assign(browser, {
      path,
      entries,
      error: '',
      selected: keepSelection ? browser.selected.filter((p) => names.has(p)) : [],
      anchor: keepSelection ? browser.anchor : null,
    });
  } catch (err) {
    browser.error = message(err);
  } finally {
    browser.loading = false;
  }
}

export const refresh = (paneId: string) => {
  const browser = sftp.browsers[paneId];
  if (browser) return navigate(paneId, browser.path, true);
};

export function visibleEntries(browser: Browser) {
  return sftp.showHidden ? browser.entries : browser.entries.filter((e) => !e.name.startsWith('.'));
}

/** Clique com Ctrl alterna, com Shift seleciona intervalo, sem modificador seleciona só um. */
export function select(paneId: string, entry: RemoteEntry, e: MouseEvent) {
  const browser = sftp.browsers[paneId];
  if (!browser) return;
  if (e.shiftKey && browser.anchor) {
    const list = visibleEntries(browser).map((x) => x.path);
    const [a, b] = [list.indexOf(browser.anchor), list.indexOf(entry.path)].sort((x, y) => x - y);
    if (a >= 0) {
      browser.selected = list.slice(a, b + 1);
      return;
    }
  }
  if (e.ctrlKey || e.metaKey) {
    browser.selected = browser.selected.includes(entry.path)
      ? browser.selected.filter((p) => p !== entry.path)
      : [...browser.selected, entry.path];
  } else {
    browser.selected = [entry.path];
  }
  browser.anchor = entry.path;
}

// ---------- Operações ----------

type Prompt = { title: string; label: string; value: string; confirm: string };

const prompt = (p: Prompt) => openDialog<string | null>({ type: 'prompt', ...p });
const confirm = (title: string, text: string, confirmLabel: string) =>
  openDialog<boolean>({ type: 'confirm', title, message: text, confirm: confirmLabel, danger: true });

async function run(paneId: string, action: () => Promise<unknown>) {
  const browser = sftp.browsers[paneId];
  if (!browser) return;
  try {
    await action();
  } catch (err) {
    browser.error = message(err);
  }
  await refresh(paneId);
}

export async function mkdir(paneId: string) {
  const browser = sftp.browsers[paneId];
  if (!browser) return;
  const name = (await prompt({ title: 'Nova pasta', label: `Em ${browser.path}`, value: '', confirm: 'Criar' }))?.trim();
  if (name) await run(paneId, () => backend.sftpMkdir(browser.sessionId, joinPath(browser.path, name)));
}

export async function rename(paneId: string, entry: RemoteEntry) {
  const browser = sftp.browsers[paneId];
  if (!browser) return;
  const name = (await prompt({ title: 'Renomear', label: entry.path, value: entry.name, confirm: 'Renomear' }))?.trim();
  if (name && name !== entry.name) {
    await run(paneId, () => backend.sftpRename(browser.sessionId, entry.path, joinPath(parentOf(entry.path), name)));
  }
}

export async function remove(paneId: string, paths: string[]) {
  const browser = sftp.browsers[paneId];
  if (!browser || paths.length === 0) return;
  const what = paths.length === 1 ? `"${paths[0].split('/').pop()}"` : `${paths.length} itens`;
  const ok = await confirm('Excluir', `Excluir ${what} do servidor? Pastas são apagadas com todo o conteúdo. Não dá para desfazer.`, 'Excluir');
  if (ok) await run(paneId, () => backend.sftpRemove(browser.sessionId, paths));
}

let transferSeq = 0;

function track(kind: Transfer['kind'], label: string): [Transfer, (p: TransferProgress) => void] {
  const id = ++transferSeq;
  sftp.transfers.unshift({ id, kind, label, done: 0, total: 0, current: '', status: 'running' });
  // Pega o proxy reativo (o objeto passado ao unshift não é).
  const transfer = sftp.transfers.find((t) => t.id === id)!;
  return [transfer, (p) => Object.assign(transfer, p)];
}

function finish(transfer: Transfer, err?: unknown) {
  if (err === undefined) {
    transfer.status = 'done';
    transfer.done = transfer.total;
    return;
  }
  const text = message(err);
  transfer.status = text === 'cancelado' ? 'canceled' : 'error';
  transfer.error = text;
}

const labelFor = (paths: string[]) => (paths.length === 1 ? paths[0].split(/[\\/]/).pop()! : `${paths.length} itens`);

/** Baixa para a pasta Downloads, ou para uma pasta escolhida (`choose`). */
export async function download(paneId: string, paths: string[], choose = false) {
  const browser = sftp.browsers[paneId];
  if (!browser || paths.length === 0) return;
  const localDir = choose ? await pickFiles({ directory: true, title: 'Baixar para…' }) : await backend.downloadDir();
  if (!localDir) return;
  const [transfer, onProgress] = track('download', labelFor(paths));
  try {
    transfer.result = await backend.sftpDownload(browser.sessionId, transfer.id, paths, localDir, onProgress);
    finish(transfer);
  } catch (err) {
    finish(transfer, err);
  }
}

/** Envia para a pasta atual do navegador. Sem `paths`, abre o seletor de arquivos. */
export async function upload(paneId: string, paths?: string[]) {
  const browser = sftp.browsers[paneId];
  if (!browser) return;
  if (!paths) {
    const picked = await pickFiles({ multiple: true, title: `Enviar para ${browser.path}` });
    if (!picked) return;
    paths = picked;
  }
  if (paths.length === 0) return;
  const remoteDir = browser.path;
  const [transfer, onProgress] = track('upload', labelFor(paths));
  try {
    await backend.sftpUpload(browser.sessionId, transfer.id, paths, remoteDir, onProgress);
    finish(transfer);
  } catch (err) {
    finish(transfer, err);
  }
  if (sftp.browsers[paneId]?.path === remoteDir) await refresh(paneId);
}

export const cancelTransfer = (transfer: Transfer) => backend.cancelTransfer(transfer.id);

export const revealTransfer = (transfer: Transfer) => {
  if (transfer.result?.[0]) void revealItemInDir(transfer.result[0]).catch(() => {});
};

export function clearTransfers() {
  sftp.transfers = sftp.transfers.filter((t) => t.status === 'running');
}

const shellQuote = (s: string) => `'${s.replace(/'/g, `'\\''`)}'`;

/** Faz o terminal do painel entrar na pasta. */
export function cdInTerminal(paneId: string, path: string) {
  const sessionId = app.meta[paneId]?.sessionId;
  if (sessionId != null) backend.write([sessionId], `cd ${shellQuote(path)}\r`);
}

export const copyText = (text: string) => void navigator.clipboard.writeText(text).catch(() => {});
