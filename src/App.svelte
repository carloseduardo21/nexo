<script lang="ts">
  import { onMount } from 'svelte';
  import TitleBar from './components/TitleBar.svelte';
  import Sidebar from './components/Sidebar.svelte';
  import PaneTree from './components/PaneTree.svelte';
  import Welcome from './components/Welcome.svelte';
  import Dialogs from './components/Dialogs.svelte';
  import SftpPanel from './components/SftpPanel.svelte';
  import { app, handleShortcut, init } from './lib/state.svelte';
  import { sftp } from './lib/sftp.svelte';

  onMount(() => {
    void init();
    window.addEventListener('keydown', handleShortcut, true);
    return () => window.removeEventListener('keydown', handleShortcut, true);
  });
</script>

<div class="app">
  <TitleBar />
  <div class="main">
    <Sidebar />
    <main class="workspace">
      {#each app.tabs as tab (tab.id)}
        <div class="tab-view" class:hidden={tab.id !== app.activeTab}>
          <PaneTree node={tab.root} tabId={tab.id} />
        </div>
      {/each}
      {#if app.tabs.length === 0}
        <Welcome />
      {/if}
    </main>
    {#if sftp.open}
      <SftpPanel />
    {/if}
  </div>
</div>
<Dialogs />
