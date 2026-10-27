// Atualização automática: procura uma versão nova no latest.json da última release do GitHub,
// avisa com um toast e, com o ok do usuário, baixa, instala e reinicia o Nexo.
import { toast } from 'svelte-sonner';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { app, openDialog, paneCount } from './state.svelte';
import { settings } from './settings.svelte';
import { errorText } from './actions.svelte';

const EVERY = 6 * 60 * 60 * 1000;

export const updater = $state({
  checking: false,
  /** Baixando ou instalando: o botão "Verificar" fica desligado. */
  busy: false,
});

/** A versão já avisada (não repete o toast a cada 6 h). */
let announced = '';
let timer = 0;

/** Verificação periódica (se ligada nas configurações). Chamado ao iniciar e quando a opção muda. */
export function scheduleUpdateChecks() {
  clearInterval(timer);
  if (!settings.checkUpdates) return;
  setTimeout(() => void checkForUpdates(false), 5000);
  timer = window.setInterval(() => void checkForUpdates(false), EVERY);
}

/** `manual`: pedido pelo usuário, avisa também quando não há nada novo ou dá erro. */
export async function checkForUpdates(manual: boolean) {
  if (updater.checking || updater.busy) return;
  updater.checking = true;
  try {
    const update = await check({ timeout: 30_000 });
    if (!update) {
      if (manual) toast.success('O Nexo está atualizado');
      return;
    }
    if (!manual && update.version === announced) return;
    announced = update.version;
    toast.info(`Nexo ${update.version} disponível`, {
      description: `Você está na ${update.currentVersion}.`,
      duration: Number.POSITIVE_INFINITY,
      action: { label: 'Atualizar', onClick: () => void install(update) },
    });
  } catch (err) {
    if (manual) toast.error('Não deu para verificar atualizações', { description: errorText(err) });
    else console.warn('updater:', err);
  } finally {
    updater.checking = false;
  }
}

async function install(update: Update) {
  // Instalar fecha o app: sessões e túneis abertos caem.
  const open = paneCount() + Object.keys(app.tunnelStatus).length;
  if (open) {
    const ok = await openDialog<boolean>({
      type: 'confirm',
      title: `Atualizar para ${update.version}?`,
      message: `O Nexo reinicia no fim, e ${open === 1 ? 'a sessão (ou túnel) aberta cai' : `as ${open} sessões e túneis abertos caem`}.`,
      confirm: 'Atualizar',
    });
    if (!ok) return;
  }
  updater.busy = true;
  const id = toast.loading(`Baixando o Nexo ${update.version}…`, { duration: Number.POSITIVE_INFINITY });
  let total = 0;
  let done = 0;
  try {
    await update.download((e) => {
      if (e.event === 'Started') total = e.data.contentLength ?? 0;
      else if (e.event === 'Progress') {
        done += e.data.chunkLength;
        if (total) toast.loading(`Baixando o Nexo ${update.version}… ${Math.floor((done / total) * 100)}%`, { id });
      }
    });
    toast.loading('Instalando…', { id });
    // Windows: o instalador fecha o app e o abre de novo sozinho; Linux: reinicia aqui.
    await update.install();
    await relaunch();
  } catch (err) {
    updater.busy = false;
    toast.error('A atualização falhou', { id, description: errorText(err), duration: 10_000 });
  }
}
