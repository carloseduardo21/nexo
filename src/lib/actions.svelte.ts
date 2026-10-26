// Ações com confirmação e aviso (toast), usadas por várias telas.
import { toast } from 'svelte-sonner';
import type { CredentialView, HostView } from './backend';
import { app, deleteCredential, deleteHost, openDialog, saveHost } from './state.svelte';

export const errorText = (err: unknown) => (typeof err === 'string' ? err : err instanceof Error ? err.message : String(err));

export async function confirmDeleteHost(host: HostView) {
  const tunnels = app.tunnels.filter((t) => t.hostId === host.id).length;
  const ok = await openDialog<boolean>({
    type: 'confirm',
    title: `Excluir ${host.label}?`,
    message: tunnels ? `O host e ${tunnels === 1 ? 'o túnel dele' : `os ${tunnels} túneis dele`} serão excluídos.` : 'O host e a senha salva dele serão excluídos.',
    confirm: 'Excluir',
    danger: true,
  });
  if (!ok) return;
  try {
    await deleteHost(host.id);
    toast.success(`${host.label} excluído`);
  } catch (err) {
    toast.error(errorText(err));
  }
}

/** Cópia do host (sem a senha própria; a credencial, se houver, continua a mesma). */
export async function duplicateHost(host: HostView) {
  const { hasPassword: _p, hasPassphrase: _k, ...copy } = $state.snapshot(host);
  try {
    const saved = await saveHost({ ...copy, id: '', label: `${host.label} (cópia)` });
    if (saved) void openDialog({ type: 'hostForm', host: saved });
  } catch (err) {
    toast.error(errorText(err));
  }
}

export async function confirmDeleteCredential(credential: CredentialView) {
  const used = credential.usedBy.length;
  const ok = await openDialog<boolean>({
    type: 'confirm',
    title: `Excluir ${credential.label}?`,
    message: used
      ? `${used === 1 ? 'O host' : `Os ${used} hosts`} ${credential.usedBy.join(', ')} ${used === 1 ? 'continua' : 'continuam'} com o usuário, mas a senha (ou chave) passa a ser pedida ao conectar.`
      : 'A credencial e os segredos dela (senha, chave) serão apagados do cofre.',
    confirm: 'Excluir',
    danger: true,
  });
  if (!ok) return;
  try {
    await deleteCredential(credential.id);
    toast.success(`${credential.label} excluída`);
  } catch (err) {
    toast.error(errorText(err));
  }
}

export async function copyText(text: string, what = 'Copiado') {
  try {
    await navigator.clipboard.writeText(text);
    toast.success(what);
  } catch (err) {
    toast.error(errorText(err));
  }
}
