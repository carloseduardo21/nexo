<script lang="ts">
  import { open as pickFile } from '@tauri-apps/plugin-dialog';
  import Modal from './Modal.svelte';
  import { closeDialog, refreshHosts } from '../lib/state.svelte';
  import { backend, type SshConfigEntry } from '../lib/backend';

  let path = $state('');
  let entries = $state<SshConfigEntry[]>([]);
  let chosen = $state<Record<string, boolean>>({});
  let loading = $state(true);
  let busy = $state(false);
  let error = $state('');

  const count = $derived(entries.filter((e) => chosen[e.alias]).length);

  async function scan(file?: string) {
    loading = true;
    error = '';
    try {
      const result = await backend.sshConfigScan(file);
      path = result.path;
      entries = result.entries;
      // Já cadastrados começam desmarcados.
      chosen = Object.fromEntries(result.entries.map((e) => [e.alias, !e.exists]));
    } catch (err) {
      entries = [];
      error = String(err);
    } finally {
      loading = false;
    }
  }

  async function choose() {
    const file = await pickFile({ title: 'Arquivo de configuração do SSH', defaultPath: path || undefined });
    if (file) await scan(file);
  }

  function toggleAll() {
    const on = count < entries.length;
    chosen = Object.fromEntries(entries.map((e) => [e.alias, on]));
  }

  async function submit() {
    busy = true;
    error = '';
    try {
      await backend.sshConfigImport(path, entries.filter((e) => chosen[e.alias]).map((e) => e.alias));
      await refreshHosts();
      closeDialog(null);
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }

  void scan();
</script>

<Modal title="Importar do ssh config" width={560}>
  <div class="form">
    <div class="row">
      <label class="grow">
        <span>Arquivo</span>
        <input
          bind:value={path}
          spellcheck="false"
          onkeydown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault();
              void scan(path);
            }
          }} />
      </label>
      <button type="button" class="import-browse" onclick={choose}>Escolher…</button>
    </div>

    {#if loading}
      <p class="muted">Lendo…</p>
    {:else if entries.length > 0}
      <div class="import-head">
        <label class="check"><input type="checkbox" checked={count === entries.length} onchange={toggleAll} /> {count} de {entries.length} selecionados</label>
      </div>
      <ul class="import-list">
        {#each entries as entry (entry.alias)}
          <li>
            <label class="check">
              <input type="checkbox" bind:checked={chosen[entry.alias]} />
              <span class="import-text">
                <span class="import-alias">{entry.alias}{#if entry.exists}<span class="badge">já cadastrado</span>{/if}</span>
                <span class="import-sub">
                  {entry.user}@{entry.hostname}{entry.port !== 22 ? `:${entry.port}` : ''}
                  {#if entry.proxyJump}· via {entry.proxyJump}{/if}
                  {#if entry.identityFile}· {entry.identityFile}{/if}
                </span>
              </span>
            </label>
          </li>
        {/each}
      </ul>
      <p class="muted">
        Hosts sem <code>IdentityFile</code> usam as chaves padrão do ~/.ssh e o agente SSH, como o OpenSSH. Jump hosts
        usados pelos escolhidos são importados junto.
      </p>
    {:else if !error}
      <p class="muted">Nenhum host encontrado nesse arquivo.</p>
    {/if}

    {#if error}<p class="form-error">{error}</p>{/if}

    <div class="buttons">
      <span class="spacer"></span>
      <button type="button" onclick={() => closeDialog(null)}>Cancelar</button>
      <button type="button" class="primary" disabled={busy || count === 0} onclick={submit}>
        Importar {count || ''}
      </button>
    </div>
  </div>
</Modal>
