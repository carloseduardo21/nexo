<script lang="ts">
  import { open as pickFile } from '@tauri-apps/plugin-dialog';
  import { toast } from 'svelte-sonner';
  import * as Dialog from '$lib/components/ui/dialog/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { Checkbox } from '$lib/components/ui/checkbox/index.js';
  import { Badge } from '$lib/components/ui/badge/index.js';
  import { closeDialog, refreshHosts } from '../lib/state.svelte';
  import { errorText } from '../lib/actions.svelte';
  import { backend, type SshConfigEntry } from '../lib/backend';

  let path = $state('');
  let entries = $state<SshConfigEntry[]>([]);
  let chosen = $state<Record<string, boolean>>({});
  let loading = $state(true);
  let busy = $state(false);
  let error = $state('');

  const count = $derived(entries.filter((e) => chosen[e.alias]).length);

  async function scan(file?: string) {
    loading = true;
    error = '';
    try {
      const result = await backend.sshConfigScan(file);
      path = result.path;
      entries = result.entries;
      // Já cadastrados começam desmarcados.
      chosen = Object.fromEntries(result.entries.map((e) => [e.alias, !e.exists]));
    } catch (err) {
      entries = [];
      error = errorText(err);
    } finally {
      loading = false;
    }
  }

  async function choose() {
    const file = await pickFile({ title: 'Arquivo de configuração do SSH', defaultPath: path || undefined });
    if (typeof file === 'string') await scan(file);
  }

  function toggleAll(on: boolean) {
    chosen = Object.fromEntries(entries.map((e) => [e.alias, on]));
  }

  async function submit() {
    busy = true;
    error = '';
    try {
      const created = await backend.sshConfigImport(path, entries.filter((e) => chosen[e.alias]).map((e) => e.alias));
      await refreshHosts();
      closeDialog(null);
      toast.success(`${created} ${created === 1 ? 'host importado' : 'hosts importados'}`, { description: 'Na pasta "ssh config".' });
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }

  void scan();
</script>

<Dialog.Root open onOpenChange={(open) => !open && closeDialog(null)}>
  <Dialog.Content class="sm:max-w-xl">
    <Dialog.Header>
      <Dialog.Title>Importar do ssh config</Dialog.Title>
      <Dialog.Description>Hosts sem IdentityFile usam as chaves padrão do ~/.ssh e o agente SSH. Jump hosts usados pelos escolhidos vêm junto.</Dialog.Description>
    </Dialog.Header>

    <div class="flex gap-2">
      <Input
        bind:value={path}
        spellcheck={false}
        onkeydown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            void scan(path);
          }
        }} />
      <Button variant="outline" onclick={choose}>Escolher…</Button>
    </div>

    {#if loading}
      <p class="text-muted-foreground">Lendo…</p>
    {:else if entries.length > 0}
      <div class="grid gap-2">
        <label class="flex items-center gap-2 text-xs text-muted-foreground">
          <Checkbox checked={count === entries.length} indeterminate={count > 0 && count < entries.length} onCheckedChange={(v) => toggleAll(v === true)} />
          {count} de {entries.length} selecionados
        </label>
        <ul class="max-h-80 divide-y overflow-y-auto rounded-lg border">
          {#each entries as entry (entry.alias)}
            <li>
              <label class="flex cursor-pointer items-center gap-3 px-3 py-2 hover:bg-accent">
                <Checkbox checked={chosen[entry.alias]} onCheckedChange={(v) => (chosen[entry.alias] = v === true)} />
                <span class="min-w-0 flex-1">
                  <span class="flex items-center gap-2 font-medium">
                    {entry.alias}
                    {#if entry.exists}<Badge variant="secondary" class="text-[10px]">já cadastrado</Badge>{/if}
                  </span>
                  <span class="block truncate text-xs text-muted-foreground">
                    {entry.user}@{entry.hostname}{entry.port !== 22 ? `:${entry.port}` : ''}
                    {#if entry.proxyJump}· via {entry.proxyJump}{/if}
                    {#if entry.identityFile}· {entry.identityFile}{/if}
                  </span>
                </span>
              </label>
            </li>
          {/each}
        </ul>
      </div>
    {:else if !error}
      <p class="text-muted-foreground">Nenhum host encontrado nesse arquivo.</p>
    {/if}

    {#if error}<p class="rounded-md bg-destructive/10 px-3 py-2 text-xs text-destructive">{error}</p>{/if}

    <Dialog.Footer>
      <Button variant="outline" onclick={() => closeDialog(null)}>Cancelar</Button>
      <Button disabled={busy || count === 0} onclick={submit}>Importar {count || ''}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
