<script lang="ts">
  import { untrack } from 'svelte';
  import { toast } from 'svelte-sonner';
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import FileIcon from '@lucide/svelte/icons/file-key';
  import * as Dialog from '$lib/components/ui/dialog/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { Label } from '$lib/components/ui/label/index.js';
  import { Textarea } from '$lib/components/ui/textarea/index.js';
  import Segmented from './Segmented.svelte';
  import { closeDialog, saveCredential } from '../lib/state.svelte';
  import { copyText, errorText } from '../lib/actions.svelte';
  import type { CredentialView } from '../lib/backend';

  let { credential }: { credential: CredentialView | null } = $props();
  const initial = untrack(() => $state.snapshot(credential));

  let form = $state({
    label: initial?.label ?? '',
    username: initial?.username ?? '',
    auth: initial?.auth ?? ('password' as CredentialView['auth']),
    keyPath: initial?.keyPath ?? '',
  });
  /** Onde fica a chave: arquivo no disco ou texto guardado no cofre. */
  let keySource = $state<'file' | 'vault'>(initial?.hasKey ? 'vault' : 'file');
  let password = $state('');
  let passphrase = $state('');
  let privateKey = $state('');
  let error = $state('');
  let busy = $state(false);

  async function browse() {
    const path = await openFile({ title: 'Chave privada', multiple: false, directory: false });
    if (typeof path === 'string') form.keyPath = path;
  }

  async function submit() {
    if (!form.label.trim()) return void (error = 'Dê um nome à credencial.');
    if (form.auth !== 'agent' && !form.username.trim()) return void (error = 'Informe o usuário.');
    busy = true;
    error = '';
    try {
      const key = form.auth === 'key';
      await saveCredential(
        {
          id: initial?.id ?? '',
          label: form.label.trim(),
          username: form.username.trim(),
          auth: form.auth,
          keyPath: key && keySource === 'file' ? form.keyPath.trim() : '',
        },
        {
          password: form.auth === 'password' ? password || undefined : '',
          passphrase: key ? passphrase || undefined : '',
          // Chave do cofre: texto novo troca; trocar para arquivo apaga a guardada.
          privateKey: key && keySource === 'vault' ? privateKey.trim() || undefined : '',
        },
      );
      closeDialog(null);
      toast.success(initial ? 'Credencial salva' : 'Credencial criada');
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }
</script>

<Dialog.Root open onOpenChange={(open) => !open && closeDialog(null)}>
  <Dialog.Content class="sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title>{initial ? 'Editar credencial' : 'Nova credencial'}</Dialog.Title>
      <Dialog.Description>Os segredos ficam no cofre criptografado; nunca são exportados.</Dialog.Description>
    </Dialog.Header>

    <form
      id="credential-form"
      class="grid gap-4"
      onsubmit={(e) => {
        e.preventDefault();
        void submit();
      }}>
      <div class="flex gap-2">
        <div class="grid flex-1 gap-1.5">
          <Label for="c-label">Nome</Label>
          <!-- svelte-ignore a11y_autofocus -->
          <Input id="c-label" bind:value={form.label} placeholder="Produção, root do lab…" autofocus />
        </div>
        <div class="grid flex-1 gap-1.5">
          <Label for="c-user">Usuário</Label>
          <Input id="c-user" bind:value={form.username} placeholder="deploy" spellcheck={false} autocomplete="off" />
        </div>
      </div>

      <Segmented
        value={form.auth}
        options={[
          { value: 'password', label: 'Senha' },
          { value: 'key', label: 'Chave SSH' },
          { value: 'agent', label: 'Agente SSH' },
        ]}
        onchange={(v) => (form.auth = v as typeof form.auth)} />

      {#if form.auth === 'password'}
        <div class="grid gap-1.5">
          <Label for="c-pass">Senha</Label>
          <Input
            id="c-pass"
            type="password"
            bind:value={password}
            placeholder={initial?.hasPassword ? '•••••••• (salva — deixe vazio para manter)' : 'vazio = perguntar ao conectar'}
            autocomplete="new-password" />
          <p class="text-xs text-muted-foreground">Serve para SSH e RDP (login automático no xRDP).</p>
        </div>
      {:else if form.auth === 'agent'}
        <p class="text-xs text-muted-foreground">
          Usa as chaves carregadas no agente do sistema (OpenSSH Authentication Agent ou Pageant no Windows). Nenhum segredo fica no Nexo. Só para SSH.
        </p>
      {:else}
        <Segmented
          value={keySource}
          options={[
            { value: 'file', label: 'Arquivo no disco' },
            { value: 'vault', label: 'Guardar no cofre' },
          ]}
          onchange={(v) => (keySource = v as typeof keySource)} />
        {#if keySource === 'file'}
          <div class="grid gap-1.5">
            <Label for="c-keypath">Arquivo da chave privada</Label>
            <div class="flex gap-2">
              <Input id="c-keypath" bind:value={form.keyPath} placeholder="vazio = ~/.ssh/id_ed25519, id_ecdsa, id_rsa" spellcheck={false} />
              <Button variant="outline" size="icon" title="Escolher arquivo" onclick={browse}><FileIcon /></Button>
            </div>
          </div>
        {:else}
          <div class="grid gap-1.5">
            <Label for="c-key">Chave privada (formato OpenSSH)</Label>
            <Textarea
              id="c-key"
              bind:value={privateKey}
              rows={5}
              class="font-mono text-[11px]"
              spellcheck={false}
              placeholder={initial?.hasKey ? 'Chave guardada no cofre — cole outra para trocar' : '-----BEGIN OPENSSH PRIVATE KEY-----'} />
            {#if initial?.publicKey}
              <button type="button" class="justify-self-start text-xs text-primary hover:underline" onclick={() => copyText(initial.publicKey!, 'Chave pública copiada')}>
                Copiar chave pública
              </button>
            {/if}
          </div>
        {/if}
        <div class="grid gap-1.5">
          <Label for="c-passphrase">Passphrase</Label>
          <Input
            id="c-passphrase"
            type="password"
            bind:value={passphrase}
            placeholder={initial?.hasPassphrase ? '•••••••• (salva)' : 'se a chave tiver'}
            autocomplete="new-password" />
        </div>
      {/if}

      {#if error}<p class="rounded-md bg-destructive/10 px-3 py-2 text-xs text-destructive">{error}</p>{/if}
    </form>

    <Dialog.Footer>
      <Button variant="outline" onclick={() => closeDialog(null)}>Cancelar</Button>
      <Button type="submit" form="credential-form" disabled={busy}>Salvar</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
