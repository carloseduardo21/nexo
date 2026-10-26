<script lang="ts">
  import PlusIcon from '@lucide/svelte/icons/plus';
  import WaypointsIcon from '@lucide/svelte/icons/waypoints';
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import PlayIcon from '@lucide/svelte/icons/play';
  import SquareIcon from '@lucide/svelte/icons/square';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Badge } from '$lib/components/ui/badge/index.js';
  import { app, openDialog, startTunnel, stopTunnel } from '../lib/state.svelte';
  import { tunnelRoute, tunnelStatusText } from '../lib/ui';
  import { cn } from '$lib/utils.js';
  import type { Tunnel } from '../lib/backend';

  const hasSsh = $derived(app.hosts.some((h) => h.protocol !== 'rdp'));
  const edit = (tunnel: Tunnel | null) => void openDialog({ type: 'tunnelForm', tunnel });
  const kindLabel = { local: 'Local', remote: 'Remoto', dynamic: 'SOCKS' } as const;
</script>

<div class="flex h-full flex-col">
  <header class="flex items-center gap-2 border-b px-6 py-4">
    <div class="mr-auto">
      <h1 class="text-lg font-semibold">Túneis</h1>
      <p class="text-xs text-muted-foreground">Portas da VM (banco, painel web) como se fossem locais. Cada túnel tem conexão própria e reconecta sozinho.</p>
    </div>
    <Button disabled={!hasSsh} onclick={() => edit(null)}><PlusIcon /> Novo túnel</Button>
  </header>

  <div class="min-h-0 flex-1 overflow-y-auto px-6 py-5">
    {#if app.tunnels.length === 0}
      <div class="mx-auto mt-20 flex max-w-sm flex-col items-center gap-3 text-center">
        <div class="flex size-14 items-center justify-center rounded-2xl bg-muted"><WaypointsIcon class="size-6 text-muted-foreground" /></div>
        <h2 class="text-base font-semibold">Nenhum túnel</h2>
        <p class="text-muted-foreground">{hasSsh ? 'Acesse o Postgres da VM em localhost:5432, um painel web interno ou um proxy SOCKS.' : 'Cadastre um host SSH primeiro.'}</p>
        {#if hasSsh}<Button onclick={() => edit(null)}><PlusIcon /> Novo túnel</Button>{/if}
      </div>
    {:else}
      <div class="grid gap-2">
        {#each app.tunnels as tunnel (tunnel.id)}
          {@const status = app.tunnelStatus[tunnel.id]}
          {@const host = app.hosts.find((h) => h.id === tunnel.hostId)}
          {@const on = !!status && status.state !== 'error'}
          <div class="flex items-center gap-3 rounded-xl border bg-card px-4 py-3">
            <span
              class={cn(
                'size-2.5 shrink-0 rounded-full bg-muted-foreground/40',
                status?.state === 'running' && 'bg-emerald-500 shadow-[0_0_8px] shadow-emerald-500/60',
                (status?.state === 'connecting' || status?.state === 'reconnecting') && 'animate-pulse bg-amber-400',
                status?.state === 'error' && 'bg-destructive',
              )}></span>
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <span class="truncate font-medium">{tunnel.label || tunnelRoute(tunnel)}</span>
                <Badge variant="secondary" class="text-[10px]">{kindLabel[tunnel.kind]}</Badge>
                {#if tunnel.autoStart}<Badge variant="outline" class="text-[10px]">automático</Badge>{/if}
              </div>
              <div class="truncate text-xs text-muted-foreground">
                {tunnel.label ? `${tunnelRoute(tunnel)} · ` : ''}via {host?.label ?? '?'}
              </div>
            </div>
            <span class={cn('max-w-64 truncate text-xs text-muted-foreground', status?.state === 'error' && 'text-destructive')}>{tunnelStatusText(status)}</span>
            {#if on}
              <Button variant="outline" size="sm" onclick={() => stopTunnel(tunnel.id)}><SquareIcon /> Parar</Button>
            {:else}
              <Button size="sm" onclick={() => startTunnel(tunnel.id)}><PlayIcon /> Iniciar</Button>
            {/if}
            <Button variant="ghost" size="icon-sm" title="Editar" onclick={() => edit(tunnel)}><PencilIcon /></Button>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
