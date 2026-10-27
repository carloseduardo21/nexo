<script lang="ts">
  import BoxIcon from '@lucide/svelte/icons/box';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import LayersIcon from '@lucide/svelte/icons/layers';
  import PlayIcon from '@lucide/svelte/icons/play';
  import ScrollTextIcon from '@lucide/svelte/icons/scroll-text';
  import SearchIcon from '@lucide/svelte/icons/search';
  import SquareIcon from '@lucide/svelte/icons/square';
  import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
  import LoaderIcon from '@lucide/svelte/icons/loader-circle';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { Switch } from '$lib/components/ui/switch/index.js';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu/index.js';
  import * as ContextMenu from '$lib/components/ui/context-menu/index.js';
  import type { Container, ContainerAction } from '../lib/backend';
  import ShieldIcon from '@lucide/svelte/icons/shield-alert';
  import XIcon from '@lucide/svelte/icons/x';
  import {
    CONTAINERS_INTERVAL,
    containerAction,
    hostinfo,
    loadContainers,
    openContainerTerminal,
    dockerSudoOn,
    setDockerSudo,
  } from '../lib/hostinfo.svelte';
  import { copyText } from '../lib/actions.svelte';
  import { cn } from '$lib/utils.js';

  let { paneId, sessionId }: { paneId: string; sessionId: number } = $props();

  let query = $state('');

  const docker = $derived(hostinfo.containers[paneId]);
  const list = $derived(docker?.sessionId === sessionId ? docker.list : null);
  const visible = $derived(
    (list ?? []).filter((c) => {
      const q = query.trim().toLowerCase();
      return (hostinfo.showStopped || c.state !== 'exited') && (!q || [c.name, c.image, c.project].some((v) => v.toLowerCase().includes(q)));
    }),
  );
  /** Agrupados por projeto do Compose (sem projeto primeiro; o backend já ordena). */
  const groups = $derived.by(() => {
    const map = new Map<string, Container[]>();
    for (const c of visible) map.set(c.project, [...(map.get(c.project) ?? []), c]);
    return [...map];
  });
  const running = $derived((list ?? []).filter((c) => c.state === 'running').length);
  const sudo = $derived(dockerSudoOn(paneId));

  $effect(() => {
    const pane = paneId;
    void sessionId;
    void loadContainers(pane);
    const timer = setInterval(() => void loadContainers(pane), CONTAINERS_INTERVAL);
    return () => clearInterval(timer);
  });

  const dot = (s: string) =>
    s === 'running' ? 'bg-emerald-500' : s === 'paused' || s === 'restarting' ? 'bg-amber-500' : s === 'dead' ? 'bg-destructive' : 'bg-muted-foreground/40';

  type Item = { label: string; run: () => void; destructive?: boolean } | null;

  function actions(c: Container): Item[] {
    const act = (action: ContainerAction) => () => void containerAction(paneId, c, action);
    const isRunning = c.state === 'running';
    const all: (Item | false)[] = [
      { label: 'Ver logs', run: () => openContainerTerminal(paneId, c, 'logs') },
      isRunning && { label: 'Abrir shell', run: () => openContainerTerminal(paneId, c, 'shell') },
      null,
      !isRunning && c.state !== 'paused' && { label: 'Iniciar', run: act('start') },
      (isRunning || c.state === 'paused') && { label: 'Reiniciar', run: act('restart') },
      isRunning && { label: 'Pausar', run: act('pause') },
      c.state === 'paused' && { label: 'Retomar', run: act('unpause') },
      (isRunning || c.state === 'restarting') && { label: 'Parar', run: act('stop') },
      null,
      { label: 'Copiar ID', run: () => copyText(c.id) },
      !isRunning && c.state !== 'paused' && { label: 'Remover', run: act('remove'), destructive: true },
    ];
    return all
      .filter((a): a is Item => a !== false)
      // Separador só entre dois grupos com itens.
      .filter((a, i, all) => a !== null || (i > 0 && all[i - 1] !== null && all.slice(i + 1).some((b) => b !== null)));
  }
</script>

<div class="flex shrink-0 flex-col gap-2 border-b p-3">
  <div class="relative">
    <SearchIcon class="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
    <Input bind:value={query} placeholder="Filtrar containers" class="h-8 pl-8 text-sm" spellcheck={false} />
  </div>
  <div class="flex items-center justify-between text-xs text-muted-foreground">
    <span class="flex items-center gap-2">
      {#if sudo}
        <button
          class="flex items-center gap-1 rounded-sm bg-amber-500/15 px-1.5 py-0.5 text-[10px] font-medium text-amber-600 hover:bg-amber-500/25 dark:text-amber-400"
          title="Os comandos rodam com sudo docker. Clique para parar de usar sudo."
          onclick={() => setDockerSudo(paneId, false)}>
          SUDO <XIcon class="size-2.5" />
        </button>
      {/if}
      {list ? `${running} rodando · ${list.length} no total` : ''}
    </span>
    <label class="flex cursor-pointer items-center gap-1.5">
      Mostrar parados
      <Switch bind:checked={hostinfo.showStopped} class="scale-75" />
    </label>
  </div>
</div>

<div class="min-h-0 flex-1 overflow-y-auto">
  {#if docker?.error}
    <div class="flex flex-col items-center gap-3 p-4 text-center">
      <p class="text-sm text-destructive">{docker.error}</p>
      {#if docker.offerSudo}
        <p class="text-xs text-muted-foreground">Se o usuário pode usar sudo, o painel roda <code>sudo docker</code>.</p>
        <Button size="sm" variant="outline" onclick={() => setDockerSudo(paneId, true)}><ShieldIcon />Usar sudo</Button>
      {:else if docker.paused}
        <Button size="sm" variant="outline" onclick={() => loadContainers(paneId, true)}>Tentar de novo</Button>
      {/if}
    </div>
  {/if}
  {#if !list}
    {#if !docker?.error}<p class="p-6 text-center text-sm text-muted-foreground">Consultando o Docker…</p>{/if}
  {:else if visible.length === 0}
    <p class="p-6 text-center text-sm text-muted-foreground">
      {list.length === 0 ? 'Nenhum container neste servidor.' : 'Nenhum container corresponde ao filtro.'}
    </p>
  {:else}
    {#each groups as [project, containers] (project)}
      {#if project}
        <div class="flex items-center gap-1.5 px-3 pt-3 pb-1 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">
          <LayersIcon class="size-3" />{project}
        </div>
      {/if}
      {#each containers as c (c.id)}
        {@const busy = docker?.busy[c.id]}
        {@const items = actions(c)}
        <ContextMenu.Root>
          <ContextMenu.Trigger>
            {#snippet child({ props })}
              <div {...props} class="group relative flex items-center gap-2.5 px-3 py-2 hover:bg-sidebar-accent">
                <span class={cn('size-2 shrink-0 rounded-full', dot(c.state))} title={c.status}></span>
                <div class="min-w-0 flex-1">
                  <div class="flex items-baseline gap-2">
                    <span class="truncate text-sm font-medium" title={c.name}>{c.name}</span>
                    {#if c.cpu != null}
                      <span class="ml-auto shrink-0 text-[11px] text-muted-foreground tabular-nums" title="CPU · memória ({c.memUsage})">
                        {c.cpu.toFixed(1)}% · {c.memUsage.split(' / ')[0]}
                      </span>
                    {/if}
                  </div>
                  <div class="truncate text-xs text-muted-foreground" title={`${c.image}\n${c.status}${c.ports ? `\n${c.ports}` : ''}`}>
                    {c.image} · {c.status}
                  </div>
                </div>
                {#if busy}
                  <LoaderIcon class="size-4 shrink-0 animate-spin text-muted-foreground" />
                {:else}
                  <!-- Sobre a linha (não ocupa espaço do nome); fica visível com o menu aberto. -->
                  <div
                    class="absolute inset-y-0 right-0 flex items-center bg-sidebar-accent pr-2 pl-3 opacity-0 group-hover:opacity-100 has-[[aria-expanded=true]]:opacity-100">
                    <Button variant="ghost" size="icon-sm" title="Ver logs" onclick={() => openContainerTerminal(paneId, c, 'logs')}>
                      <ScrollTextIcon />
                    </Button>
                    {#if c.state === 'running'}
                      <Button variant="ghost" size="icon-sm" title="Abrir shell" onclick={() => openContainerTerminal(paneId, c, 'shell')}>
                        <SquareTerminalIcon />
                      </Button>
                      <Button variant="ghost" size="icon-sm" title="Parar" onclick={() => containerAction(paneId, c, 'stop')}><SquareIcon /></Button>
                    {:else if c.state !== 'paused'}
                      <Button variant="ghost" size="icon-sm" title="Iniciar" onclick={() => containerAction(paneId, c, 'start')}><PlayIcon /></Button>
                    {/if}
                    <DropdownMenu.Root>
                      <DropdownMenu.Trigger>
                        {#snippet child({ props })}
                          <Button {...props} variant="ghost" size="icon-sm" title="Mais ações"><EllipsisIcon /></Button>
                        {/snippet}
                      </DropdownMenu.Trigger>
                      <DropdownMenu.Content align="end">
                        {#each items as item, i (i)}
                          {#if item}
                            <DropdownMenu.Item variant={item.destructive ? 'destructive' : 'default'} onSelect={item.run}>{item.label}</DropdownMenu.Item>
                          {:else}
                            <DropdownMenu.Separator />
                          {/if}
                        {/each}
                      </DropdownMenu.Content>
                    </DropdownMenu.Root>
                  </div>
                {/if}
              </div>
            {/snippet}
          </ContextMenu.Trigger>
          <ContextMenu.Content>
            {#each items as item, i (i)}
              {#if item}
                <ContextMenu.Item variant={item.destructive ? 'destructive' : 'default'} onSelect={item.run}>{item.label}</ContextMenu.Item>
              {:else}
                <ContextMenu.Separator />
              {/if}
            {/each}
          </ContextMenu.Content>
        </ContextMenu.Root>
      {/each}
    {/each}
    <div class="flex items-center justify-center gap-1.5 p-3 text-[11px] text-muted-foreground/70">
      <BoxIcon class="size-3" />Atualiza a cada {CONTAINERS_INTERVAL / 1000} s
    </div>
  {/if}
</div>
