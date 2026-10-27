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
  import PencilIcon from '@lucide/svelte/icons/pencil';
  import { actionsFor, k8s, kindInfo, loadYaml, openPodTerminal, saveYaml, toggleReveal } from '../lib/k8s.svelte';
  import { copyText } from '../lib/actions.svelte';

  const detail = $derived(k8s.detail!);
  const secret = $derived(detail.kind === 'secret');
  /** Editando: o texto (null = só leitura). */
  let draft = $state<string | null>(null);
  let saveError = $state('');
  let saving = $state(false);
  const hidden = $derived(secret && !detail.reveal);

  // Outro item aberto: sai da edição.
  $effect(() => {
    void [detail.kind, detail.namespace, detail.name];
    draft = null;
    saveError = '';
  });

  function startEdit() {
    if (detail.yaml === null || hidden) return;
    draft = detail.yaml;
    saveError = '';
  }

  async function save() {
    if (draft === null || saving) return;
    saving = true;
    const error = await saveYaml(draft);
    saving = false;
    if (error) saveError = error;
    else draft = null;
  }

  function editorKey(e: KeyboardEvent) {
    const area = e.currentTarget as HTMLTextAreaElement;
    if ((e.ctrlKey || e.metaKey) && e.code === 'KeyS') {
      e.preventDefault();
      void save();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      draft = null;
    } else if (e.key === 'Tab' && !e.shiftKey) {
      // YAML usa espaços: Tab indenta em vez de sair do campo.
      e.preventDefault();
      area.setRangeText('  ', area.selectionStart, area.selectionEnd, 'end');
      draft = area.value;
    }
  }

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
    if (e.key === 'Escape' && draft === null && !e.defaultPrevented && !document.querySelector('[role=dialog],[role=menu]')) k8s.detail = null;
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
    <Button
      variant="ghost"
      size="icon-sm"
      title={hidden ? 'Revele os valores para editar' : 'Editar YAML'}
      disabled={detail.yaml === null || hidden || draft !== null}
      onclick={startEdit}>
      <PencilIcon />
    </Button>
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

  {#if draft !== null}
    <div class="flex min-h-0 flex-1 flex-col">
      {#if saveError}
        <p class="border-b bg-destructive/10 px-4 py-2 text-sm text-destructive">{saveError}</p>
      {/if}
      <!-- svelte-ignore a11y_autofocus -->
      <textarea
        class="min-h-0 flex-1 resize-none bg-background p-4 font-mono text-xs leading-relaxed outline-none"
        spellcheck="false"
        autofocus
        bind:value={draft}
        onkeydown={editorKey}></textarea>
      <div class="flex items-center gap-2 border-t px-4 py-2">
        <span class="flex-1 text-[11px] text-muted-foreground">Ctrl+S salva · Esc cancela</span>
        <Button variant="outline" size="sm" onclick={() => (draft = null)}>Cancelar</Button>
        <Button size="sm" disabled={saving} onclick={save}>{saving ? 'Salvando…' : 'Salvar'}</Button>
      </div>
    </div>
  {:else if detail.error}
    <p class="p-4 text-sm text-destructive">{detail.error}</p>
  {:else if detail.yaml === null}
    <p class="p-4 text-sm text-muted-foreground">Carregando…</p>
  {:else}
    <pre class="min-h-0 flex-1 overflow-auto p-4 font-mono text-xs leading-relaxed select-text">{#each lines as line, i (i)}{#each highlight(line) as part, j (j)}<span class={part.class}>{part.text}</span>{/each}{'\n'}{/each}</pre>
  {/if}
</aside>
