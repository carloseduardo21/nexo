<script lang="ts">
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import ShipWheelIcon from '@lucide/svelte/icons/ship-wheel';
  import SearchIcon from '@lucide/svelte/icons/search';
  import RefreshIcon from '@lucide/svelte/icons/refresh-cw';
  import FileIcon from '@lucide/svelte/icons/file-cog';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import ColumnsIcon from '@lucide/svelte/icons/columns-3';
  import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
  import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
  import LoaderIcon from '@lucide/svelte/icons/loader-circle';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu/index.js';
  import Picker from './Picker.svelte';
  import KubeDetail from './KubeDetail.svelte';
  import {
    KIND_GROUPS,
    LIST_INTERVAL,
    k8s,
    kindInfo,
    loadContexts,
    loadList,
    openDetail,
    parseAge,
    selectContext,
    selectKind,
    selectNamespace,
  } from '../lib/k8s.svelte';
  import { settings, updateSettings } from '../lib/settings.svelte';
  import { cn } from '$lib/utils.js';

  let query = $state('');
  let sort = $state<{ column: number; desc: boolean } | null>(null);

  const context = $derived(k8s.config?.contexts.find((c) => c.name === k8s.context));
  const info = $derived(kindInfo(k8s.kind));
  const allNamespaces = $derived(info.namespaced && !k8s.namespace);
  const table = $derived(k8s.table);

  // Colunas: as principais (ou todas, em "mais colunas"); com todos os namespaces, um
  // "Namespace" logo depois da primeira.
  type Col = { name: string; index: number | 'namespace' };
  const columns = $derived.by(() => {
    if (!table) return [] as Col[];
    const cols: Col[] = table.columns.flatMap((c, index) => (k8s.wide || c.priority === 0 ? [{ name: c.name, index }] : []));
    if (allNamespaces) cols.splice(1, 0, { name: 'Namespace', index: 'namespace' });
    return cols;
  });

  type Row = NonNullable<typeof table>['rows'][number];
  const cell = (row: Row, index: Col['index']) => {
    const v = index === 'namespace' ? row.namespace : row.cells[index];
    return v == null ? '' : typeof v === 'object' ? JSON.stringify(v) : String(v);
  };

  const rows = $derived.by(() => {
    if (!table) return [];
    const q = query.trim().toLowerCase();
    const list = q ? table.rows.filter((r) => [r.name, r.namespace, ...r.cells.map(String)].some((v) => v.toLowerCase().includes(q))) : [...table.rows];
    if (sort) {
      const col = columns[sort.column]?.index;
      if (col !== undefined) {
        const dir = sort.desc ? -1 : 1;
        list.sort((a, b) => dir * compare(cell(a, col), cell(b, col)));
      }
    }
    return list;
  });

  /** Idades ("7d10h"), números ("2 (14h ago)") e texto, nessa ordem. */
  function compare(a: string, b: string) {
    const [ageA, ageB] = [parseAge(a), parseAge(b)];
    if (ageA !== null && ageB !== null) return ageA - ageB;
    const [numA, numB] = [parseFloat(a), parseFloat(b)];
    if (!isNaN(numA) && !isNaN(numB) && numA !== numB) return numA - numB;
    return a.localeCompare(b, undefined, { numeric: true });
  }

  function toggleSort(column: number) {
    sort = sort?.column !== column ? { column, desc: false } : sort.desc ? null : { column, desc: true };
  }

  const GOOD = /^(Running|Completed|Succeeded|Active|Bound|Ready|Available|Normal|True|Complete)$/;
  const BAD = /BackOff|Err|Error|Failed|OOMKilled|NotReady|Evicted|Lost|Unschedulable|Invalid|Unknown/;
  const WARN = /Pending|Creating|Initializing|Terminating|^Init:|Warning|Released|SchedulingDisabled|Suspended/;

  /** Cor do texto em colunas de estado e em "Ready" incompleto (menos em pods que já terminaram). */
  function tone(column: string, value: string, row: Row) {
    if (column === 'Ready' && /^\d+\/\d+$/.test(value)) {
      const [a, b] = value.split('/');
      return a === b || row.cells.includes('Completed') ? '' : 'text-amber-600 dark:text-amber-400';
    }
    if (!['Status', 'Phase', 'Type'].includes(column)) return '';
    if (BAD.test(value)) return 'text-destructive';
    if (WARN.test(value)) return 'text-amber-600 dark:text-amber-400';
    if (GOOD.test(value)) return 'text-emerald-600 dark:text-emerald-400';
    return '';
  }

  // Lê o kubeconfig ao abrir a página (e quando o arquivo escolhido muda).
  $effect(() => {
    void settings.kubeconfig;
    void loadContexts();
  });

  // A lista visível é consultada de tempos em tempos.
  $effect(() => {
    void [k8s.context, k8s.namespace, k8s.kind];
    sort = null;
    void loadList();
    const timer = setInterval(() => !document.hidden && void loadList(), LIST_INTERVAL);
    return () => clearInterval(timer);
  });

  async function chooseKubeconfig() {
    const path = await openFile({ title: 'Escolher kubeconfig', multiple: false });
    if (typeof path === 'string') updateSettings({ kubeconfig: path });
  }

  const contextItems = $derived((k8s.config?.contexts ?? []).map((c) => ({ value: c.name, label: c.name, hint: c.server || c.cluster })));
  const namespaceItems = $derived([
    { value: '', label: 'Todos os namespaces' },
    ...k8s.namespaces.map((n) => ({ value: n, label: n })),
    // O escolhido continua na lista mesmo sem permissão para listar.
    ...(k8s.namespace && !k8s.namespaces.includes(k8s.namespace) ? [{ value: k8s.namespace, label: k8s.namespace }] : []),
  ]);
  const NAME = /^[a-z0-9]([-a-z0-9]*[a-z0-9])?$/;
</script>

{#snippet sourceMenu()}
  <DropdownMenu.Root>
    <DropdownMenu.Trigger>
      {#snippet child({ props })}
        <Button {...props} variant="ghost" size="icon-xs" title="Kubeconfig"><EllipsisIcon /></Button>
      {/snippet}
    </DropdownMenu.Trigger>
    <DropdownMenu.Content align="start">
      <DropdownMenu.Item onSelect={() => void loadContexts()}><RefreshIcon />Recarregar o kubeconfig</DropdownMenu.Item>
      <DropdownMenu.Item onSelect={chooseKubeconfig}><FileIcon />Escolher outro arquivo…</DropdownMenu.Item>
      {#if settings.kubeconfig}
        <DropdownMenu.Item onSelect={() => updateSettings({ kubeconfig: '' })}>Usar o padrão (~/.kube/config)</DropdownMenu.Item>
      {/if}
    </DropdownMenu.Content>
  </DropdownMenu.Root>
{/snippet}

{#if !k8s.config}
  <div class="flex h-full flex-col items-center justify-center gap-3 p-8 text-center">
    <ShipWheelIcon class="size-8 text-muted-foreground" />
    {#if k8s.configError}
      <p class="font-medium">Não deu para ler o kubeconfig</p>
      <p class="max-w-md text-sm text-muted-foreground">{k8s.configError}</p>
      <div class="flex gap-2">
        <Button variant="outline" size="sm" onclick={chooseKubeconfig}><FileIcon />Escolher arquivo…</Button>
        {#if settings.kubeconfig}
          <Button variant="ghost" size="sm" onclick={() => updateSettings({ kubeconfig: '' })}>Usar o padrão</Button>
        {:else}
          <Button variant="ghost" size="sm" onclick={() => void loadContexts()}><RefreshIcon />Tentar de novo</Button>
        {/if}
      </div>
    {:else}
      <p class="text-sm text-muted-foreground">Lendo o kubeconfig…</p>
    {/if}
  </div>
{:else if k8s.config.contexts.length === 0}
  <div class="flex h-full flex-col items-center justify-center gap-3 p-8 text-center">
    <ShipWheelIcon class="size-8 text-muted-foreground" />
    <p class="font-medium">Nenhum contexto em {k8s.config.source}</p>
    <Button variant="outline" size="sm" onclick={chooseKubeconfig}><FileIcon />Escolher outro arquivo…</Button>
  </div>
{:else}
  <div class="flex h-full">
    <aside class="flex w-56 shrink-0 flex-col border-r bg-sidebar">
      <div class="grid gap-1.5 p-2">
        <Picker items={contextItems} value={k8s.context} onSelect={selectContext} placeholder="Buscar contexto" empty="Nenhum contexto.">
          {#snippet trigger()}
            <span class="block truncate text-[13px] font-medium">{k8s.context || 'Contexto'}</span>
            <span class="block truncate text-[11px] text-muted-foreground">{context?.server || context?.cluster}</span>
          {/snippet}
        </Picker>
        <Picker
          items={namespaceItems}
          value={k8s.namespace}
          onSelect={selectNamespace}
          placeholder="Buscar ou digitar namespace"
          empty="Nenhum namespace."
          custom={(typed) => (NAME.test(typed) ? `Usar “${typed}”` : null)}>
          {#snippet trigger()}
            <span class={cn('block truncate text-[13px]', !info.namespaced && 'text-muted-foreground')}>
              {k8s.namespace || 'Todos os namespaces'}
            </span>
          {/snippet}
        </Picker>
        {#if k8s.namespacesError}
          <p class="px-1 text-[11px] text-muted-foreground" title={k8s.namespacesError}>Sem permissão para listar namespaces: digite um.</p>
        {/if}
      </div>

      <nav class="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {#each KIND_GROUPS as group (group.label)}
          <div class="mt-2 mb-0.5 px-1.5 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">{group.label}</div>
          {#each group.kinds as item (item.kind)}
            <button
              class={cn(
                'flex w-full items-center rounded-md px-1.5 py-1 text-left text-[13px] hover:bg-sidebar-accent',
                k8s.kind === item.kind && 'bg-sidebar-accent font-medium text-sidebar-primary',
              )}
              onclick={() => selectKind(item.kind)}>
              {item.label}
            </button>
          {/each}
        {/each}
      </nav>

      <div class="flex items-center gap-1 border-t px-2 py-1.5 text-[11px] text-muted-foreground">
        <span class="min-w-0 flex-1 truncate" title={k8s.config.source}>{k8s.config.source}</span>
        {@render sourceMenu()}
      </div>
    </aside>

    <section class="flex min-w-0 flex-1 flex-col">
      <header class="flex h-12 shrink-0 items-center gap-3 border-b px-4">
        <h1 class="font-medium">{info.label}</h1>
        {#if table && k8s.tableKey}
          <span class="text-xs text-muted-foreground">
            {query ? `${rows.length} de ${table.rows.length}` : table.rows.length}{table.truncated ? '+' : ''}
          </span>
        {/if}
        {#if k8s.loading}<LoaderIcon class="size-3.5 animate-spin text-muted-foreground" />{/if}
        <div class="flex-1"></div>
        <div class="relative w-56">
          <SearchIcon class="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input bind:value={query} placeholder="Filtrar" class="h-8 pl-8 text-sm" spellcheck={false} />
        </div>
        <Button
          variant={k8s.wide ? 'secondary' : 'ghost'}
          size="icon-sm"
          title={k8s.wide ? 'Menos colunas' : 'Mais colunas (-o wide)'}
          onclick={() => (k8s.wide = !k8s.wide)}>
          <ColumnsIcon />
        </Button>
        <Button variant="ghost" size="icon-sm" title="Atualizar" onclick={() => void loadList()}><RefreshIcon /></Button>
      </header>

      {#if k8s.listError}
        <div class="border-b bg-destructive/10 px-4 py-2 text-sm text-destructive">{k8s.listError}</div>
      {/if}
      {#if table?.truncated}
        <div class="border-b bg-amber-500/10 px-4 py-1.5 text-xs text-amber-700 dark:text-amber-400">
          Lista cortada nos primeiros {table.rows.length} itens. Escolha um namespace para ver menos.
        </div>
      {/if}

      <div class="min-h-0 flex-1 overflow-auto">
        {#if !table}
          {#if !k8s.listError}<p class="p-8 text-center text-sm text-muted-foreground">Consultando o cluster…</p>{/if}
        {:else if rows.length === 0}
          <p class="p-8 text-center text-sm text-muted-foreground">
            {table.rows.length ? 'Nada corresponde ao filtro.' : `Nenhum item${info.namespaced && k8s.namespace ? ` em ${k8s.namespace}` : ''}.`}
          </p>
        {:else}
          <table class="w-full border-separate border-spacing-0 text-[13px]">
            <thead class="sticky top-0 z-10 bg-background">
              <tr>
                {#each columns as col, i (col.index)}
                  <th class="border-b px-3 py-2 text-left text-xs font-medium whitespace-nowrap text-muted-foreground first:pl-4">
                    <button class="inline-flex items-center gap-1 hover:text-foreground" onclick={() => toggleSort(i)}>
                      {col.name}
                      {#if sort?.column === i}
                        {#if sort.desc}<ArrowDownIcon class="size-3" />{:else}<ArrowUpIcon class="size-3" />{/if}
                      {/if}
                    </button>
                  </th>
                {/each}
              </tr>
            </thead>
            <tbody>
              {#each rows as row (row.uid || `${row.namespace}/${row.name}`)}
                {@const selected = k8s.detail?.kind === k8s.kind && k8s.detail.name === row.name && k8s.detail.namespace === row.namespace}
                <tr
                  class={cn('cursor-pointer hover:bg-accent/60', selected && 'bg-accent')}
                  onclick={() => openDetail(k8s.kind, row.namespace, row.name)}>
                  {#each columns as col, i (col.index)}
                    {@const value = cell(row, col.index)}
                    <td
                      class={cn(
                        'max-w-[28rem] truncate border-b border-border/50 px-3 py-1.5 tabular-nums first:pl-4',
                        i === 0 && 'font-medium',
                        tone(col.name, value, row),
                      )}
                      title={value}>
                      {value}
                    </td>
                  {/each}
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>
    </section>

    {#if k8s.detail}
      <KubeDetail />
    {/if}
  </div>
{/if}
