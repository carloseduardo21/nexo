<script lang="ts">
  import * as Dialog from '$lib/components/ui/dialog/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { Label } from '$lib/components/ui/label/index.js';
  import { backend, type KubePort } from '../lib/backend';
  import { forwards, k8s, kindInfo, startForward } from '../lib/k8s.svelte';
  import { settings } from '../lib/settings.svelte';
  import { cn } from '$lib/utils.js';

  const target = $derived(forwards.dialog!);

  let ports = $state<KubePort[] | null>(null);
  let port = $state('');
  let local = $state('');
  let busy = $state(false);

  $effect(() => {
    const { kind, namespace, name } = target;
    ports = null;
    backend
      .k8sPorts(settings.kubeconfig, k8s.context, kind, namespace, name)
      .then((list) => {
        ports = list;
        if (!port && list[0]) port = String(list[0].port);
      })
      .catch(() => (ports = []));
  });

  const valid = (v: string, allowEmpty: boolean) => (allowEmpty && !v.trim()) || (/^\d+$/.test(v.trim()) && +v > 0 && +v < 65536);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!valid(port, false) || !valid(local, true)) return;
    busy = true;
    const ok = await startForward(target.kind, target.namespace, target.name, +port, local.trim() ? +local : 0);
    busy = false;
    if (ok) forwards.dialog = null;
  }
</script>

<Dialog.Root open onOpenChange={(open) => !open && (forwards.dialog = null)}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>Port-forward</Dialog.Title>
      <Dialog.Description>
        {kindInfo(target.kind).label.replace(/s$/, '')} <span class="font-medium text-foreground">{target.name}</span> em {target.namespace}. A porta fica aberta
        só nesta máquina (127.0.0.1) enquanto o Nexo estiver aberto.
      </Dialog.Description>
    </Dialog.Header>
    <form id="forward-form" class="grid gap-4" onsubmit={submit}>
      <div class="grid gap-1.5">
        <Label for="f-port">Porta {target.kind === 'service' ? 'do service' : 'no pod'}</Label>
        <Input id="f-port" bind:value={port} inputmode="numeric" placeholder="8080" aria-invalid={!!port && !valid(port, false)} />
        {#if ports && ports.length}
          <div class="flex flex-wrap gap-1.5">
            {#each ports as p (p.port)}
              <button
                type="button"
                class={cn('rounded-md border px-2 py-0.5 text-xs hover:bg-accent', String(p.port) === port.trim() && 'border-primary bg-primary/10')}
                onclick={() => (port = String(p.port))}>
                {p.port}{p.name ? ` · ${p.name}` : ''}{p.protocol !== 'TCP' ? ` · ${p.protocol}` : ''}
              </button>
            {/each}
          </div>
        {:else if ports}
          <p class="text-xs text-muted-foreground">Nenhuma porta declarada: digite a porta.</p>
        {/if}
      </div>
      <div class="grid gap-1.5">
        <Label for="f-local">Porta local</Label>
        <Input id="f-local" bind:value={local} inputmode="numeric" placeholder={port ? `${port} (a mesma, se livre)` : 'a mesma'} aria-invalid={!valid(local, true)} />
      </div>
    </form>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (forwards.dialog = null)}>Cancelar</Button>
      <Button type="submit" form="forward-form" disabled={busy || !valid(port, false) || !valid(local, true)}>Abrir</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
