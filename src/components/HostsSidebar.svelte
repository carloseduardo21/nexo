<script lang="ts">
  import SearchIcon from '@lucide/svelte/icons/search';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import TerminalSquareIcon from '@lucide/svelte/icons/square-terminal';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import HostAvatar from './HostAvatar.svelte';
  import { app, connectHost, localSpec, newTab, openDialog } from '../lib/state.svelte';
  import { address, groupHosts, matchHost } from '../lib/ui';
  import { cn } from '$lib/utils.js';

  let query = $state('');
  /** Pastas recolhidas. */
  let collapsed = $state<Record<string, boolean>>({});

  const groups = $derived(groupHosts(app.hosts.filter((h) => matchHost(h, query))));
  // Hosts com sessão aberta ficam marcados.
  const open = $derived(new Set(Object.values(app.meta).flatMap((m) => (m.spec.kind !== 'local' && !m.exited ? [m.spec.host.id] : []))));
</script>

<aside class="flex w-60 shrink-0 flex-col border-r bg-sidebar">
  <div class="flex items-center gap-1 p-2">
    <div class="relative flex-1">
      <SearchIcon class="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground" />
      <Input bind:value={query} placeholder="Buscar" class="h-8 pl-7 text-xs" spellcheck={false} />
    </div>
    <Button variant="ghost" size="icon-sm" title="Novo host" onclick={() => void openDialog({ type: 'hostForm', host: null })}><PlusIcon /></Button>
  </div>

  <div class="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
    {#each groups as [group, hosts] (group)}
      {#if group}
        <button
          class="mt-2 mb-0.5 flex w-full items-center gap-1 px-1 text-[11px] font-medium tracking-wide text-muted-foreground uppercase hover:text-foreground"
          onclick={() => (collapsed[group] = !collapsed[group])}>
          <ChevronRightIcon class={cn('size-3 transition-transform', !collapsed[group] && 'rotate-90')} />
          {group}
          <span class="ml-auto font-normal normal-case">{hosts.length}</span>
        </button>
      {/if}
      {#if !collapsed[group] || query}
        {#each hosts as host (host.id)}
          <div class="group relative">
            <button
              class="flex w-full items-center gap-2 rounded-md px-1.5 py-1.5 text-left hover:bg-sidebar-accent"
              title={`Conectar a ${address(host)}`}
              onclick={() => connectHost(host)}>
              <HostAvatar {host} size="sm" />
              <span class="min-w-0 flex-1">
                <span class="block truncate text-[13px]">{host.label}</span>
                <span class="block truncate text-[11px] text-muted-foreground">{address(host)}</span>
              </span>
              {#if open.has(host.id)}<span class="size-1.5 shrink-0 rounded-full bg-primary" title="Sessão aberta"></span>{/if}
            </button>
            <Button
              variant="ghost"
              size="icon-xs"
              class="absolute top-1/2 right-1 -translate-y-1/2 bg-sidebar-accent opacity-0 group-hover:opacity-100"
              title="Editar"
              onclick={() => void openDialog({ type: 'hostForm', host })}>
              <PencilIcon />
            </Button>
          </div>
        {/each}
      {/if}
    {:else}
      <p class="px-2 py-6 text-center text-xs text-muted-foreground">{app.hosts.length ? 'Nenhum host encontrado.' : 'Nenhum host cadastrado.'}</p>
    {/each}
  </div>

  <div class="border-t p-2">
    <p class="mb-1 px-1 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">Terminais locais</p>
    {#each app.profiles as profile (profile.id)}
      <button class="flex w-full items-center gap-2 rounded-md px-1.5 py-1 text-left text-[13px] hover:bg-sidebar-accent" onclick={() => newTab(localSpec(profile))}>
        <TerminalSquareIcon class="size-4 text-muted-foreground" />
        <span class="truncate">{profile.name}</span>
      </button>
    {/each}
  </div>
</aside>
