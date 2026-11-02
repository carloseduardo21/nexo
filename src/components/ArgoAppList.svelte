<script lang="ts">
  import { openUrl } from '@tauri-apps/plugin-opener';
  import SearchIcon from '@lucide/svelte/icons/search';
  import RefreshIcon from '@lucide/svelte/icons/refresh-cw';
  import LoaderIcon from '@lucide/svelte/icons/loader-circle';
  import GridIcon from '@lucide/svelte/icons/layout-grid';
  import ListIcon from '@lucide/svelte/icons/list';
  import ExternalIcon from '@lucide/svelte/icons/external-link';
  import FolderGitIcon from '@lucide/svelte/icons/folder-git-2';
  import ServerIcon from '@lucide/svelte/icons/server';
  import ZapIcon from '@lucide/svelte/icons/zap';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import XIcon from '@lucide/svelte/icons/x';
  import RocketIcon from '@lucide/svelte/icons/rocket';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu/index.js';
  import * as ContextMenu from '$lib/components/ui/context-menu/index.js';
  import { cn } from '$lib/utils.js';
  import ArgoStatus, { accent } from './ArgoStatus.svelte';
  import type { ArgoApp } from '../lib/backend';
  import {
    HEALTH,
    SYNC,
    ago,
    appMenu,
    argo,
    clearFilters,
    destination,
    loadApps,
    matches,
    openApp,
    rememberFilters,
    repoShort,
    shortRevision,
    toggle,
    type Layout,
  } from '../lib/argo.svelte';

  const apps = $derived(argo.data?.apps ?? []);
  const shown = $derived(apps.filter((a) => matches(a)));

  /** Projetos: os AppProjects que dá para ler mais os usados pelos apps. */
  const projects = $derived.by(() => {
    const names = new Set([...(argo.data?.projects ?? []).map((p) => p.name), ...apps.map((a) => a.project)]);
    return [...names].sort().map((name) => ({
      name,
      description: argo.data?.projects.find((p) => p.name === name)?.description ?? '',
      count: apps.filter((a) => a.project === name).length,
    }));
  });

  /** Contagens de cada estado entre os apps que passam pelos outros filtros. */
  const countBy = (field: 'health' | 'sync', value: string) => apps.filter((a) => a[field] === value && matches(a, field)).length;
  const healthChips = $derived(HEALTH.map((h) => ({ ...h, count: countBy('health', h.value) })).filter((h) => h.count || argo.health.includes(h.value)));
  const syncChips = $derived(SYNC.map((s) => ({ ...s, count: countBy('sync', s.value) })).filter((s) => s.count || argo.sync.includes(s.value)));
  const filtered = $derived(argo.projects.length + argo.health.length + argo.sync.length > 0 || !!argo.query.trim());

  /** Agrupados por projeto (com um projeto só, sem cabeçalho). */
  const groups = $derived.by(() => {
    const map = new Map<string, ArgoApp[]>();
    for (const app of shown) map.set(app.project, [...(map.get(app.project) ?? []), app]);
    return [...map].sort(([a], [b]) => a.localeCompare(b));
  });

  function setLayout(layout: Layout) {
    argo.layout = layout;
    rememberFilters();
  }

  function lastSync(app: ArgoApp) {
    const op = app.operation;
    if (!op) return '';
    if (op.phase === 'Running') return 'sincronizando agora';
    return `${op.phase === 'Succeeded' ? 'sincronizado' : 'tentou'} ${ago(op.finishedAt || op.startedAt)}`;
  }

  function sourceText(app: ArgoApp) {
    const s = app.sources.find((x) => !x.ref) ?? app.sources[0];
    if (!s) return '';
    const what = s.chart ? `${s.chart}` : s.path || '.';
    return `${repoShort(s.repoUrl)} · ${what}${s.targetRevision ? ` @ ${s.targetRevision}` : ''}${app.sources.length > 1 ? ` (+${app.sources.length - 1})` : ''}`;
  }
</script>

{#snippet menu(app: ArgoApp, context: boolean)}
  {#each appMenu(app, { open: true }) as item, i (i)}
    {#if item && context}
      <ContextMenu.Item onSelect={item.run}>{item.label}</ContextMenu.Item>
    {:else if item}
      <DropdownMenu.Item onSelect={item.run}>{item.label}</DropdownMenu.Item>
    {:else if context}
      <ContextMenu.Separator />
    {:else}
      <DropdownMenu.Separator />
    {/if}
  {/each}
{/snippet}

{#snippet chip(active: boolean, onclick: () => void, what: 'health' | 'sync', value: string, label: string, count: number)}
  <button
    class={cn(
      'inline-flex h-7 items-center gap-1.5 rounded-full border px-2.5 text-xs transition-colors hover:bg-accent',
      active ? 'border-primary bg-primary/10 text-foreground' : 'text-muted-foreground',
    )}
    aria-pressed={active}
    {onclick}>
    <ArgoStatus {what} {value} />
    {label}
    <span class="tabular-nums opacity-70">{count}</span>
  </button>
{/snippet}

<section class="flex min-w-0 flex-1 flex-col">
  <header class="flex h-12 shrink-0 items-center gap-3 border-b px-4">
    <h1 class="font-medium">Applications</h1>
    {#if argo.data}
      <span class="text-xs text-muted-foreground">{filtered ? `${shown.length} de ${apps.length}` : apps.length}</span>
    {/if}
    {#if argo.loading}<LoaderIcon class="size-3.5 animate-spin text-muted-foreground" />{/if}
    <div class="flex-1"></div>
    <div class="relative w-56">
      <SearchIcon class="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
      <Input bind:value={argo.query} placeholder="Nome, repositório, namespace…" class="h-8 pl-8 text-sm" spellcheck={false} />
    </div>
    <div class="flex rounded-md bg-muted p-0.5">
      <button
        class={cn('rounded-[5px] p-1.5 text-muted-foreground hover:text-foreground', argo.layout === 'tiles' && 'bg-background text-foreground shadow-sm')}
        title="Cartões"
        onclick={() => setLayout('tiles')}><GridIcon class="size-3.5" /></button>
      <button
        class={cn('rounded-[5px] p-1.5 text-muted-foreground hover:text-foreground', argo.layout === 'list' && 'bg-background text-foreground shadow-sm')}
        title="Lista"
        onclick={() => setLayout('list')}><ListIcon class="size-3.5" /></button>
    </div>
    {#if argo.data?.url}
      <Button variant="ghost" size="icon-sm" title={`Abrir ${argo.data.url}`} onclick={() => void openUrl(argo.data!.url)}><ExternalIcon /></Button>
    {/if}
    <Button variant="ghost" size="icon-sm" title="Atualizar" onclick={() => void loadApps()}><RefreshIcon /></Button>
  </header>

  {#if argo.data && apps.length}
    <div class="flex flex-wrap items-center gap-1.5 border-b px-4 py-2">
      <DropdownMenu.Root>
        <DropdownMenu.Trigger>
          {#snippet child({ props })}
            <button
              {...props}
              class={cn(
                'inline-flex h-7 items-center gap-1.5 rounded-full border px-2.5 text-xs transition-colors hover:bg-accent',
                argo.projects.length ? 'border-primary bg-primary/10' : 'text-muted-foreground',
              )}>
              <span class="text-muted-foreground">Projeto</span>
              <span class="max-w-48 truncate font-medium text-foreground">
                {argo.projects.length === 0 ? 'todos' : argo.projects.length === 1 ? argo.projects[0] : `${argo.projects.length} escolhidos`}
              </span>
              <ChevronDownIcon class="size-3" />
            </button>
          {/snippet}
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="start" class="max-h-80 w-64">
          {#each projects as p (p.name)}
            <DropdownMenu.CheckboxItem checked={argo.projects.includes(p.name)} onCheckedChange={() => toggle('projects', p.name)}>
              <div class="min-w-0 flex-1">
                <div class="truncate">{p.name}</div>
                {#if p.description}<div class="truncate text-[11px] text-muted-foreground">{p.description}</div>{/if}
              </div>
              <span class="text-xs text-muted-foreground tabular-nums">{p.count}</span>
            </DropdownMenu.CheckboxItem>
          {/each}
          {#if argo.projects.length}
            <DropdownMenu.Separator />
            <DropdownMenu.Item onSelect={() => ((argo.projects = []), rememberFilters())}>Todos os projetos</DropdownMenu.Item>
          {/if}
        </DropdownMenu.Content>
      </DropdownMenu.Root>
      <div class="mx-1 h-4 w-px bg-border"></div>
      {#each healthChips as h (h.value)}
        {@render chip(argo.health.includes(h.value), () => toggle('health', h.value), 'health', h.value, h.label, h.count)}
      {/each}
      <div class="mx-1 h-4 w-px bg-border"></div>
      {#each syncChips as s (s.value)}
        {@render chip(argo.sync.includes(s.value), () => toggle('sync', s.value), 'sync', s.value, s.label, s.count)}
      {/each}
      {#if filtered}
        <Button variant="ghost" size="xs" class="ml-1 text-muted-foreground" onclick={clearFilters}><XIcon />Limpar</Button>
      {/if}
    </div>
  {/if}

  {#if argo.error}
    <div class="border-b bg-destructive/10 px-4 py-2 text-sm text-destructive">{argo.error}</div>
  {/if}

  <div class="min-h-0 flex-1 overflow-auto">
    {#if !argo.data}
      {#if !argo.error}<p class="p-8 text-center text-sm text-muted-foreground">Consultando o Argo CD…</p>{/if}
    {:else if apps.length === 0}
      <div class="flex flex-col items-center gap-2 p-12 text-center">
        <RocketIcon class="size-8 text-muted-foreground" />
        <p class="font-medium">Nenhum Application neste cluster</p>
        <p class="max-w-sm text-sm text-muted-foreground">Os apps do Argo CD aparecem aqui assim que forem criados (ou se o seu usuário puder lê-los).</p>
      </div>
    {:else if shown.length === 0}
      <p class="p-8 text-center text-sm text-muted-foreground">
        Nada corresponde aos filtros. <button class="underline hover:text-foreground" onclick={clearFilters}>Limpar filtros</button>
      </p>
    {:else if argo.layout === 'tiles'}
      <div class="p-4">
        {#each groups as [project, list] (project)}
          {#if groups.length > 1}
            <h2 class="mt-2 mb-2 flex items-baseline gap-2 text-xs font-medium tracking-wide text-muted-foreground uppercase first:mt-0">
              {project}<span class="font-normal normal-case tabular-nums">{list.length}</span>
            </h2>
          {/if}
          <div class="mb-4 grid grid-cols-[repeat(auto-fill,minmax(19rem,1fr))] gap-3">
            {#each list as app (`${app.namespace}/${app.name}`)}
              <ContextMenu.Root>
                <ContextMenu.Trigger>
                  {#snippet child({ props })}
                    <div
                      {...props}
                      role="button"
                      tabindex="0"
                      class={cn(
                        'group flex cursor-pointer flex-col gap-2 rounded-lg border border-l-[3px] bg-card p-3 text-[13px] shadow-xs transition-colors hover:border-primary/50',
                        accent(app.health),
                      )}
                      onclick={() => openApp(app.namespace, app.name)}
                      onkeydown={(e) => e.key === 'Enter' && openApp(app.namespace, app.name)}>
                      <div class="flex items-center gap-2">
                        <span class="min-w-0 flex-1 truncate font-medium" title={app.name}>{app.name}</span>
                        {#if groups.length === 1}
                          <span class="max-w-28 truncate rounded bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground">{app.project}</span>
                        {/if}
                        <DropdownMenu.Root>
                          <DropdownMenu.Trigger>
                            {#snippet child({ props })}
                              <Button
                                {...props}
                                variant="ghost"
                                size="icon-xs"
                                title="Ações"
                                class="-my-1 opacity-0 group-hover:opacity-100 aria-expanded:opacity-100"
                                onclick={(e: MouseEvent) => e.stopPropagation()}>
                                <EllipsisIcon />
                              </Button>
                            {/snippet}
                          </DropdownMenu.Trigger>
                          <DropdownMenu.Content align="end">
                            {@render menu(app, false)}
                          </DropdownMenu.Content>
                        </DropdownMenu.Root>
                      </div>
                      <div class="flex items-center gap-3 text-xs">
                        <ArgoStatus what="health" value={app.health} label />
                        <ArgoStatus what="sync" value={app.sync} label />
                        {#if app.revisions[0]}
                          <span class="font-mono text-[11px] text-muted-foreground">{shortRevision(app.revisions[0])}</span>
                        {/if}
                        <div class="flex-1"></div>
                        {#if app.autoSync}
                          <span class="inline-flex items-center gap-0.5 text-[11px] text-muted-foreground" title={`Auto-sync${app.prune ? ' · prune' : ''}${app.selfHeal ? ' · self-heal' : ''}`}>
                            <ZapIcon class="size-3" />auto
                          </span>
                        {/if}
                      </div>
                      <div class="grid gap-0.5 text-xs text-muted-foreground">
                        <span class="flex min-w-0 items-center gap-1.5" title={app.sources.map((s) => s.repoUrl).join('\n')}>
                          <FolderGitIcon class="size-3 shrink-0" /><span class="truncate">{sourceText(app)}</span>
                        </span>
                        <span class="flex min-w-0 items-center gap-1.5"><ServerIcon class="size-3 shrink-0" /><span class="truncate">{destination(app)}</span></span>
                      </div>
                      <div class="flex items-center gap-2 border-t pt-2 text-[11px] text-muted-foreground">
                        {#if app.operation && ['Running', 'Failed', 'Error', 'Terminating'].includes(app.operation.phase)}
                          <ArgoStatus what="phase" value={app.operation.phase} label class="shrink-0" />
                        {/if}
                        <span class="truncate">{app.counts.total} recursos{app.counts.outOfSync ? ` · ${app.counts.outOfSync} fora de sincronia` : ''}{app.counts.degraded ? ` · ${app.counts.degraded} degradados` : ''}</span>
                        <span class="ml-auto shrink-0">{lastSync(app)}</span>
                      </div>
                    </div>
                  {/snippet}
                </ContextMenu.Trigger>
                <ContextMenu.Content>
                  {@render menu(app, true)}
                </ContextMenu.Content>
              </ContextMenu.Root>
            {/each}
          </div>
        {/each}
      </div>
    {:else}
      <table class="w-full border-separate border-spacing-0 text-[13px]">
        <thead class="sticky top-0 z-10 bg-background">
          <tr>
            {#each ['Nome', 'Saúde', 'Sync', 'Revisão', 'Origem', 'Destino', 'Última operação'] as h (h)}
              <th class="border-b px-3 py-2 text-left text-xs font-medium whitespace-nowrap text-muted-foreground first:pl-4">{h}</th>
            {/each}
            <th class="w-10 border-b"></th>
          </tr>
        </thead>
        <tbody>
          {#each groups as [project, list] (project)}
            {#if groups.length > 1}
              <tr>
                <td colspan="8" class="border-b bg-muted/40 px-4 py-1 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">
                  {project} <span class="font-normal tabular-nums">{list.length}</span>
                </td>
              </tr>
            {/if}
            {#each list as app (`${app.namespace}/${app.name}`)}
              <tr class="group cursor-pointer hover:bg-accent/60" onclick={() => openApp(app.namespace, app.name)}>
                <td class="max-w-64 truncate border-b border-border/50 px-3 py-1.5 pl-4 font-medium" title={app.name}>{app.name}</td>
                <td class="border-b border-border/50 px-3 py-1.5 whitespace-nowrap"><ArgoStatus what="health" value={app.health} label /></td>
                <td class="border-b border-border/50 px-3 py-1.5 whitespace-nowrap">
                  <span class="inline-flex items-center gap-2">
                    <ArgoStatus what="sync" value={app.sync} label />
                    {#if app.autoSync}<ZapIcon class="size-3 text-muted-foreground" />{/if}
                  </span>
                </td>
                <td class="border-b border-border/50 px-3 py-1.5 font-mono text-xs text-muted-foreground">{shortRevision(app.revisions[0] ?? '')}</td>
                <td class="max-w-80 truncate border-b border-border/50 px-3 py-1.5 text-muted-foreground" title={app.sources.map((s) => s.repoUrl).join('\n')}>{sourceText(app)}</td>
                <td class="max-w-56 truncate border-b border-border/50 px-3 py-1.5 text-muted-foreground">{destination(app)}</td>
                <td class="border-b border-border/50 px-3 py-1.5 whitespace-nowrap text-muted-foreground">
                  <span class="inline-flex items-center gap-1.5">
                    {#if app.operation}<ArgoStatus what="phase" value={app.operation.phase} />{/if}
                    {lastSync(app)}
                  </span>
                </td>
                <td class="border-b border-border/50 pr-2 text-right">
                  <DropdownMenu.Root>
                    <DropdownMenu.Trigger>
                      {#snippet child({ props })}
                        <Button
                          {...props}
                          variant="ghost"
                          size="icon-xs"
                          title="Ações"
                          class="opacity-0 group-hover:opacity-100 aria-expanded:opacity-100"
                          onclick={(e: MouseEvent) => e.stopPropagation()}>
                          <EllipsisIcon />
                        </Button>
                      {/snippet}
                    </DropdownMenu.Trigger>
                    <DropdownMenu.Content align="end">
                      {@render menu(app, false)}
                    </DropdownMenu.Content>
                  </DropdownMenu.Root>
                </td>
              </tr>
            {/each}
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
</section>
