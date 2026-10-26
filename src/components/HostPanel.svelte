<script lang="ts">
  import ActivityIcon from '@lucide/svelte/icons/activity';
  import CpuIcon from '@lucide/svelte/icons/cpu';
  import MemoryStickIcon from '@lucide/svelte/icons/memory-stick';
  import HardDriveIcon from '@lucide/svelte/icons/hard-drive';
  import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
  import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
  import NetworkIcon from '@lucide/svelte/icons/network';
  import XIcon from '@lucide/svelte/icons/x';
  import ServerIcon from '@lucide/svelte/icons/server';
  import { Button } from '$lib/components/ui/button/index.js';
  import Sparkline from './Sparkline.svelte';
  import { app } from '../lib/state.svelte';
  import { side } from '../lib/side.svelte';
  import { sftpTarget } from '../lib/sftp.svelte';
  import { METRICS_INTERVAL, forgetMonitor, hostinfo, memPercent, sample } from '../lib/hostinfo.svelte';
  import { formatBytes, formatUptime } from '../lib/ui';
  import { cn } from '$lib/utils.js';

  const target = $derived(sftpTarget());
  const meta = $derived(target ? app.meta[target] : undefined);
  const sessionId = $derived(meta?.sessionId ?? null);
  const monitor = $derived(target ? hostinfo.monitors[target] : undefined);
  const m = $derived(monitor?.sessionId === sessionId ? monitor?.metrics : null);

  // Lê o servidor do painel alvo enquanto o painel estiver aberto.
  $effect(() => {
    const pane = target;
    if (!pane || sessionId == null) return;
    void sample(pane);
    const timer = setInterval(() => void sample(pane), METRICS_INTERVAL);
    return () => clearInterval(timer);
  });

  // Esquece leituras de painéis fechados.
  $effect(() => {
    for (const id of Object.keys(hostinfo.monitors)) if (!app.meta[id]) forgetMonitor(id);
  });

  /** Cor da barra conforme o uso: normal, alto, crítico. */
  const level = (pct: number) => (pct >= 90 ? 'bg-destructive' : pct >= 75 ? 'bg-amber-500' : 'bg-primary');
  const pct = (used: number, total: number) => (total > 0 ? (used / total) * 100 : 0);
  const rate = (bytes: number | null | undefined) => (bytes == null ? '—' : `${formatBytes(Math.round(bytes))}/s`);
</script>

{#snippet bar(value: number)}
  <div class="h-1.5 overflow-hidden rounded-full bg-muted">
    <div class={cn('h-full rounded-full transition-[width] duration-500', level(value))} style:width="{Math.min(100, value)}%"></div>
  </div>
{/snippet}

{#snippet section(Icon: typeof CpuIcon, title: string)}
  <div class="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
    <Icon class="size-3.5" />{title}
  </div>
{/snippet}

<aside class="flex w-[320px] shrink-0 flex-col border-l bg-sidebar">
  <header class="flex h-9 shrink-0 items-center gap-2 border-b pr-1.5 pl-3 text-muted-foreground">
    <ActivityIcon class="size-3.5" />
    <span class="min-w-0 flex-1 truncate text-sm">{meta?.spec.kind === 'ssh' ? meta.spec.host.label : 'Host'}</span>
    <Button variant="ghost" size="icon-sm" title="Fechar (Ctrl+Shift+M)" onclick={() => (side.panel = null)}><XIcon /></Button>
  </header>

  {#if !target}
    <div class="flex flex-1 flex-col items-center justify-center gap-2 p-6 text-center text-sm text-muted-foreground">
      <ServerIcon class="size-6" />
      <p>Abra uma sessão SSH para ver as métricas do servidor.</p>
    </div>
  {:else if sessionId == null}
    <p class="p-6 text-center text-sm text-muted-foreground">{meta?.exited ? 'Sessão encerrada.' : 'Aguardando a conexão…'}</p>
  {:else if !m}
    <p class={cn('p-6 text-center text-sm', monitor?.error ? 'text-destructive' : 'text-muted-foreground')}>
      {monitor?.error || 'Lendo o servidor…'}
    </p>
  {:else}
    {@const memUsed = m.memTotal - m.memAvailable}
    <div class="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-3">
      <div class="space-y-0.5">
        <div class="truncate text-sm font-medium">{m.hostname}</div>
        <div class="truncate text-xs text-muted-foreground" title={m.kernel}>{m.os || m.kernel}</div>
        <div class="text-xs text-muted-foreground">Ligado há {formatUptime(m.uptime)}</div>
        {#if monitor?.error}
          <div class="pt-1 text-xs text-destructive">{monitor.error}</div>
        {/if}
      </div>

      <div class="space-y-2">
        {@render section(CpuIcon, 'CPU')}
        <div class="flex items-baseline justify-between">
          <span class="text-2xl font-semibold tabular-nums">{monitor?.cpu == null ? '—' : `${Math.round(monitor.cpu)}%`}</span>
          <span class="text-xs text-muted-foreground tabular-nums" title="Carga média em 1, 5 e 15 minutos">
            {m.cpus} vCPU · carga {m.load.map((l) => l.toFixed(2)).join(' ')}
          </span>
        </div>
        <Sparkline values={monitor?.cpuHistory ?? []} class="h-10 w-full text-primary" />
      </div>

      <div class="space-y-2">
        {@render section(MemoryStickIcon, 'Memória')}
        <div class="flex items-baseline justify-between">
          <span class="text-2xl font-semibold tabular-nums">{Math.round(memPercent(m))}%</span>
          <span class="text-xs text-muted-foreground tabular-nums">{formatBytes(memUsed)} de {formatBytes(m.memTotal)}</span>
        </div>
        <Sparkline values={monitor?.memHistory ?? []} class="h-10 w-full text-primary" />
        {#if m.swapTotal > 0}
          {@const swapUsed = m.swapTotal - m.swapFree}
          <div class="space-y-1">
            <div class="flex justify-between text-xs text-muted-foreground tabular-nums">
              <span>Swap</span><span>{formatBytes(swapUsed)} de {formatBytes(m.swapTotal)}</span>
            </div>
            {@render bar(pct(swapUsed, m.swapTotal))}
          </div>
        {/if}
      </div>

      <div class="space-y-2">
        {@render section(NetworkIcon, 'Rede')}
        <div class="grid grid-cols-2 gap-2 text-sm tabular-nums">
          <div class="flex items-center gap-1.5" title="Recebendo"><ArrowDownIcon class="size-3.5 text-muted-foreground" />{rate(monitor?.rx)}</div>
          <div class="flex items-center gap-1.5" title="Enviando"><ArrowUpIcon class="size-3.5 text-muted-foreground" />{rate(monitor?.tx)}</div>
        </div>
      </div>

      <div class="space-y-3">
        {@render section(HardDriveIcon, 'Discos')}
        {#each m.disks as disk (disk.device + disk.mount)}
          {@const used = pct(disk.used, disk.total)}
          <div class="space-y-1">
            <div class="flex items-baseline justify-between gap-2 text-xs">
              <span class="truncate font-medium" title={disk.device}>{disk.mount}</span>
              <span class="shrink-0 text-muted-foreground tabular-nums">{formatBytes(disk.used)} de {formatBytes(disk.total)} · {Math.round(used)}%</span>
            </div>
            {@render bar(used)}
          </div>
        {:else}
          <p class="text-xs text-muted-foreground">Nenhum disco encontrado.</p>
        {/each}
      </div>
    </div>
  {/if}
</aside>
