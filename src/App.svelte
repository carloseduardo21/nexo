<script lang="ts">
  import { onMount } from 'svelte';
  import { Toaster } from '$lib/components/ui/sonner/index.js';
  import TitleBar from './components/TitleBar.svelte';
  import NavRail from './components/NavRail.svelte';
  import HostsSidebar from './components/HostsSidebar.svelte';
  import HostsPage from './components/HostsPage.svelte';
  import CredentialsPage from './components/CredentialsPage.svelte';
  import TunnelsPage from './components/TunnelsPage.svelte';
  import SettingsPage from './components/SettingsPage.svelte';
  import PaneTree from './components/PaneTree.svelte';
  import Dialogs from './components/Dialogs.svelte';
  import SftpPanel from './components/SftpPanel.svelte';
  import HostPanel from './components/HostPanel.svelte';
  import { app, handleShortcut, init } from './lib/state.svelte';
  import { settings } from './lib/settings.svelte';
  import { side } from './lib/side.svelte';

  onMount(() => {
    void init();
    window.addEventListener('keydown', handleShortcut, true);
    return () => window.removeEventListener('keydown', handleShortcut, true);
  });

  const session = $derived(app.view === 'session');
</script>

<div class="flex h-full flex-col">
  <TitleBar />
  <div class="flex min-h-0 flex-1">
    <NavRail />
    {#if session && settings.hostsSidebar}
      <HostsSidebar />
    {/if}
    <main class="relative min-w-0 flex-1 bg-background">
      {#if app.view === 'hosts'}
        <HostsPage />
      {:else if app.view === 'credentials'}
        <CredentialsPage />
      {:else if app.view === 'tunnels'}
        <TunnelsPage />
      {:else if app.view === 'settings'}
        <SettingsPage />
      {/if}
      <!-- As sessões ficam montadas mesmo nas outras páginas: terminais e áreas remotas não caem. -->
      <div class="workspace absolute inset-0" class:hidden={!session}>
        {#each app.tabs as tab (tab.id)}
          <div class="tab-view" class:hidden={tab.id !== app.activeTab}>
            <PaneTree node={tab.root} tabId={tab.id} />
          </div>
        {/each}
      </div>
    </main>
    {#if session && side.panel === 'sftp'}
      <SftpPanel />
    {:else if session && side.panel === 'host'}
      <HostPanel />
    {/if}
  </div>
</div>
<Dialogs />
<Toaster position="bottom-right" richColors closeButton />
