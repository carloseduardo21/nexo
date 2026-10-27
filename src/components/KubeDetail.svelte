<script lang="ts">
  import XIcon from '@lucide/svelte/icons/x';
  import CopyIcon from '@lucide/svelte/icons/copy';
  import EyeIcon from '@lucide/svelte/icons/eye';
  import EyeOffIcon from '@lucide/svelte/icons/eye-off';
  import RefreshIcon from '@lucide/svelte/icons/refresh-cw';
  import { Button } from '$lib/components/ui/button/index.js';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import ScrollTextIcon from '@lucide/svelte/icons/scroll-text';
  import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu/index.js';
  import { actionsFor, k8s, kindInfo, loadYaml, openPodTerminal, toggleReveal } from '../lib/k8s.svelte';
  import { copyText } from '../lib/actions.svelte';

  const detail = $derived(k8s.detail!);
  const secret = $derived(detail.kind === 'secret');
  const items = $derived(actionsFor(detail.kind, detail.namespace, detail.name, { containers: detail.containers }));

  /** Realce simples por linha: chave, `- ` de lista e valores (número, booleano, nulo). */
  type Part = { text: string; class?: string };
  function highlight(line: string): Part[] {
    const m = /^(\s*)(- )?([^\s:#'"][^:]*?|"[^"]*"|'[^']*')(:)(\s.*|)$/.exec(line);
    if (!m) {
      const item = /^(\s*)(- )(.*)$/.exec(line);
      return item ? [{ text: item[1] }, { text: item[2], class: 'text-muted-foreground' }, ...value(item[3])] : value(line);
    }
    const [, indent, dash, key, colon, rest] = m;
    return [
      { text: indent },
      ...(dash ? [{ text: dash, class: 'text-muted-foreground' }] : []),
      { text: key, class: 'text-sky-700 dark:text-sky-300' },
      { text: colon, class: 'text-muted-foreground' },
      ...value(rest),
    ];
  }
  function value(text: string): Part[] {
    const trimmed = text.trim();
    if (/^(true|false|null|~)$/.test(trimmed)) return [{ text, class: 'text-violet-700 dark:text-violet-300' }];
    if (/^-?\d+(\.\d+)?$/.test(trimmed)) return [{ text, class: 'text-amber-700 dark:text-amber-300' }];
    if (/^[|>][-+]?$/.test(trimmed)) return [{ text, class: 'text-muted-foreground' }];
    return [{ text, class: 'text-emerald-700 dark:text-emerald-300' }];
  }

  const lines = $derived((detail.yaml ?? '').replace(/\n$/, '').split('\n'));

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && !e.defaultPrevented && !document.querySelector('[role=dialog],[role=menu]')) k8s.detail = null;
  }
</script>

<svelte:window onkeydown={onKeydown} />

<aside class="flex w-[min(42rem,48%)] shrink-0 flex-col border-l bg-sidebar">
  <header class="flex h-12 shrink-0 items-center gap-2 border-b pr-1.5 pl-4">
    <div class="min-w-0 flex-1">
      <div class="truncate text-sm font-medium" title={detail.name}>{detail.name}</div>
      <div class="truncate text-[11px] text-muted-foreground">{kindInfo(detail.kind).label}{detail.namespace ? ` · ${detail.namespace}` : ''}</div>
    </div>
    {#if detail.kind === 'pod'}
      <Button variant="ghost" size="icon-sm" title="Logs" onclick={() => openPodTerminal(detail.namespace, detail.name, { type: 'logs', previous: false })}>
        <ScrollTextIcon />
      </Button>
      <Button variant="ghost" size="icon-sm" title="Shell" onclick={() => openPodTerminal(detail.namespace, detail.name, { type: 'shell' })}>
        <SquareTerminalIcon />
      </Button>
    {/if}
    {#if secret}
      <Button variant="ghost" size="icon-sm" title={detail.reveal ? 'Esconder valores' : 'Revelar valores'} onclick={toggleReveal}>
        {#if detail.reveal}<EyeOffIcon />{:else}<EyeIcon />{/if}
      </Button>
    {/if}
    <Button variant="ghost" size="icon-sm" title="Copiar YAML" disabled={!detail.yaml} onclick={() => detail.yaml && copyText(detail.yaml)}><CopyIcon /></Button>
    <Button variant="ghost" size="icon-sm" title="Recarregar" onclick={() => void loadYaml()}><RefreshIcon /></Button>
    <DropdownMenu.Root>
      <DropdownMenu.Trigger>
        {#snippet child({ props })}
          <Button {...props} variant="ghost" size="icon-sm" title="Ações"><EllipsisIcon /></Button>
        {/snippet}
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="end">
        {#each items as item, i (i)}
          {#if item}
            <DropdownMenu.Item variant={item.destructive ? 'destructive' : 'default'} onSelect={item.run}>{item.label}</DropdownMenu.Item>
          {:else}
            <DropdownMenu.Separator />
          {/if}
        {/each}
      </DropdownMenu.Content>
    </DropdownMenu.Root>
    <Button variant="ghost" size="icon-sm" title="Fechar (Esc)" onclick={() => (k8s.detail = null)}><XIcon /></Button>
  </header>

  {#if detail.error}
    <p class="p-4 text-sm text-destructive">{detail.error}</p>
  {:else if detail.yaml === null}
    <p class="p-4 text-sm text-muted-foreground">Carregando…</p>
  {:else}
    <pre class="min-h-0 flex-1 overflow-auto p-4 font-mono text-xs leading-relaxed select-text">{#each lines as line, i (i)}{#each highlight(line) as part, j (j)}<span class={part.class}>{part.text}</span>{/each}{'\n'}{/each}</pre>
  {/if}
</aside>
