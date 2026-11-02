<script lang="ts" module>
  import BoxIcon from '@lucide/svelte/icons/box';
  import BoxesIcon from '@lucide/svelte/icons/boxes';
  import LayersIcon from '@lucide/svelte/icons/layers';
  import DatabaseIcon from '@lucide/svelte/icons/database';
  import ServerIcon from '@lucide/svelte/icons/server';
  import NetworkIcon from '@lucide/svelte/icons/network';
  import GlobeIcon from '@lucide/svelte/icons/globe';
  import FileTextIcon from '@lucide/svelte/icons/file-text';
  import KeyRoundIcon from '@lucide/svelte/icons/key-round';
  import ClockIcon from '@lucide/svelte/icons/clock';
  import PlayIcon from '@lucide/svelte/icons/play';
  import HardDriveIcon from '@lucide/svelte/icons/hard-drive';
  import UserIcon from '@lucide/svelte/icons/user';
  import ShieldIcon from '@lucide/svelte/icons/shield';
  import FolderIcon from '@lucide/svelte/icons/folder';
  import GaugeIcon from '@lucide/svelte/icons/gauge';
  import RocketIcon from '@lucide/svelte/icons/rocket';

  const ICONS: Record<string, typeof BoxIcon> = {
    Pod: BoxIcon,
    Deployment: LayersIcon,
    ReplicaSet: LayersIcon,
    Rollout: LayersIcon,
    StatefulSet: DatabaseIcon,
    DaemonSet: ServerIcon,
    Service: NetworkIcon,
    Ingress: GlobeIcon,
    HTTPRoute: GlobeIcon,
    ConfigMap: FileTextIcon,
    Secret: KeyRoundIcon,
    SealedSecret: KeyRoundIcon,
    ExternalSecret: KeyRoundIcon,
    CronJob: ClockIcon,
    Job: PlayIcon,
    PersistentVolumeClaim: HardDriveIcon,
    ServiceAccount: UserIcon,
    Role: ShieldIcon,
    RoleBinding: ShieldIcon,
    ClusterRole: ShieldIcon,
    ClusterRoleBinding: ShieldIcon,
    NetworkPolicy: ShieldIcon,
    Namespace: FolderIcon,
    HorizontalPodAutoscaler: GaugeIcon,
    Application: RocketIcon,
  };
  export const kindIcon = (kind: string) => ICONS[kind] ?? BoxesIcon;
</script>

<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity';
  import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
  import * as ContextMenu from '$lib/components/ui/context-menu/index.js';
  import { cn } from '$lib/utils.js';
  import ArgoStatus, { accent } from './ArgoStatus.svelte';
  import type { ArgoApp, ArgoNode } from '../lib/backend';
  import { ARGO, k8s, openDetail } from '../lib/k8s.svelte';
  import { nodeMenu, nodeKey, nodeMatches, shortRevision } from '../lib/argo.svelte';

  let { app, nodes, query, problems, showOld }: { app: ArgoApp; nodes: ArgoNode[]; query: string; problems: boolean; showOld: boolean } = $props();

  const W = 248;
  const H = 54;
  const GAP_X = 52;
  const GAP_Y = 10;
  const ROOT = '';

  /** Recolhidos pelo usuário (continua assim enquanto o app está aberto). */
  const collapsed = new SvelteSet<string>();

  type Item = { id: string; node: ArgoNode | null; depth: number; y: number; children: number; hidden: number };

  const layout = $derived.by(() => {
    const visible = nodes.filter((n) => showOld || !n.old);
    const ids = new Set(visible.map((n) => n.id));
    const kids = new Map<string, ArgoNode[]>();
    for (const n of visible) {
      const parent = n.parent && ids.has(n.parent) ? n.parent : ROOT;
      kids.set(parent, [...(kids.get(parent) ?? []), n]);
    }
    const count = (id: string): number => (kids.get(id) ?? []).reduce((sum, c) => sum + 1 + count(c.id), 0);
    const items: Item[] = [];
    const edges: { from: Item; to: Item }[] = [];
    let row = 0;
    let depth = 0;
    // Folhas uma por linha; cada pai no meio dos filhos.
    function place(id: string, node: ArgoNode | null, d: number): Item {
      const children = kids.get(id) ?? [];
      const item: Item = { id, node, depth: d, y: 0, children: children.length, hidden: 0 };
      items.push(item);
      depth = Math.max(depth, d);
      if (!children.length || collapsed.has(id)) {
        item.hidden = children.length ? count(id) : 0;
        item.y = row++;
        return item;
      }
      const placed = children.map((c) => {
        const child = place(c.id, c, d + 1);
        edges.push({ from: item, to: child });
        return child;
      });
      item.y = (placed[0].y + placed[placed.length - 1].y) / 2;
      return item;
    }
    place(ROOT, null, 0);
    return { items, edges, rows: row, depth };
  });

  const x = (depth: number) => depth * (W + GAP_X);
  const y = (row: number) => row * (H + GAP_Y);

  function edgePath(from: Item, to: Item) {
    const [x1, y1, x2, y2] = [x(from.depth) + W, y(from.y) + H / 2, x(to.depth), y(to.y) + H / 2];
    const mid = (x1 + x2) / 2;
    return `M ${x1} ${y1} C ${mid} ${y1}, ${mid} ${y2}, ${x2} ${y2}`;
  }

  const dimmed = (node: ArgoNode | null) => !!node && !nodeMatches(node, query, problems);

  function selected(node: ArgoNode | null) {
    const d = k8s.detail;
    if (!d) return false;
    if (!node) return d.kind === ARGO && d.name === app.name && d.namespace === app.namespace;
    return d.name === node.name && d.namespace === node.namespace && d.kind === nodeKey(node);
  }

  function open(node: ArgoNode | null) {
    if (!node) return openDetail(ARGO, app.namespace, app.name);
    const key = nodeKey(node);
    if (key) openDetail(key, node.namespace, node.name);
  }

  function toggle(id: string, e: MouseEvent) {
    e.stopPropagation();
    if (collapsed.has(id)) collapsed.delete(id);
    else collapsed.add(id);
  }

  function tooltip(n: ArgoNode) {
    return [
      `${n.kind} ${n.namespace ? `${n.namespace}/` : ''}${n.name}`,
      n.managed ? '' : 'criado pelo cluster',
      n.healthMessage,
      n.result,
      n.requiresPruning ? 'Saiu do git: será removido num sync com prune' : '',
      ...n.images.map((i) => `imagem: ${i}`),
    ]
      .filter(Boolean)
      .join('\n');
  }
</script>

<div class="relative" style:width={`${x(layout.depth) + W + 24}px`} style:height={`${y(layout.rows) + 8}px`}>
  <svg class="pointer-events-none absolute inset-0 overflow-visible" width="100%" height="100%" aria-hidden="true">
    {#each layout.edges as e (e.to.id)}
      <path d={edgePath(e.from, e.to)} class={cn('fill-none stroke-border', dimmed(e.to.node) && 'opacity-40')} stroke-width="1.5" />
    {/each}
  </svg>

  {#each layout.items as item (item.id || '__app')}
    {@const n = item.node}
    {@const Icon = kindIcon(n ? n.kind : 'Application')}
    <div class="absolute" style:left={`${x(item.depth)}px`} style:top={`${y(item.y)}px`} style:width={`${W}px`} style:height={`${H}px`}>
      <ContextMenu.Root>
        <ContextMenu.Trigger>
          {#snippet child({ props })}
            <button
              {...props}
              class={cn(
                'flex size-full items-center gap-2.5 rounded-md border border-l-[3px] bg-card px-2.5 text-left shadow-xs transition-[opacity,border-color] hover:border-primary/60',
                accent(n ? n.health : app.health),
                n?.old && 'border-dashed opacity-60',
                dimmed(n) && 'opacity-35',
                selected(n) && 'ring-2 ring-primary',
              )}
              title={n ? tooltip(n) : `Application ${app.namespace}/${app.name}`}
              onclick={() => open(n)}>
              <span class={cn('grid size-7 shrink-0 place-items-center rounded-md', n ? 'bg-muted text-muted-foreground' : 'bg-primary/10 text-primary')}>
                <Icon class="size-4" />
              </span>
              <span class="min-w-0 flex-1">
                <span class="flex items-center gap-1.5">
                  <span class="truncate text-[13px] font-medium">{n ? n.name : app.name}</span>
                </span>
                <span class="flex items-center gap-1 text-[11px] text-muted-foreground">
                  {#if n}
                    <span class="shrink-0">{n.kind}</span>
                    {#if n.result}
                      <span class="truncate text-destructive">· {n.result}</span>
                    {:else if n.requiresPruning}
                      <span class="truncate text-amber-600 dark:text-amber-400">· remover</span>
                    {:else if n.info.length}
                      <span class="truncate">· {n.info.join(' · ')}</span>
                    {/if}
                    {#if n.hook}<span class="shrink-0 rounded bg-muted px-1">hook</span>{/if}
                  {:else}
                    <span class="truncate">{app.project} · {shortRevision(app.revisions[0] ?? '')}</span>
                  {/if}
                </span>
              </span>
              <span class="flex shrink-0 flex-col items-end gap-0.5">
                {#if n ? n.health : app.health}<ArgoStatus what="health" value={n ? n.health : app.health} />{/if}
                {#if n ? n.managed : true}<ArgoStatus what="sync" value={n ? n.sync : app.sync} />{/if}
              </span>
            </button>
          {/snippet}
        </ContextMenu.Trigger>
        <ContextMenu.Content>
          {#each nodeMenu(app, n) as entry, i (i)}
            {#if entry}
              <ContextMenu.Item variant={entry.destructive ? 'destructive' : 'default'} onSelect={entry.run}>{entry.label}</ContextMenu.Item>
            {:else}
              <ContextMenu.Separator />
            {/if}
          {/each}
        </ContextMenu.Content>
      </ContextMenu.Root>
      {#if item.children}
        <button
          class="absolute top-1/2 -right-2.5 z-10 flex h-5 min-w-5 -translate-y-1/2 items-center justify-center rounded-full border bg-background px-1 text-[10px] text-muted-foreground shadow-xs hover:text-foreground"
          title={item.hidden ? `Mostrar ${item.hidden}` : 'Recolher'}
          onclick={(e) => toggle(item.id, e)}>
          {#if item.hidden}+{item.hidden}{:else}<ChevronRightIcon class="size-3 rotate-180" />{/if}
        </button>
      {/if}
    </div>
  {/each}
</div>
