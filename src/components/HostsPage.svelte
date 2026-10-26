<script lang="ts">
  import SearchIcon from '@lucide/svelte/icons/search';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import FolderIcon from '@lucide/svelte/icons/folder';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import TerminalSquareIcon from '@lucide/svelte/icons/square-terminal';
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import DownloadIcon from '@lucide/svelte/icons/download';
  import ServerIcon from '@lucide/svelte/icons/server';
  import KeyRoundIcon from '@lucide/svelte/icons/key-round';
  import RouteIcon from '@lucide/svelte/icons/route';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { Badge } from '$lib/components/ui/badge/index.js';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu/index.js';
  import * as ContextMenu from '$lib/components/ui/context-menu/index.js';
  import HostAvatar from './HostAvatar.svelte';
  import { app, connectHost, localSpec, newTab, openDialog } from '../lib/state.svelte';
  import { address, groupHosts, matchHost } from '../lib/ui';
  import type { HostView } from '../lib/backend';
  import { confirmDeleteHost, duplicateHost } from '../lib/actions.svelte';

  let query = $state('');
  /** Pasta aberta (null = todas). */
  let folder = $state<string | null>(null);

  const credentialOf = (host: HostView) => app.credentials.find((c) => c.id === host.credentialId);
  const jumpOf = (host: HostView) => app.hosts.find((h) => h.id === host.jumpHostId);

  // Com busca, mostra todos os hosts que batem (de qualquer pasta), sem a grade de pastas.
  const searching = $derived(query.trim() !== '');
  const folders = $derived(groupHosts(app.hosts).filter(([g]) => g !== ''));
  // Na raiz, todos os hosts (as pastas em cima são atalhos de filtro).
  const visible = $derived(
    app.hosts
      .filter((h) => (searching ? matchHost(h, query) : folder === null || h.group === folder))
      .sort((a, b) => a.label.localeCompare(b.label)),
  );

  const edit = (host: HostView | null) => void openDialog({ type: 'hostForm', host });

  /** Ações do host (menu ⋯ e botão direito). null = separador. */
  const actions = (host: HostView) =>
    [
      { label: 'Conectar', run: () => connectHost(host) },
      { label: 'Editar', run: () => edit(host) },
      { label: 'Duplicar', run: () => duplicateHost(host) },
      host.protocol === 'ssh' && { label: 'Novo túnel por este host', run: () => void openDialog({ type: 'tunnelForm', tunnel: null, hostId: host.id }) },
      null,
      { label: 'Excluir', run: () => confirmDeleteHost(host), destructive: true },
    ].filter((a) => a !== false);
</script>


<div class="flex h-full flex-col">
  <header class="flex flex-wrap items-center gap-2 border-b px-6 py-4">
    <div class="mr-auto flex items-center gap-1.5 text-lg font-semibold">
      {#if folder !== null && !searching}
        <button class="text-muted-foreground hover:text-foreground" onclick={() => (folder = null)}>Hosts</button>
        <ChevronRightIcon class="size-4 text-muted-foreground" />
        <span>{folder}</span>
      {:else}
        Hosts
      {/if}
    </div>
    <div class="relative w-64">
      <SearchIcon class="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
      <Input bind:value={query} placeholder="Buscar hosts" class="pl-8" spellcheck={false} />
    </div>
    <DropdownMenu.Root>
      <DropdownMenu.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="outline"><TerminalSquareIcon /> Terminal local <ChevronDownIcon class="opacity-60" /></Button>
        {/snippet}
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="end">
        {#each app.profiles as profile (profile.id)}
          <DropdownMenu.Item onSelect={() => newTab(localSpec(profile))}>{profile.name}</DropdownMenu.Item>
        {/each}
      </DropdownMenu.Content>
    </DropdownMenu.Root>
    <Button variant="outline" size="icon" title="Importar do ~/.ssh/config" onclick={() => void openDialog({ type: 'sshImport' })}>
      <DownloadIcon />
    </Button>
    <Button onclick={() => edit(null)}><PlusIcon /> Novo host</Button>
  </header>

  <div class="min-h-0 flex-1 overflow-y-auto px-6 py-5">
    {#if app.hosts.length === 0}
      <div class="mx-auto mt-20 flex max-w-sm flex-col items-center gap-3 text-center">
        <div class="flex size-14 items-center justify-center rounded-2xl bg-muted"><ServerIcon class="size-6 text-muted-foreground" /></div>
        <h2 class="text-base font-semibold">Nenhum host ainda</h2>
        <p class="text-muted-foreground">Cadastre seus servidores SSH e RDP para conectar com um clique, ou importe do seu ~/.ssh/config.</p>
        <div class="flex gap-2">
          <Button variant="outline" onclick={() => void openDialog({ type: 'sshImport' })}><DownloadIcon /> Importar</Button>
          <Button onclick={() => edit(null)}><PlusIcon /> Novo host</Button>
        </div>
      </div>
    {:else}
      {#if folder === null && !searching && folders.length > 0}
        <h3 class="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">Pastas</h3>
        <div class="mb-6 grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-3">
          {#each folders as [name, hosts] (name)}
            <button
              class="group flex items-center gap-3 rounded-xl border bg-card p-3 text-left transition-colors hover:border-primary/40 hover:bg-accent"
              onclick={() => (folder = name)}>
              <span class="flex size-9 items-center justify-center rounded-lg bg-primary/12 text-primary"><FolderIcon class="size-4" /></span>
              <span class="min-w-0 flex-1">
                <span class="block truncate font-medium">{name}</span>
                <span class="block text-xs text-muted-foreground">{hosts.length === 1 ? '1 host' : `${hosts.length} hosts`}</span>
              </span>
              <ChevronRightIcon class="size-4 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100" />
            </button>
          {/each}
        </div>
        <h3 class="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">Todos os hosts · {app.hosts.length}</h3>
      {/if}

      {#if visible.length === 0 && (searching || folder !== null)}
        <p class="mt-10 text-center text-muted-foreground">{searching ? 'Nenhum host encontrado.' : 'Pasta vazia.'}</p>
      {/if}

      <div class="grid grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-3">
        {#each visible as host (host.id)}
          {@const credential = credentialOf(host)}
          {@const jump = jumpOf(host)}
          <ContextMenu.Root>
            <ContextMenu.Trigger>
              {#snippet child({ props })}
                <div
                  {...props}
                  role="button"
                  tabindex="0"
                  class="group relative flex cursor-pointer items-center gap-3 rounded-xl border bg-card p-3 transition-colors hover:border-primary/40 hover:bg-accent"
                  title="Conectar"
                  onclick={() => connectHost(host)}
                  onkeydown={(e) => e.key === 'Enter' && connectHost(host)}>
                  <HostAvatar {host} size="lg" />
                  <div class="min-w-0 flex-1">
                    <div class="flex items-center gap-1.5">
                      <span class="truncate font-medium">{host.label}</span>
                      {#if host.protocol === 'rdp'}<Badge variant="secondary" class="h-4 px-1 text-[10px]">RDP</Badge>{/if}
                    </div>
                    <div class="truncate text-xs text-muted-foreground">{address(host, credential?.username || host.username)}</div>
                    {#if credential || jump || (folder === null && host.group)}
                      <div class="mt-1 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-muted-foreground">
                        {#if folder === null && host.group}<span class="flex items-center gap-1"><FolderIcon class="size-3" />{host.group}</span>{/if}
                        {#if credential}<span class="flex items-center gap-1"><KeyRoundIcon class="size-3" />{credential.label}</span>{/if}
                        {#if jump}<span class="flex items-center gap-1"><RouteIcon class="size-3" />via {jump.label}</span>{/if}
                      </div>
                    {/if}
                  </div>
                  <DropdownMenu.Root>
                    <DropdownMenu.Trigger>
                      {#snippet child({ props })}
                        <Button
                          {...props}
                          variant="ghost"
                          size="icon-sm"
                          class="opacity-0 group-hover:opacity-100 aria-expanded:opacity-100"
                          onclick={(e: MouseEvent) => e.stopPropagation()}>
                          <EllipsisIcon />
                        </Button>
                      {/snippet}
                    </DropdownMenu.Trigger>
                    <DropdownMenu.Content align="end">
                      {#each actions(host) as action, i (i)}
                        {#if action}
                          <DropdownMenu.Item variant={action.destructive ? 'destructive' : 'default'} onSelect={action.run}>{action.label}</DropdownMenu.Item>
                        {:else}
                          <DropdownMenu.Separator />
                        {/if}
                      {/each}
                    </DropdownMenu.Content>
                  </DropdownMenu.Root>
                </div>
              {/snippet}
            </ContextMenu.Trigger>
            <ContextMenu.Content>
              {#each actions(host) as action, i (i)}
                {#if action}
                  <ContextMenu.Item variant={action.destructive ? 'destructive' : 'default'} onSelect={action.run}>{action.label}</ContextMenu.Item>
                {:else}
                  <ContextMenu.Separator />
                {/if}
              {/each}
            </ContextMenu.Content>
          </ContextMenu.Root>
        {/each}
      </div>
    {/if}
  </div>
</div>
