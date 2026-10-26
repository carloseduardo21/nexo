<script lang="ts">
  import Pane from './Pane.svelte';
  import PaneTree from './PaneTree.svelte';
  import { setRatio } from '../lib/state.svelte';
  import type { PaneNode } from '../lib/layout';

  let { node, tabId }: { node: PaneNode; tabId: string } = $props();
  let container = $state<HTMLDivElement>();

  function startDrag(e: PointerEvent) {
    if (node.type !== 'split') return;
    const split = node;
    const handle = e.currentTarget as HTMLElement;
    const rect = container!.getBoundingClientRect();
    handle.setPointerCapture(e.pointerId);
    document.body.classList.add(split.dir === 'row' ? 'resizing-col' : 'resizing-row');

    const move = (ev: PointerEvent) => {
      const ratio =
        split.dir === 'row' ? (ev.clientX - rect.left) / rect.width : (ev.clientY - rect.top) / rect.height;
      setRatio(split, Math.min(0.9, Math.max(0.1, ratio)));
    };
    const up = () => {
      handle.removeEventListener('pointermove', move);
      handle.removeEventListener('pointerup', up);
      document.body.classList.remove('resizing-col', 'resizing-row');
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', up);
  }
</script>

{#if node.type === 'leaf'}
  {#key node.id}
    <Pane paneId={node.id} {tabId} />
  {/key}
{:else}
  <div class="split {node.dir}" bind:this={container}>
    <div class="cell" style:flex="{node.ratio} 1 0"><PaneTree node={node.a} {tabId} /></div>
    <div class="divider" role="separator" onpointerdown={startDrag}></div>
    <div class="cell" style:flex="{1 - node.ratio} 1 0"><PaneTree node={node.b} {tabId} /></div>
  </div>
{/if}
