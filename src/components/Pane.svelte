<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import { app, closePane, fillSecret, getPane, secretCandidates, setFocus, toggleMute } from '../lib/state.svelte';

  let { paneId, tabId }: { paneId: string; tabId: string } = $props();
  let body: HTMLDivElement;

  const tab = $derived(app.tabs.find((t) => t.id === tabId));
  const meta = $derived(app.meta[paneId]);
  const focused = $derived(tab?.focused === paneId);
  const isSplit = $derived(tab?.root.type === 'split');
  const receiving = $derived(!!tab?.broadcast && !meta?.muted);

  onMount(() => {
    const pane = getPane(paneId);
    if (!pane) return;
    pane.attach(body);
    void pane.start();

    let frame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => pane.fit());
    });
    observer.observe(body);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  });

  $effect(() => {
    if (focused && app.activeTab === tabId) getPane(paneId)?.focus();
  });
</script>

<div
  class="pane"
  class:focused={focused && isSplit}
  class:receiving
  data-pane={paneId}
  role="presentation"
  onmousedown={() => setFocus(paneId)}>
  {#if isSplit || tab?.broadcast}
    <div class="pane-header">
      <span class="kind-dot kind-{meta?.kind}"></span>
      <span class="title">{meta?.title}</span>
      {#if meta?.exited}<span class="badge">encerrado</span>{/if}
      {#if tab?.broadcast}
        <button
          class="chip"
          class:off={meta?.muted}
          title="Incluir ou excluir este painel do broadcast"
          onclick={() => toggleMute(paneId)}>{meta?.muted ? 'fora do broadcast' : 'recebendo'}</button>
      {/if}
      <button class="pane-close" title="Fechar painel (Ctrl+Shift+W)" onclick={() => closePane(paneId)}>
        <Icon name="x" size={12} />
      </button>
    </div>
  {/if}
  <div class="pane-body" bind:this={body}></div>
  {#if meta?.passwordPrompt && focused && app.activeTab === tabId}
    {@const prompt = meta.passwordPrompt}
    <div
      class="secret-popup"
      role="listbox"
      tabindex="-1"
      style:left="{Math.min(prompt.x, innerWidth - 300)}px"
      style:top="{Math.min(prompt.y + 4, innerHeight - 220)}px"
      onmousedown={(e) => e.preventDefault()}>
      <div class="secret-head"><Icon name="lock" size={12} /> Senha salva</div>
      {#each secretCandidates(paneId) as host, i (host.id)}
        <button
          class="secret-item"
          class:active={i === prompt.index}
          role="option"
          aria-selected={i === prompt.index}
          onmouseenter={() => (prompt.index = i)}
          onclick={() => fillSecret(paneId, host.id)}>
          <span class="secret-label">{host.label}</span>
          <span class="secret-sub">{host.username}@{host.hostname}</span>
        </button>
      {/each}
      <div class="secret-hint"><kbd>↑↓</kbd> escolher <kbd>Enter</kbd> preencher <kbd>Esc</kbd> ignorar</div>
    </div>
  {/if}
  {#if meta?.kind === 'ssh' && meta.latency.echo !== undefined && !meta.exited}
    {@const { echo, network } = meta.latency}
    <div
      class="latency"
      class:slow={echo > 80}
      title={`Tecla até o eco: ${echo.toFixed(1)} ms\nRede + servidor: ${network?.toFixed(1) ?? '—'} ms\nNo app: ${network !== undefined ? Math.max(0, echo - network).toFixed(1) : '—'} ms`}>
      {Math.round(echo)} ms
    </div>
  {/if}
</div>
