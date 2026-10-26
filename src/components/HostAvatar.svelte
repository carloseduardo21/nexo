<script lang="ts">
  import TerminalIcon from '@lucide/svelte/icons/terminal';
  import MonitorIcon from '@lucide/svelte/icons/monitor';
  import type { HostView } from '../lib/backend';
  import { hue } from '../lib/ui';
  import { cn } from '$lib/utils.js';

  let { host, size = 'md' }: { host: HostView; size?: 'sm' | 'md' | 'lg' } = $props();
  const Icon = $derived(host.protocol === 'rdp' ? MonitorIcon : TerminalIcon);
</script>

<!-- Cor estável por host; o ícone diz o protocolo. -->
<span
  class={cn(
    'flex shrink-0 items-center justify-center rounded-lg text-white',
    size === 'sm' && 'size-7 rounded-md',
    size === 'md' && 'size-9',
    size === 'lg' && 'size-11 rounded-xl',
  )}
  style:background="linear-gradient(135deg, oklch(0.62 0.13 {hue(host.id)}), oklch(0.5 0.13 {(hue(host.id) + 40) % 360}))">
  <Icon class={size === 'sm' ? 'size-3.5' : size === 'lg' ? 'size-5' : 'size-4'} />
</span>
