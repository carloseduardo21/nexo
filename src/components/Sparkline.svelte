<script lang="ts">
  /** Mini-gráfico de área (0–100), na cor do texto (`currentColor`). */
  let { values, max = 100, points = 60, class: className = '' }: { values: number[]; max?: number; points?: number; class?: string } = $props();

  const W = 100;
  const H = 30;

  const line = $derived.by(() => {
    if (values.length < 2) return '';
    // Ancorado à direita: o gráfico enche da direita para a esquerda conforme chegam amostras.
    const step = W / (points - 1);
    const offset = (points - values.length) * step;
    return values.map((v, i) => `${(offset + i * step).toFixed(2)},${(H - (Math.min(v, max) / max) * H).toFixed(2)}`).join(' ');
  });
  const start = $derived(line.split(' ')[0]?.split(',')[0] ?? '0');
</script>

<svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" class={className} aria-hidden="true">
  {#if line}
    <polygon points="{start},{H} {line} {W},{H}" fill="currentColor" opacity="0.15" />
    <polyline points={line} fill="none" stroke="currentColor" stroke-width="1.5" vector-effect="non-scaling-stroke" stroke-linejoin="round" />
  {/if}
</svg>
