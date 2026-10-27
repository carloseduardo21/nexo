<script lang="ts">
  import { toast } from 'svelte-sonner';
  import ShieldAlertIcon from '@lucide/svelte/icons/shield-alert';
  import ShieldIcon from '@lucide/svelte/icons/shield';
  import LockIcon from '@lucide/svelte/icons/lock';
  import CopyIcon from '@lucide/svelte/icons/copy';
  import * as Dialog from '$lib/components/ui/dialog/index.js';
  import * as AlertDialog from '$lib/components/ui/alert-dialog/index.js';
  import { Button, buttonVariants } from '$lib/components/ui/button/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { Label } from '$lib/components/ui/label/index.js';
  import { Checkbox } from '$lib/components/ui/checkbox/index.js';
  import HostEditor from './HostEditor.svelte';
  import TunnelForm from './TunnelForm.svelte';
  import SshImport from './SshImport.svelte';
  import CredentialForm from './CredentialForm.svelte';
  import CommandPalette from './CommandPalette.svelte';
  import { app, closeDialog, generateKey, openDialog } from '../lib/state.svelte';
  import { copyText, errorText } from '../lib/actions.svelte';

  let secret = $state('');
  let remember = $state(true);
  let text = $state('');
  let keyLabel = $state('');
  let keyUser = $state('');
  let busy = $state(false);

  // Reinicia os campos a cada novo diálogo.
  $effect(() => {
    const dialog = app.dialog;
    if (dialog?.type === 'secret') {
      secret = '';
      remember = true;
    } else if (dialog?.type === 'prompt') {
      text = dialog.value;
    } else if (dialog?.type === 'generateKey') {
      keyLabel = 'Chave do Nexo';
      keyUser = '';
    }
  });

  /** Seleciona o nome sem a extensão, como no Explorer. */
  function selectName(input: HTMLInputElement) {
    input.focus();
    const dot = input.value.lastIndexOf('.');
    input.setSelectionRange(0, dot > 0 ? dot : input.value.length);
  }

  async function createKey() {
    busy = true;
    try {
      const view = await generateKey(keyLabel.trim() || 'Chave do Nexo', keyUser.trim());
      closeDialog(null);
      toast.success('Chave gerada');
      // Mostra a pública na sequência (a fila de diálogos abre depois deste fechar), para copiar para o authorized_keys.
      if (view.publicKey) void openDialog({ type: 'publicKey', label: view.label, publicKey: view.publicKey });
    } catch (err) {
      toast.error(errorText(err));
    } finally {
      busy = false;
    }
  }

  const onOpenChange = (open: boolean) => !open && closeDialog(null);
</script>

{#if app.dialog?.type === 'hostForm'}
  {#key app.dialog}
    <HostEditor host={app.dialog.host} />
  {/key}
{:else if app.dialog?.type === 'tunnelForm'}
  {#key app.dialog}
    <TunnelForm tunnel={app.dialog.tunnel} hostId={app.dialog.hostId} />
  {/key}
{:else if app.dialog?.type === 'credentialForm'}
  {#key app.dialog}
    <CredentialForm credential={app.dialog.credential} />
  {/key}
{:else if app.dialog?.type === 'sshImport'}
  <SshImport />
{:else if app.dialog?.type === 'palette'}
  <CommandPalette />
{:else if app.dialog?.type === 'hostKey'}
  {@const d = app.dialog}
  <Dialog.Root open {onOpenChange}>
    <Dialog.Content class="sm:max-w-lg">
      <Dialog.Header>
        <Dialog.Title class="flex items-center gap-2"><ShieldIcon class="size-5 text-amber-500" /> Servidor desconhecido</Dialog.Title>
        <Dialog.Description>
          É a primeira conexão com <strong class="text-foreground">{d.host.hostname}:{d.host.port}</strong>. Confira se a impressão digital é mesmo a do servidor antes de confiar.
        </Dialog.Description>
      </Dialog.Header>
      <div class="rounded-lg border bg-muted/40 p-3">
        <div class="text-xs text-muted-foreground">{d.keyType}</div>
        <div class="mt-1 font-mono text-xs break-all select-text">{d.fingerprint}</div>
      </div>
      <p class="text-xs text-muted-foreground">
        {d.host.protocol === 'rdp'
          ? 'Servidores xRDP costumam usar certificado autoassinado. Ao confiar, o Nexo lembra deste certificado e avisa se ele mudar.'
          : 'Ao confiar, a chave é gravada no ~/.ssh/known_hosts (o mesmo do OpenSSH).'}
      </p>
      <Dialog.Footer>
        <Button variant="outline" onclick={() => closeDialog(false)}>Cancelar</Button>
        <!-- svelte-ignore a11y_autofocus -->
        <Button autofocus onclick={() => closeDialog(true)}>Confiar e conectar</Button>
      </Dialog.Footer>
    </Dialog.Content>
  </Dialog.Root>
{:else if app.dialog?.type === 'certChanged'}
  {@const d = app.dialog}
  <AlertDialog.Root open {onOpenChange}>
    <AlertDialog.Content>
      <AlertDialog.Header>
        <AlertDialog.Title class="flex items-center gap-2"><ShieldAlertIcon class="size-5 text-destructive" /> O certificado do servidor mudou</AlertDialog.Title>
        <AlertDialog.Description>
          O certificado de <strong class="text-foreground">{d.host.hostname}:{d.host.port}</strong> não é o da última conexão. Pode ser uma reinstalação (o xRDP gera um novo) ou alguém interceptando a conexão.
        </AlertDialog.Description>
      </AlertDialog.Header>
      <div class="rounded-lg border bg-muted/40 p-3">
        <div class="text-xs text-muted-foreground">Novo certificado (SHA-256)</div>
        <div class="mt-1 font-mono text-xs break-all select-text">{d.fingerprint}</div>
      </div>
      <AlertDialog.Footer>
        <AlertDialog.Cancel onclick={() => closeDialog(false)}>Cancelar</AlertDialog.Cancel>
        <AlertDialog.Action class={buttonVariants({ variant: 'destructive' })} onclick={() => closeDialog(true)}>Confiar no novo certificado</AlertDialog.Action>
      </AlertDialog.Footer>
    </AlertDialog.Content>
  </AlertDialog.Root>
{:else if app.dialog?.type === 'secret'}
  {@const d = app.dialog}
  <Dialog.Root open {onOpenChange}>
    <Dialog.Content class="sm:max-w-sm">
      <Dialog.Header>
        <Dialog.Title>{d.title ?? (d.field === 'password' ? 'Senha' : 'Passphrase da chave')}</Dialog.Title>
        <Dialog.Description>{d.description ?? (d.field === 'password' ? `${d.host?.username}@${d.host?.hostname}` : d.keyPath)}</Dialog.Description>
      </Dialog.Header>
      <form
        id="secret-form"
        class="grid gap-3"
        onsubmit={(e) => {
          e.preventDefault();
          closeDialog({ value: secret, remember: remember && !d.noRemember });
        }}>
        {#if d.error}<p class="rounded-md bg-destructive/10 px-3 py-2 text-xs text-destructive">{d.error}</p>{/if}
        <!-- svelte-ignore a11y_autofocus -->
        <Input type="password" bind:value={secret} autofocus autocomplete="off" />
        {#if !d.noRemember}
          <label class="flex items-center gap-2 text-sm">
            <Checkbox checked={remember} onCheckedChange={(v) => (remember = v === true)} />
            <LockIcon class="size-3.5 text-muted-foreground" />
            Salvar no cofre{d.host?.credentialId ? ' (na credencial)' : ''}
          </label>
        {/if}
      </form>
      <Dialog.Footer>
        <Button variant="outline" onclick={() => closeDialog(null)}>Cancelar</Button>
        <Button type="submit" form="secret-form">{d.confirm ?? 'Conectar'}</Button>
      </Dialog.Footer>
    </Dialog.Content>
  </Dialog.Root>
{:else if app.dialog?.type === 'prompt'}
  {@const d = app.dialog}
  <Dialog.Root open {onOpenChange}>
    <Dialog.Content class="sm:max-w-md">
      <Dialog.Header>
        <Dialog.Title>{d.title}</Dialog.Title>
        <Dialog.Description class="break-all">{d.label}</Dialog.Description>
      </Dialog.Header>
      <form
        id="prompt-form"
        onsubmit={(e) => {
          e.preventDefault();
          closeDialog(text);
        }}>
        <input
          class="h-9 w-full rounded-md border border-input bg-transparent px-2.5 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 dark:bg-input/30"
          bind:value={text}
          use:selectName
          spellcheck="false"
          autocomplete="off" />
      </form>
      <Dialog.Footer>
        <Button variant="outline" onclick={() => closeDialog(null)}>Cancelar</Button>
        <Button type="submit" form="prompt-form" disabled={!text.trim()}>{d.confirm}</Button>
      </Dialog.Footer>
    </Dialog.Content>
  </Dialog.Root>
{:else if app.dialog?.type === 'confirm'}
  {@const d = app.dialog}
  <AlertDialog.Root open {onOpenChange}>
    <AlertDialog.Content>
      <AlertDialog.Header>
        <AlertDialog.Title>{d.title}</AlertDialog.Title>
        <AlertDialog.Description>{d.message}</AlertDialog.Description>
      </AlertDialog.Header>
      <AlertDialog.Footer>
        <AlertDialog.Cancel onclick={() => closeDialog(false)}>Cancelar</AlertDialog.Cancel>
        <AlertDialog.Action class={d.danger ? buttonVariants({ variant: 'destructive' }) : ''} onclick={() => closeDialog(true)}>{d.confirm}</AlertDialog.Action>
      </AlertDialog.Footer>
    </AlertDialog.Content>
  </AlertDialog.Root>
{:else if app.dialog?.type === 'generateKey'}
  <Dialog.Root open {onOpenChange}>
    <Dialog.Content class="sm:max-w-md">
      <Dialog.Header>
        <Dialog.Title>Gerar chave SSH</Dialog.Title>
        <Dialog.Description>Um par ed25519 novo, guardado no cofre. Depois é só copiar a chave pública para o ~/.ssh/authorized_keys do servidor.</Dialog.Description>
      </Dialog.Header>
      <form
        id="key-form"
        class="grid gap-3"
        onsubmit={(e) => {
          e.preventDefault();
          void createKey();
        }}>
        <div class="grid gap-1.5">
          <Label for="k-label">Nome</Label>
          <!-- svelte-ignore a11y_autofocus -->
          <Input id="k-label" bind:value={keyLabel} autofocus />
        </div>
        <div class="grid gap-1.5">
          <Label for="k-user">Usuário</Label>
          <Input id="k-user" bind:value={keyUser} placeholder="deploy" spellcheck={false} autocomplete="off" />
        </div>
      </form>
      <Dialog.Footer>
        <Button variant="outline" onclick={() => closeDialog(null)}>Cancelar</Button>
        <Button type="submit" form="key-form" disabled={busy}>Gerar</Button>
      </Dialog.Footer>
    </Dialog.Content>
  </Dialog.Root>
{:else if app.dialog?.type === 'publicKey'}
  {@const d = app.dialog}
  <Dialog.Root open {onOpenChange}>
    <Dialog.Content class="sm:max-w-lg">
      <Dialog.Header>
        <Dialog.Title>Chave pública — {d.label}</Dialog.Title>
        <Dialog.Description>Cole esta linha no ~/.ssh/authorized_keys do servidor (ou use <code>ssh-copy-id</code>).</Dialog.Description>
      </Dialog.Header>
      <div class="rounded-lg border bg-muted/40 p-3 font-mono text-[11px] break-all select-text">{d.publicKey}</div>
      <Dialog.Footer>
        <Button variant="outline" onclick={() => closeDialog(null)}>Fechar</Button>
        <Button onclick={() => copyText(d.publicKey, 'Chave pública copiada')}><CopyIcon /> Copiar</Button>
      </Dialog.Footer>
    </Dialog.Content>
  </Dialog.Root>
{/if}
