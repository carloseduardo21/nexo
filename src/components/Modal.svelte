<script lang="ts">
  import type { Snippet } from 'svelte';
  import { closeDialog } from '../lib/state.svelte';

  let { title, width = 420, children }: { title: string; width?: number; children: Snippet } = $props();
</script>

<div class="modal-backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && closeDialog(null)}>
  <div class="modal" role="dialog" aria-modal="true" aria-label={title} style:width="{width}px">
    <h2>{title}</h2>
    {@render children()}
  </div>
</div>
