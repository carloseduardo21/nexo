<script lang="ts">
  import { openUrl } from '@tauri-apps/plugin-opener';
  import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';
  import RefreshIcon from '@lucide/svelte/icons/refresh-cw';
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import SearchIcon from '@lucide/svelte/icons/search';
  import FolderGitIcon from '@lucide/svelte/icons/folder-git-2';
  import ServerIcon from '@lucide/svelte/icons/server';
  import GitCommitIcon from '@lucide/svelte/icons/git-commit-horizontal';
  import ZapIcon from '@lucide/svelte/icons/zap';
  import StopIcon from '@lucide/svelte/icons/square-stop';
  import UndoIcon from '@lucide/svelte/icons/undo-2';
  import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
  import ExternalIcon from '@lucide/svelte/icons/external-link';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu/index.js';
  import { cn } from '$lib/utils.js';
  import ArgoStatus from './ArgoStatus.svelte';
  import ArgoTree, { kindIcon } from './ArgoTree.svelte';
  import type { ArgoSource } from '../lib/backend';
  import { k8s, openDetail } from '../lib/k8s.svelte';
  import {
    ago,
    argo,
    closeApp,
    commitUrl,
    currentApp,
    destination,
    duration,
    fullDate,
    loadApp,
    nodeKey,
    nodeMatches,
    nodeMenu,
    openSync,
    pathUrl,
    refresh,
    repoShort,
    repoWeb,
    rollback,
    shortRevision,
    target,
    terminate,
    webUrl,
    type Tab,
  } from '../lib/argo.svelte';

  const open = $derived(argo.open!);
  /** O detalhe (mais completo) ou, enquanto carrega, o resumo da lista. */
  const app = $derived(argo.detail?.app ?? currentApp(open.namespace, open.name));
  const nodes = $derived(argo.detail?.nodes ?? []);
  const history = $derived(argo.detail?.history ?? []);
  const managed = $derived(nodes.filter((n) => n.managed));

  let query = $state('');
  let problems = $state(false);
  let showOld = $state(false);
  let includeLive = $state(false);

  const listed = $derived(nodes.filter((n) => (includeLive || n.managed) && (showOld || !n.old) && nodeMatches(n, query, problems)));
  const oldCount = $derived(nodes.filter((n) => n.old).length);

  const TABS: { value: Tab; label: string }[] = [
    { value: 'tree', label: 'Árvore' },
    { value: 'resources', label: 'Recursos' },
    { value: 'history', label: 'Histórico' },
  ];

  const op = $derived(app?.operation ?? null);

  function sourceLinks(s: ArgoSource, revision: string) {
    return {
      repo: repoWeb(s.repoUrl),
      path: s.path ? pathUrl(s.repoUrl, revision || s.targetRevision, s.path) : null,
      commit: commitUrl(s.repoUrl, revision),
    };
  }

  function onKeydown(e: KeyboardEvent) {
    // Esc volta para a lista (o painel de YAML fecha primeiro).
    if (e.key === 'Escape' && !k8s.detail && !e.defaultPrevented && !document.querySelector('[role=dialog],[role=menu]')) closeApp();
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#snippet link(href: string | null, text: string, cls = '')}
  {#if href}
    <button class={cn('truncate hover:text-foreground hover:underline', cls)} title={href} onclick={() => void openUrl(href)}>{text}</button>
  {:else}
    <span class={cn('truncate', cls)}>{text}</span>
  {/if}
{/snippet}

<section class="flex min-w-0 flex-1 flex-col">
  <header class="flex h-12 shrink-0 items-center gap-2 border-b pr-3 pl-2">
    <Button variant="ghost" size="icon-sm" title="Voltar para os Applications (Esc)" onclick={closeApp}><ArrowLeftIcon /></Button>
    <div class="min-w-0">
      <div class="flex items-center gap-2">
        <h1 class="truncate font-medium" title={open.name}>{open.name}</h1>
        {#if app}<span class="truncate rounded bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground">{app.project}</span>{/if}
      </div>
    </div>
    <div class="flex-1"></div>
    {#if app}
      {#if op?.phase === 'Running'}
        <Button variant="outline" size="sm" class="text-destructive" onclick={() => void terminate(app)}><StopIcon />Encerrar</Button>
      {/if}
      <Button size="sm" disabled={app.busy} title={app.busy ? 'Há uma operação em andamento' : 'Aplicar o que está no git'} onclick={() => openSync(app)}>
        Sincronizar
      </Button>
      <div class="flex">
        <Button variant="outline" size="sm" class="rounded-r-none border-r-0" title="Comparar de novo com o git" onclick={() => void refresh(app)}>
          <RefreshIcon />Atualizar
        </Button>
        <DropdownMenu.Root>
          <DropdownMenu.Trigger>
            {#snippet child({ props })}
              <Button {...props} variant="outline" size="icon-sm" class="h-8 w-7 rounded-l-none" title="Mais opções de atualizar"><ChevronDownIcon /></Button>
            {/snippet}
          </DropdownMenu.Trigger>
          <DropdownMenu.Content align="end">
            <DropdownMenu.Item onSelect={() => void refresh(app)}>Atualizar</DropdownMenu.Item>
            <DropdownMenu.Item onSelect={() => void refresh(app, true)}>Atualizar sem cache (hard refresh)</DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Root>
      </div>
      {#if argo.data?.url}
        <Button variant="ghost" size="icon-sm" title="Abrir no Argo CD" onclick={() => void openUrl(webUrl(argo.data!.url, app))}><ExternalIcon /></Button>
      {/if}
      <DropdownMenu.Root>
        <DropdownMenu.Trigger>
          {#snippet child({ props })}
            <Button {...props} variant="ghost" size="icon-sm" title="Ações"><EllipsisIcon /></Button>
          {/snippet}
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="end">
          {#each nodeMenu(app, null) as item, i (i)}
            {#if item}
              <DropdownMenu.Item onSelect={item.run}>{item.label}</DropdownMenu.Item>
            {:else}
              <DropdownMenu.Separator />
            {/if}
          {/each}
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    {/if}
  </header>

  {#if argo.detailError}
    <div class="border-b bg-destructive/10 px-4 py-2 text-sm text-destructive">{argo.detailError}</div>
  {/if}

  {#if !app}
    {#if !argo.detailError}<p class="p-8 text-center text-sm text-muted-foreground">Carregando o app…</p>{/if}
  {:else}
    <div class="shrink-0 space-y-3 border-b p-4">
      <div class="grid gap-3 lg:grid-cols-3">
        <!-- Saúde -->
        <div class="min-w-0 rounded-lg border bg-card p-3">
          <div class="mb-1 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">Saúde</div>
          <ArgoStatus what="health" value={app.health} label class="text-[15px] font-medium" />
          <p class="mt-1 line-clamp-2 text-xs text-muted-foreground" title={app.healthMessage}>
            {app.healthMessage ||
              [
                app.counts.degraded && `${app.counts.degraded} degradados`,
                app.counts.progressing && `${app.counts.progressing} progredindo`,
                app.counts.missing && `${app.counts.missing} ausentes`,
              ]
                .filter(Boolean)
                .join(' · ') ||
              `${app.counts.total} recursos`}
          </p>
        </div>
        <!-- Sincronização -->
        <div class="min-w-0 rounded-lg border bg-card p-3">
          <div class="mb-1 flex items-center gap-2 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">
            Sincronização
            <span class="flex-1"></span>
            {#if app.autoSync}
              <span class="inline-flex items-center gap-0.5 rounded bg-primary/10 px-1.5 py-0.5 text-[10px] text-primary normal-case">
                <ZapIcon class="size-3" />auto{app.prune ? ' · prune' : ''}{app.selfHeal ? ' · self-heal' : ''}
              </span>
            {:else}
              <span class="rounded bg-muted px-1.5 py-0.5 text-[10px] normal-case">manual</span>
            {/if}
          </div>
          <ArgoStatus what="sync" value={app.sync} label class="text-[15px] font-medium" />
          <p class="mt-1 flex min-w-0 items-center gap-1 text-xs text-muted-foreground">
            {#if app.revisions[0]}
              <GitCommitIcon class="size-3 shrink-0" />
              {@render link(app.sources[0] ? commitUrl(app.sources[0].repoUrl, app.revisions[0]) : null, shortRevision(app.revisions[0]), 'font-mono')}
            {/if}
            {#if app.sources[0]?.targetRevision}<span class="truncate">rastreando {app.sources[0].targetRevision}</span>{/if}
            {#if app.counts.outOfSync}<span class="shrink-0 text-amber-600 dark:text-amber-400">· {app.counts.outOfSync} fora</span>{/if}
          </p>
        </div>
        <!-- Última operação -->
        <div class="min-w-0 rounded-lg border bg-card p-3">
          <div class="mb-1 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">Última operação</div>
          {#if op}
            <div class="flex items-center gap-2">
              <ArgoStatus what="phase" value={op.phase} label class="text-[15px] font-medium" />
              {#if op.dryRun}<span class="rounded bg-muted px-1.5 text-[10px]">simulação</span>{/if}
            </div>
            <p class="mt-1 line-clamp-2 text-xs text-muted-foreground" title={op.message}>
              {op.automated ? 'automático' : op.initiatedBy || 'manual'} · <span title={fullDate(op.startedAt)}>{ago(op.startedAt)}</span>{op.finishedAt
                ? ` · levou ${duration(op.startedAt, op.finishedAt)}`
                : ''}{op.message ? ` · ${op.message}` : ''}
            </p>
          {:else}
            <span class="text-[15px] text-muted-foreground">Nenhuma</span>
          {/if}
        </div>
      </div>

      <div class="flex flex-wrap items-center gap-x-5 gap-y-1 text-xs text-muted-foreground">
        {#each app.sources as s, i (i)}
          {@const links = sourceLinks(s, app.revisions[i] ?? '')}
          <span class="flex min-w-0 items-center gap-1.5">
            <FolderGitIcon class="size-3.5 shrink-0" />
            {@render link(links.repo, repoShort(s.repoUrl))}
            {#if s.chart}<span>· chart {s.chart}</span>{/if}
            {#if s.path}<span>·</span>{@render link(links.path, s.path)}{/if}
            {#if s.targetRevision}<span>@ {s.targetRevision}</span>{/if}
            {#if s.ref}<span class="rounded bg-muted px-1">${s.ref}</span>{/if}
          </span>
        {/each}
        <span class="flex items-center gap-1.5" title={app.destServer || app.destName}><ServerIcon class="size-3.5" />{destination(app)}</span>
        {#if !app.inCluster}
          <span class="text-amber-600 dark:text-amber-400" title="O app aplica em outro cluster: a árvore mostra só o que o Argo gerencia, sem os pods.">outro cluster</span>
        {/if}
        <span title={fullDate(app.reconciledAt)}>comparado {ago(app.reconciledAt)}</span>
      </div>

      {#each app.conditions as c, i (i)}
        <div
          class={cn(
            'flex items-start gap-2 rounded-md px-3 py-2 text-xs',
            /Error/.test(c.type) ? 'bg-destructive/10 text-destructive' : 'bg-amber-500/10 text-amber-700 dark:text-amber-400',
          )}>
          <TriangleAlertIcon class="mt-0.5 size-3.5 shrink-0" />
          <span class="min-w-0"><span class="font-medium">{c.type}</span> · {c.message}</span>
        </div>
      {/each}
    </div>

    <div class="flex h-11 shrink-0 items-center gap-1 border-b px-2">
      {#each TABS as t (t.value)}
        <button
          class={cn(
            'relative h-full px-3 text-[13px] text-muted-foreground hover:text-foreground',
            argo.tab === t.value && 'font-medium text-foreground after:absolute after:inset-x-2 after:bottom-0 after:h-0.5 after:rounded-full after:bg-primary',
          )}
          onclick={() => (argo.tab = t.value)}>
          {t.label}
          {#if t.value === 'resources'}<span class="ml-1 text-xs text-muted-foreground tabular-nums">{managed.length}</span>{/if}
          {#if t.value === 'history'}<span class="ml-1 text-xs text-muted-foreground tabular-nums">{history.length}</span>{/if}
        </button>
      {/each}
      <div class="flex-1"></div>
      {#if argo.tab !== 'history'}
        <div class="relative w-48">
          <SearchIcon class="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input bind:value={query} placeholder="Destacar recurso" class="h-7 pl-8 text-xs" spellcheck={false} />
        </div>
        <button
          class={cn('h-7 rounded-md border px-2.5 text-xs hover:bg-accent', problems ? 'border-primary bg-primary/10' : 'text-muted-foreground')}
          aria-pressed={problems}
          title="Só o que está fora de sincronia, sem saúde ou com erro"
          onclick={() => (problems = !problems)}>Com problema</button>
        {#if argo.tab === 'resources'}
          <button
            class={cn('h-7 rounded-md border px-2.5 text-xs hover:bg-accent', includeLive ? 'border-primary bg-primary/10' : 'text-muted-foreground')}
            aria-pressed={includeLive}
            title="Também os ReplicaSets, Jobs e Pods que o cluster criou"
            onclick={() => (includeLive = !includeLive)}>Pods e filhos</button>
        {/if}
        {#if oldCount}
          <button
            class={cn('h-7 rounded-md border px-2.5 text-xs hover:bg-accent', showOld ? 'border-primary bg-primary/10' : 'text-muted-foreground')}
            aria-pressed={showOld}
            title="ReplicaSets de versões anteriores (sem réplicas)"
            onclick={() => (showOld = !showOld)}>Versões antigas {oldCount}</button>
        {/if}
      {/if}
      {#if argo.tab === 'history'}
        <Button variant="ghost" size="icon-sm" title="Recarregar" onclick={() => void loadApp()}><RefreshIcon /></Button>
      {/if}
    </div>

    <div class="min-h-0 flex-1 overflow-auto">
      {#if !argo.detail}
        {#if !argo.detailError}<p class="p-8 text-center text-sm text-muted-foreground">Lendo os recursos…</p>{/if}
      {:else if argo.tab === 'tree'}
        <div class="p-5">
          <ArgoTree {app} {nodes} {query} {problems} {showOld} />
        </div>
      {:else if argo.tab === 'resources'}
        {#if listed.length === 0}
          <p class="p-8 text-center text-sm text-muted-foreground">{nodes.length ? 'Nada corresponde ao filtro.' : 'O app não tem recursos.'}</p>
        {:else}
          <table class="w-full border-separate border-spacing-0 text-[13px]">
            <thead class="sticky top-0 z-10 bg-background">
              <tr>
                {#each ['Tipo', 'Nome', 'Namespace', 'Sync', 'Saúde', 'Detalhes'] as h (h)}
                  <th class="border-b px-3 py-2 text-left text-xs font-medium whitespace-nowrap text-muted-foreground first:pl-4">{h}</th>
                {/each}
                <th class="w-24 border-b"></th>
              </tr>
            </thead>
            <tbody>
              {#each listed as n (n.id)}
                {@const Icon = kindIcon(n.kind)}
                {@const key = nodeKey(n)}
                {@const selected = k8s.detail?.kind === key && k8s.detail?.name === n.name && k8s.detail?.namespace === n.namespace}
                <tr class={cn('group hover:bg-accent/60', key && 'cursor-pointer', selected && 'bg-accent', n.old && 'opacity-60')} onclick={() => key && openDetail(key, n.namespace, n.name)}>
                  <td class="border-b border-border/50 px-3 py-1.5 pl-4 whitespace-nowrap text-muted-foreground">
                    <span class="inline-flex items-center gap-1.5"><Icon class="size-3.5" />{n.kind}</span>
                  </td>
                  <td class="max-w-72 truncate border-b border-border/50 px-3 py-1.5 font-medium" title={n.name}>
                    {n.name}{#if !n.managed}<span class="ml-1.5 text-[11px] font-normal text-muted-foreground">cluster</span>{/if}
                  </td>
                  <td class="border-b border-border/50 px-3 py-1.5 text-muted-foreground">{n.namespace}</td>
                  <td class="border-b border-border/50 px-3 py-1.5 whitespace-nowrap">{#if n.sync}<ArgoStatus what="sync" value={n.sync} label />{/if}</td>
                  <td class="border-b border-border/50 px-3 py-1.5 whitespace-nowrap">{#if n.health}<ArgoStatus what="health" value={n.health} label />{/if}</td>
                  <td
                    class={cn('max-w-[28rem] truncate border-b border-border/50 px-3 py-1.5 text-xs text-muted-foreground', n.result && 'text-destructive')}
                    title={[n.result, n.healthMessage, ...n.info].filter(Boolean).join('\n')}>
                    {n.result || (n.requiresPruning ? 'saiu do git: será removido num sync com prune' : '') || n.healthMessage || n.info.join(' · ')}
                  </td>
                  <td class="border-b border-border/50 pr-2 text-right whitespace-nowrap">
                    {#if n.managed && n.sync === 'OutOfSync' && !app.busy}
                      <Button
                        variant="ghost"
                        size="xs"
                        class="opacity-0 group-hover:opacity-100"
                        onclick={(e: MouseEvent) => (e.stopPropagation(), openSync(app, [target(n)]))}>Sincronizar</Button>
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      {:else}
        {#if history.length === 0}
          <p class="p-8 text-center text-sm text-muted-foreground">Nenhuma sincronização no histórico ainda.</p>
        {:else}
          <ol class="mx-auto max-w-4xl p-4">
            {#each history as h, i (h.id)}
              {@const s = h.sources.find((x) => !x.ref) ?? h.sources[0]}
              {@const rev = h.revisions[h.sources.indexOf(s)] ?? h.revisions[0] ?? ''}
              <li class="relative flex gap-3 pb-4 last:pb-0">
                <!-- linha do tempo -->
                <div class="flex flex-col items-center">
                  <span class={cn('mt-1 size-2.5 shrink-0 rounded-full', i === 0 ? 'bg-primary' : 'bg-border')}></span>
                  {#if i < history.length - 1}<span class="w-px flex-1 bg-border"></span>{/if}
                </div>
                <div class="min-w-0 flex-1 rounded-lg border bg-card p-3">
                  <div class="flex items-center gap-2 text-[13px]">
                    <span class="text-muted-foreground tabular-nums">#{h.id}</span>
                    <GitCommitIcon class="size-3.5 text-muted-foreground" />
                    {@render link(s ? commitUrl(s.repoUrl, rev) : null, shortRevision(rev) || '—', 'font-mono font-medium')}
                    {#if i === 0}<span class="rounded bg-primary/10 px-1.5 py-0.5 text-[10px] text-primary">atual</span>{/if}
                    <span class="flex-1"></span>
                    <span class="text-xs text-muted-foreground" title={fullDate(h.deployedAt)}>{ago(h.deployedAt)}</span>
                    {#if i > 0}
                      <Button
                        variant="outline"
                        size="xs"
                        disabled={app.autoSync || app.busy}
                        title={app.autoSync ? 'Desligue o auto-sync para voltar uma versão' : app.busy ? 'Há uma operação em andamento' : `Aplicar de novo a versão #${h.id}`}
                        onclick={() => void rollback(app, h.id, rev)}>
                        <UndoIcon />Voltar para esta
                      </Button>
                    {/if}
                  </div>
                  <div class="mt-1 flex flex-wrap gap-x-3 text-xs text-muted-foreground">
                    {#if s}
                      <span>{repoShort(s.repoUrl)}{s.path ? ` · ${s.path}` : ''}{s.chart ? ` · ${s.chart}` : ''}{s.targetRevision ? ` @ ${s.targetRevision}` : ''}</span>
                    {/if}
                    <span>{h.automated ? 'automático' : h.initiatedBy || 'manual'}</span>
                    {#if h.startedAt}<span>levou {duration(h.startedAt, h.deployedAt)}</span>{/if}
                  </div>
                </div>
              </li>
            {/each}
          </ol>
        {/if}
      {/if}
    </div>
  {/if}
</section>
