<script lang="ts">
  import { app, localSpec, newTab } from '../lib/state.svelte';

  const shortcuts = [
    ['Ctrl+Shift+T', 'Nova aba'],
    ['Ctrl+Shift+D', 'Dividir à direita'],
    ['Ctrl+Shift+E', 'Dividir abaixo'],
    ['Ctrl+Shift+B', 'Broadcast'],
    ['Alt + setas', 'Navegar entre painéis'],
    ['Ctrl+Shift+W', 'Fechar painel'],
  ];
</script>

<div class="welcome">
  <img src="/logo.svg" alt="" width="64" height="64" />
  <h1>Nexo</h1>
  <p>Abra um terminal para começar.</p>
  <div class="profiles">
    {#each app.profiles as profile (profile.id)}
      <button onclick={() => newTab(localSpec(profile))}><span class="kind-dot kind-{profile.kind}"></span>{profile.name}</button>
    {/each}
  </div>
  <dl class="shortcuts">
    {#each shortcuts as [keys, label]}
      <dt><kbd>{keys}</kbd></dt>
      <dd>{label}</dd>
    {/each}
  </dl>
</div>
