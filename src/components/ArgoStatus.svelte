<script lang="ts" module>
  import HeartIcon from '@lucide/svelte/icons/heart';
  import HeartCrackIcon from '@lucide/svelte/icons/heart-crack';
  import LoaderIcon from '@lucide/svelte/icons/loader-circle';
  import PauseIcon from '@lucide/svelte/icons/circle-pause';
  import GhostIcon from '@lucide/svelte/icons/ghost';
  import HelpIcon from '@lucide/svelte/icons/circle-question-mark';
  import CheckIcon from '@lucide/svelte/icons/circle-check';
  import ArrowUpIcon from '@lucide/svelte/icons/circle-arrow-up';
  import XIcon from '@lucide/svelte/icons/circle-x';
  import StopIcon from '@lucide/svelte/icons/square-stop';

  type Look = { icon: typeof HeartIcon; color: string; spin?: boolean };

  const HEALTH: Record<string, Look> = {
    Healthy: { icon: HeartIcon, color: 'text-emerald-600 dark:text-emerald-400' },
    Progressing: { icon: LoaderIcon, color: 'text-sky-600 dark:text-sky-400', spin: true },
    Degraded: { icon: HeartCrackIcon, color: 'text-destructive' },
    Suspended: { icon: PauseIcon, color: 'text-violet-600 dark:text-violet-400' },
    Missing: { icon: GhostIcon, color: 'text-amber-600 dark:text-amber-400' },
  };
  const SYNC: Record<string, Look> = {
    Synced: { icon: CheckIcon, color: 'text-emerald-600 dark:text-emerald-400' },
    OutOfSync: { icon: ArrowUpIcon, color: 'text-amber-600 dark:text-amber-400' },
  };
  const PHASE: Record<string, Look> = {
    Running: { icon: LoaderIcon, color: 'text-sky-600 dark:text-sky-400', spin: true },
    Terminating: { icon: StopIcon, color: 'text-amber-600 dark:text-amber-400' },
    Succeeded: { icon: CheckIcon, color: 'text-emerald-600 dark:text-emerald-400' },
    Failed: { icon: XIcon, color: 'text-destructive' },
    Error: { icon: XIcon, color: 'text-destructive' },
  };
  const UNKNOWN: Look = { icon: HelpIcon, color: 'text-muted-foreground' };

  export function look(what: 'health' | 'sync' | 'phase', value: string): Look {
    return ({ health: HEALTH, sync: SYNC, phase: PHASE }[what][value] ?? UNKNOWN);
  }

  /** Cor da faixa lateral dos cartões, pela saúde. */
  export function accent(health: string) {
    return (
      {
        Healthy: 'border-l-emerald-500',
        Progressing: 'border-l-sky-500',
        Degraded: 'border-l-destructive',
        Suspended: 'border-l-violet-500',
        Missing: 'border-l-amber-500',
      }[health] ?? 'border-l-border'
    );
  }
</script>

<script lang="ts">
  import { cn } from '$lib/utils.js';
  import { healthLabel, PHASES, syncLabel } from '../lib/argo.svelte';

  let {
    what,
    value,
    label = false,
    class: className = '',
  }: { what: 'health' | 'sync' | 'phase'; value: string; label?: boolean; class?: string } = $props();

  const l = $derived(look(what, value));
  const text = $derived(what === 'health' ? healthLabel(value) : what === 'sync' ? syncLabel(value) : (PHASES[value] ?? value));
</script>

<span class={cn('inline-flex min-w-0 items-center gap-1.5', l.color, className)} title={label ? undefined : text}>
  <l.icon class={cn('size-3.5 shrink-0', l.spin && 'animate-spin')} />
  {#if label}<span class="truncate">{text}</span>{/if}
</span>
