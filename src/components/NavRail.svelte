<script lang="ts">
  import ServerIcon from '@lucide/svelte/icons/server';
  import KeyRoundIcon from '@lucide/svelte/icons/key-round';
  import WaypointsIcon from '@lucide/svelte/icons/waypoints';
  import SettingsIcon from '@lucide/svelte/icons/settings';
  import PanelLeftIcon from '@lucide/svelte/icons/panel-left';
  import FolderIcon from '@lucide/svelte/icons/folder-open';
  import ActivityIcon from '@lucide/svelte/icons/activity';
  import * as Tooltip from '$lib/components/ui/tooltip/index.js';
  import { app, showView, type View } from '../lib/state.svelte';
  import { settings, updateSettings } from '../lib/settings.svelte';
  import { side, togglePanel } from '../lib/side.svelte';
  import { cn } from '$lib/utils.js';

  const items: { view: View; label: string; icon: typeof ServerIcon }[] = [
    { view: 'hosts', label: 'Hosts', icon: ServerIcon },
    { view: 'credentials', label: 'Credenciais', icon: KeyRoundIcon },
    { view: 'tunnels', label: 'Túneis', icon: WaypointsIcon },
  ];

  const runningTunnels = $derived(Object.values(app.tunnelStatus).filter((s) => s.state === 'running').length);
</script>

{#snippet railButton(label: string, active: boolean, onclick: () => void, Icon: typeof ServerIcon, badge?: number)}
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <button
          {...props}
          {onclick}
          aria-label={label}
          class={cn(
            'relative flex size-10 items-center justify-center rounded-lg text-sidebar-foreground/60 transition-colors hover:bg-sidebar-accent hover:text-sidebar-foreground',
            active && 'bg-sidebar-accent text-sidebar-primary',
          )}>
          <Icon class="size-[18px]" />
          {#if badge}
            <span class="absolute top-1 right-1 flex size-4 items-center justify-center rounded-full bg-sidebar-primary text-[9px] font-semibold text-sidebar-primary-foreground">{badge}</span>
          {/if}
        </button>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content side="right">{label}</Tooltip.Content>
  </Tooltip.Root>
{/snippet}

<Tooltip.Provider delayDuration={300}>
  <nav class="flex w-14 shrink-0 flex-col items-center gap-1 border-r bg-sidebar py-2">
    {#each items as item (item.view)}
      {@render railButton(item.label, app.view === item.view, () => showView(item.view), item.icon, item.view === 'tunnels' ? runningTunnels : undefined)}
    {/each}

    <div class="flex-1"></div>

    {#if app.view === 'session'}
      {@render railButton(settings.hostsSidebar ? 'Esconder lista de hosts' : 'Mostrar lista de hosts', settings.hostsSidebar, () => updateSettings({ hostsSidebar: !settings.hostsSidebar }), PanelLeftIcon)}
      {@render railButton('Arquivos SFTP (Ctrl+Shift+F)', side.panel === 'sftp', () => togglePanel('sftp'), FolderIcon)}
      {@render railButton('Painel do host (Ctrl+Shift+M)', side.panel === 'host', () => togglePanel('host'), ActivityIcon)}
    {/if}
    {@render railButton('Configurações', app.view === 'settings', () => showView('settings'), SettingsIcon)}
  </nav>
</Tooltip.Provider>
