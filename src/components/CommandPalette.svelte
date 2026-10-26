<script lang="ts">
  import ServerIcon from '@lucide/svelte/icons/server';
  import KeyRoundIcon from '@lucide/svelte/icons/key-round';
  import WaypointsIcon from '@lucide/svelte/icons/waypoints';
  import SettingsIcon from '@lucide/svelte/icons/settings';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
  import * as Command from '$lib/components/ui/command/index.js';
  import HostAvatar from './HostAvatar.svelte';
  import { app, closeDialog, connectHost, localSpec, newTab, openDialog, showTab, showView } from '../lib/state.svelte';
  import { address } from '../lib/ui';

  /** Fecha a paleta e só então executa (a ação pode abrir outro diálogo). */
  function run(action: () => void) {
    closeDialog(null);
    queueMicrotask(action);
  }
</script>

<Command.Dialog open onOpenChange={(open) => !open && closeDialog(null)} title="Buscar" description="Conectar a um host, abrir terminal ou ir para uma página">
  <Command.Input placeholder="Conectar a um host, abrir terminal…" />
  <Command.List>
    <Command.Empty>Nada encontrado.</Command.Empty>
    {#if app.hosts.length}
      <Command.Group heading="Hosts">
        {#each [...app.hosts].sort((a, b) => a.label.localeCompare(b.label)) as host (host.id)}
          <Command.Item value={`${host.label} ${host.hostname} ${host.username} ${host.group}`} onSelect={() => run(() => connectHost(host))}>
            <HostAvatar {host} size="sm" />
            <span class="min-w-0 flex-1 truncate">{host.label}</span>
            <span class="max-w-[50%] shrink-0 truncate text-xs text-muted-foreground">{address(host)}</span>
          </Command.Item>
        {/each}
      </Command.Group>
    {/if}
    {#if app.tabs.length}
      <Command.Group heading="Abas abertas">
        {#each app.tabs as tab (tab.id)}
          <Command.Item value={`aba ${app.meta[tab.focused]?.title ?? ''} ${tab.id}`} onSelect={() => run(() => showTab(tab.id))}>
            <SquareTerminalIcon /> {app.meta[tab.focused]?.title ?? 'Terminal'}
          </Command.Item>
        {/each}
      </Command.Group>
    {/if}
    <Command.Group heading="Terminal local">
      {#each app.profiles as profile (profile.id)}
        <Command.Item value={`terminal ${profile.name}`} onSelect={() => run(() => newTab(localSpec(profile)))}>
          <SquareTerminalIcon /> {profile.name}
        </Command.Item>
      {/each}
    </Command.Group>
    <Command.Group heading="Ir para">
      <Command.Item value="novo host" onSelect={() => run(() => void openDialog({ type: 'hostForm', host: null }))}><PlusIcon /> Novo host</Command.Item>
      <Command.Item value="hosts" onSelect={() => run(() => showView('hosts'))}><ServerIcon /> Hosts</Command.Item>
      <Command.Item value="credenciais" onSelect={() => run(() => showView('credentials'))}><KeyRoundIcon /> Credenciais</Command.Item>
      <Command.Item value="túneis tuneis" onSelect={() => run(() => showView('tunnels'))}><WaypointsIcon /> Túneis</Command.Item>
      <Command.Item value="configurações configuracoes tema" onSelect={() => run(() => showView('settings'))}><SettingsIcon /> Configurações</Command.Item>
    </Command.Group>
  </Command.List>
</Command.Dialog>
