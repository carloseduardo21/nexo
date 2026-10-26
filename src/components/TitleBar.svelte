<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import Icon from './Icon.svelte';
  import { app, activeTab, closeTab, localSpec, newTab, split, toggleBroadcast } from '../lib/state.svelte';
  import { leaves } from '../lib/layout';
  import { sftp, toggleSftp } from '../lib/sftp.svelte';

  const win = getCurrentWindow();
  let menuOpen = $state(false);
  const current = $derived(activeTab());
</script>

<header class="titlebar" data-tauri-drag-region>
  <div class="brand" data-tauri-drag-region>
    <img src="/logo.svg" alt="" width="18" height="18" />
    <span data-tauri-drag-region>Nexo</span>
  </div>

  <nav class="tabs" data-tauri-drag-region>
    {#each app.tabs as tab (tab.id)}
      {@const count = leaves(tab.root).length}
      {@const meta = app.meta[tab.focused]}
      <div
        class="tab"
        class:active={tab.id === app.activeTab}
        class:broadcast={tab.broadcast}
        role="tab"
        tabindex="-1"
        aria-selected={tab.id === app.activeTab}
        onmousedown={(e) => e.button === 0 && (app.activeTab = tab.id)}
        onauxclick={(e) => e.button === 1 && closeTab(tab.id)}>
        <span class="kind-dot kind-{meta?.kind}"></span>
        <span class="label">{meta?.title ?? 'Terminal'}</span>
        {#if count > 1}<span class="count">{count}</span>{/if}
        <button
          class="tab-close"
          title="Fechar aba"
          onmousedown={(e) => e.stopPropagation()}
          onclick={() => closeTab(tab.id)}><Icon name="x" size={12} /></button>
      </div>
    {/each}
    <div class="new-tab">
      <button title="Nova aba (Ctrl+Shift+T)" onclick={() => newTab()}><Icon name="plus" size={15} /></button>
      <button title="Escolher shell" onclick={() => (menuOpen = !menuOpen)}><Icon name="chevron" size={13} /></button>
      {#if menuOpen}
        <div class="backdrop" role="presentation" onclick={() => (menuOpen = false)}></div>
        <ul class="menu">
          {#each app.profiles as profile (profile.id)}
            <li>
              <button
                onclick={() => {
                  newTab(localSpec(profile));
                  menuOpen = false;
                }}><span class="kind-dot kind-{profile.kind}"></span>{profile.name}</button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </nav>

  {#if current}
    <div class="actions">
      <button title="Dividir à direita (Ctrl+Shift+D)" onclick={() => split('row')}><Icon name="splitRight" /></button>
      <button title="Dividir abaixo (Ctrl+Shift+E)" onclick={() => split('col')}><Icon name="splitDown" /></button>
      <button
        class="broadcast-toggle"
        class:on={current.broadcast}
        title="Broadcast: digitar em todos os painéis da aba (Ctrl+Shift+B)"
        onclick={toggleBroadcast}><Icon name="broadcast" /><span>Broadcast</span></button>
    </div>
  {/if}
  <div class="actions">
    <button class="sftp-toggle" class:on={sftp.open} title="Arquivos SFTP (Ctrl+Shift+F)" onclick={toggleSftp}>
      <Icon name="folder" /><span>SFTP</span>
    </button>
  </div>

  <div class="window-controls">
    <button title="Minimizar" onclick={() => win.minimize()}><Icon name="minimize" size={14} /></button>
    <button title="Maximizar" onclick={() => win.toggleMaximize()}><Icon name="maximize" size={13} /></button>
    <button class="close" title="Fechar" onclick={() => win.close()}><Icon name="x" size={15} /></button>
  </div>
</header>
