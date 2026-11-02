<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity';
  import * as Dialog from '$lib/components/ui/dialog/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Checkbox } from '$lib/components/ui/checkbox/index.js';
  import { cn } from '$lib/utils.js';
  import ArgoStatus from './ArgoStatus.svelte';
  import { kindIcon } from './ArgoTree.svelte';
  import { backend, type ArgoNode } from '../lib/backend';
  import { argo, currentApp, shortRevision, sync, target, troubled } from '../lib/argo.svelte';
  import { k8s } from '../lib/k8s.svelte';
  import { settings } from '../lib/settings.svelte';
  import { errorText } from '../lib/actions.svelte';

  const req = $derived(argo.syncDialog!);
  const app = $derived(currentApp(req.namespace, req.name));

  /** Os gerenciados (do detalhe aberto ou lidos agora, se o sync veio da lista). */
  let resources = $state<ArgoNode[] | null>(null);
  let loadError = $state('');
  const chosen = new SvelteSet<string>();
  const opts = $state({ prune: false, dryRun: false, applyOutOfSyncOnly: false, serverSide: false, replace: false, force: false });
  let busy = $state(false);

  $effect(() => {
    const r = req;
    loadError = '';
    const use = (nodes: ArgoNode[]) => {
      resources = nodes.filter((n) => n.managed);
      chosen.clear();
      const only = new Set(r.only.map((t) => `${t.group}/${t.kind}/${t.namespace}/${t.name}`));
      for (const n of resources) if (!only.size || only.has(n.id)) chosen.add(n.id);
    };
    const open = argo.detail?.app;
    if (open && open.name === r.name && open.namespace === r.namespace) return use(argo.detail!.nodes);
    resources = null;
    backend
      .k8sArgoApp(settings.kubeconfig, k8s.context, r.namespace, r.name)
      .then((d) => argo.syncDialog === r && use(d.nodes))
      .catch((err) => argo.syncDialog === r && (loadError = errorText(err)));
  });

  function set(ids: Iterable<string>) {
    chosen.clear();
    for (const id of ids) chosen.add(id);
  }
  function flip(id: string) {
    if (chosen.has(id)) chosen.delete(id);
    else chosen.add(id);
  }
  const all = $derived(!!resources && chosen.size === resources.length);
  const pruneCount = $derived((resources ?? []).filter((n) => n.requiresPruning && chosen.has(n.id)).length);

  async function submit() {
    if (!resources || !chosen.size || busy) return;
    busy = true;
    const ok = await sync(req, {
      type: 'sync',
      ...opts,
      resources: all ? [] : resources.filter((n) => chosen.has(n.id)).map(target),
    });
    busy = false;
    if (ok) argo.syncDialog = null;
  }

  const OPTIONS = [
    { key: 'prune', label: 'Prune', hint: 'Remove do cluster o que saiu do git.' },
    { key: 'dryRun', label: 'Simular (dry run)', hint: 'Valida sem aplicar nada.' },
    { key: 'applyOutOfSyncOnly', label: 'Só o que está fora de sincronia', hint: 'Não reaplica o que já está igual ao git.' },
    { key: 'serverSide', label: 'Server-side apply', hint: 'O servidor faz o merge (bom para CRDs grandes).' },
    { key: 'replace', label: 'Replace', hint: 'Recria com kubectl replace/create em vez de apply.' },
    { key: 'force', label: 'Forçar', hint: 'Apaga e recria o que não dá para atualizar.' },
  ] as const;
</script>

<Dialog.Root open onOpenChange={(open) => !open && (argo.syncDialog = null)}>
  <Dialog.Content class="flex max-h-[85vh] flex-col sm:max-w-2xl">
    <Dialog.Header>
      <Dialog.Title>Sincronizar {req.name}</Dialog.Title>
      <Dialog.Description>
        {#if app}
          Aplica no cluster o que está em
          <span class="font-medium text-foreground">{app.sources[0]?.targetRevision || 'HEAD'}</span>{#if app.revisions[0]}
            (agora <span class="font-mono">{shortRevision(app.revisions[0])}</span>){/if}.
        {/if}
      </Dialog.Description>
    </Dialog.Header>

    <div class="grid grid-cols-2 gap-x-4 gap-y-2.5">
      {#each OPTIONS as o (o.key)}
        <label class="flex cursor-pointer items-start gap-2 text-sm">
          <Checkbox class="mt-0.5" bind:checked={opts[o.key]} />
          <span>
            <span class={cn(o.key === 'force' && opts.force && 'text-destructive')}>{o.label}</span>
            <span class="block text-xs text-muted-foreground">{o.hint}</span>
          </span>
        </label>
      {/each}
    </div>

    <div class="flex min-h-0 flex-1 flex-col rounded-md border">
      <div class="flex items-center gap-2 border-b px-3 py-1.5 text-xs">
        <span class="font-medium">Recursos</span>
        <span class="text-muted-foreground">{chosen.size} de {resources?.length ?? 0}</span>
        <span class="flex-1"></span>
        <button class="text-muted-foreground hover:text-foreground" onclick={() => set((resources ?? []).map((n) => n.id))}>Todos</button>
        <button class="text-muted-foreground hover:text-foreground" onclick={() => set((resources ?? []).filter(troubled).map((n) => n.id))}>Com diferença</button>
        <button class="text-muted-foreground hover:text-foreground" onclick={() => set([])}>Nenhum</button>
      </div>
      <div class="min-h-0 flex-1 overflow-y-auto py-1">
        {#if loadError}
          <p class="p-3 text-sm text-destructive">{loadError}</p>
        {:else if !resources}
          <p class="p-3 text-sm text-muted-foreground">Lendo os recursos…</p>
        {:else}
          {#each resources as n (n.id)}
            {@const Icon = kindIcon(n.kind)}
            <label class="flex cursor-pointer items-center gap-2.5 px-3 py-1 text-[13px] hover:bg-accent/60">
              <Checkbox checked={chosen.has(n.id)} onCheckedChange={() => flip(n.id)} />
              <Icon class="size-3.5 shrink-0 text-muted-foreground" />
              <span class="w-36 shrink-0 truncate text-xs text-muted-foreground">{n.kind}</span>
              <span class="min-w-0 flex-1 truncate">{n.name}</span>
              {#if n.requiresPruning}<span class="text-[11px] text-amber-600 dark:text-amber-400">remover</span>{/if}
              <ArgoStatus what="sync" value={n.sync} />
            </label>
          {/each}
        {/if}
      </div>
    </div>
    {#if pruneCount && !opts.prune}
      <p class="text-xs text-amber-700 dark:text-amber-400">{pruneCount} recursos saíram do git e só são removidos com Prune.</p>
    {/if}

    <Dialog.Footer>
      <Button variant="outline" onclick={() => (argo.syncDialog = null)}>Cancelar</Button>
      <Button disabled={busy || !chosen.size || !resources} onclick={submit}>
        {opts.dryRun ? 'Simular' : 'Sincronizar'}{resources && !all && chosen.size ? ` (${chosen.size})` : ''}
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
