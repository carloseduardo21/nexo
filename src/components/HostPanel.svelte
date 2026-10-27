<script lang="ts">
  import ActivityIcon from '@lucide/svelte/icons/activity';
  import XIcon from '@lucide/svelte/icons/x';
  import ServerIcon from '@lucide/svelte/icons/server';
  import { Button } from '$lib/components/ui/button/index.js';
  import * as Tabs from '$lib/components/ui/tabs/index.js';
  import HostMetrics from './HostMetrics.svelte';
  import ContainerList from './ContainerList.svelte';
  import { app } from '../lib/state.svelte';
  import { side } from '../lib/side.svelte';
  import { forgetPane, hostinfo, hostTarget, paneLabel } from '../lib/hostinfo.svelte';

  const target = $derived(hostTarget());
  const meta = $derived(target ? app.meta[target] : undefined);
  const sessionId = $derived(meta?.sessionId ?? null);

  // Esquece leituras de painéis fechados.
  $effect(() => {
    for (const id of new Set([...Object.keys(hostinfo.monitors), ...Object.keys(hostinfo.containers)])) if (!app.meta[id]) forgetPane(id);
  });
</script>

<aside class="flex w-[340px] shrink-0 flex-col border-l bg-sidebar">
  <header class="flex h-9 shrink-0 items-center gap-2 border-b pr-1.5 pl-3 text-muted-foreground">
    <ActivityIcon class="size-3.5" />
    <span class="min-w-0 flex-1 truncate text-sm">{target ? paneLabel(target) : 'Host'}</span>
    <Button variant="ghost" size="icon-sm" title="Fechar (Ctrl+Shift+M)" onclick={() => (side.panel = null)}><XIcon /></Button>
  </header>

  {#if !target}
    <div class="flex flex-1 flex-col items-center justify-center gap-2 p-6 text-center text-sm text-muted-foreground">
      <ServerIcon class="size-6" />
      <p>Abra uma sessão SSH ou um terminal Linux/WSL para ver as métricas e os containers.</p>
    </div>
  {:else if sessionId == null}
    <p class="p-6 text-center text-sm text-muted-foreground">{meta?.exited ? 'Sessão encerrada.' : 'Aguardando a conexão…'}</p>
  {:else}
    <Tabs.Root bind:value={hostinfo.tab} class="flex min-h-0 flex-1 flex-col gap-0">
      <div class="shrink-0 px-3 pt-2.5">
        <Tabs.List class="w-full">
          <Tabs.Trigger value="metrics">Métricas</Tabs.Trigger>
          <Tabs.Trigger value="containers">Containers</Tabs.Trigger>
        </Tabs.List>
      </div>
      <!-- Só a aba visível consulta o servidor. -->
      {#if hostinfo.tab === 'metrics'}
        <HostMetrics paneId={target} {sessionId} />
      {:else}
        <ContainerList paneId={target} {sessionId} />
      {/if}
    </Tabs.Root>
  {/if}
</aside>
