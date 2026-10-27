<script lang="ts">
  import type { Snippet } from 'svelte';
  import CheckIcon from '@lucide/svelte/icons/check';
  import ChevronsUpDownIcon from '@lucide/svelte/icons/chevrons-up-down';
  import * as Popover from '$lib/components/ui/popover/index.js';
  import * as Command from '$lib/components/ui/command/index.js';
  import { cn } from '$lib/utils.js';

  export interface PickerItem {
    value: string;
    label: string;
    hint?: string;
  }

  let {
    items,
    value,
    onSelect,
    placeholder = 'Buscar',
    empty = 'Nada encontrado.',
    /** Aceita um valor digitado que não está na lista. */
    custom,
    trigger,
    class: className,
  }: {
    items: PickerItem[];
    value: string;
    onSelect: (value: string) => void;
    placeholder?: string;
    empty?: string;
    custom?: (typed: string) => string | null;
    trigger: Snippet;
    class?: string;
  } = $props();

  let open = $state(false);
  let search = $state('');
  const typed = $derived(custom && search.trim() && !items.some((i) => i.value === search.trim()) ? custom(search.trim()) : null);

  function pick(v: string) {
    open = false;
    search = '';
    onSelect(v);
  }
</script>

<Popover.Root bind:open>
  <Popover.Trigger>
    {#snippet child({ props })}
      <button
        {...props}
        class={cn(
          'flex w-full items-center gap-2 rounded-md border border-input bg-background px-2 py-1.5 text-left text-sm hover:bg-accent dark:bg-input/30 dark:hover:bg-input/50',
          className,
        )}>
        <span class="min-w-0 flex-1">{@render trigger()}</span>
        <ChevronsUpDownIcon class="size-3.5 shrink-0 text-muted-foreground" />
      </button>
    {/snippet}
  </Popover.Trigger>
  <Popover.Content class="w-72 p-0" align="start">
    <Command.Root>
      <Command.Input {placeholder} bind:value={search} />
      <Command.List class="max-h-72">
        <Command.Empty>{empty}</Command.Empty>
        {#if typed}
          <Command.Item value={`\0${search}`} onSelect={() => pick(search.trim())}>{typed}</Command.Item>
        {/if}
        {#each items as item (item.value)}
          <Command.Item value={`${item.label} ${item.hint ?? ''} ${item.value}`} onSelect={() => pick(item.value)}>
            <CheckIcon class={cn('size-3.5', item.value !== value && 'opacity-0')} />
            <span class="min-w-0 flex-1">
              <span class="block truncate">{item.label}</span>
              {#if item.hint}<span class="block truncate text-xs text-muted-foreground">{item.hint}</span>{/if}
            </span>
          </Command.Item>
        {/each}
      </Command.List>
    </Command.Root>
  </Popover.Content>
</Popover.Root>
