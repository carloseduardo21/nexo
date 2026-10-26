<script lang="ts">
  import { cn } from '$lib/utils.js';

  let {
    value,
    options,
    onchange,
    class: className = '',
  }: { value: string; options: { value: string; label: string }[]; onchange: (value: string) => void; class?: string } = $props();
</script>

<!-- Escolha única em botões lado a lado (protocolo, método de autenticação, tipo de túnel…). -->
<div class={cn('flex rounded-lg bg-muted p-0.5', className)} role="radiogroup">
  {#each options as option (option.value)}
    <button
      type="button"
      role="radio"
      aria-checked={value === option.value}
      class={cn(
        'flex-1 rounded-md px-3 py-1.5 text-[13px] text-muted-foreground transition-colors hover:text-foreground',
        value === option.value && 'bg-background text-foreground shadow-sm',
      )}
      onclick={() => onchange(option.value)}>{option.label}</button>
  {/each}
</div>
