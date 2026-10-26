<script lang="ts">
  import { untrack } from 'svelte';
  import Modal from './Modal.svelte';
  import { app, closeDialog, connectHost, deleteHost, saveHost } from '../lib/state.svelte';
  import type { HostView } from '../lib/backend';

  let { host }: { host: HostView | null } = $props();

  // Cópia local editável; o host original só muda ao salvar.
  const initial = untrack(() => $state.snapshot(host));
  let form = $state({
    label: initial?.label ?? '',
    hostname: initial?.hostname ?? '',
    port: initial?.port ?? 22,
    username: initial?.username ?? '',
    auth: initial?.auth ?? 'password',
    keyPath: initial?.keyPath ?? '',
    group: initial?.group ?? '',
    jumpHostId: initial?.jumpHostId ?? '',
  });

  // Jump hosts possíveis: qualquer outro host que não dependa (direta ou indiretamente) deste.
  const jumpChoices = $derived.by(() => {
    const self = initial?.id;
    const reaches = (id: string): boolean => {
      for (let hop = app.hosts.find((h) => h.id === id), n = 0; hop && n < 10; hop = app.hosts.find((h) => h.id === hop!.jumpHostId), n++) {
        if (hop.id === self) return true;
      }
      return false;
    };
    return app.hosts.filter((h) => !self || !reaches(h.id)).sort((a, b) => a.label.localeCompare(b.label));
  });
  let password = $state('');
  let passphrase = $state('');
  let clearPassword = $state(false);
  let clearPassphrase = $state(false);
  let error = $state('');
  let busy = $state(false);
  let confirmDelete = $state(false);

  // Cola "usuario@servidor:porta" no campo endereço e ele se divide sozinho.
  function parseAddress() {
    const m = form.hostname.trim().match(/^(?:([^@\s]+)@)?([^:\s]+)(?::(\d+))?$/);
    if (!m) return;
    if (m[1] && !form.username) form.username = m[1];
    form.hostname = m[2];
    if (m[3]) form.port = Number(m[3]);
  }

  async function submit(connect: boolean) {
    parseAddress();
    if (!form.hostname.trim() || !form.username.trim()) {
      error = 'Informe endereço e usuário.';
      return;
    }
    busy = true;
    error = '';
    try {
      const saved = await saveHost(
        {
          id: initial?.id ?? '',
          ...form,
          label: form.label.trim() || form.hostname.trim(),
          hostname: form.hostname.trim(),
          username: form.username.trim(),
          port: Number(form.port) || 22,
          group: form.group.trim(),
        },
        // undefined = manter o que está salvo
        clearPassword ? '' : password || undefined,
        clearPassphrase ? '' : passphrase || undefined,
      );
      closeDialog(saved ?? null);
      if (connect && saved) connectHost(saved);
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (!initial) return;
    if (!confirmDelete) {
      confirmDelete = true;
      return;
    }
    await deleteHost(initial.id);
    closeDialog(null);
  }
</script>

<Modal title={initial ? 'Editar host' : 'Novo host'} width={460}>
  <form
    class="form"
    onsubmit={(e) => {
      e.preventDefault();
      void submit(true);
    }}>
    <div class="row">
      <label class="grow">
        <span>Endereço</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input bind:value={form.hostname} onblur={parseAddress} placeholder="192.168.0.10 ou user@host:22" autofocus spellcheck="false" />
      </label>
      <label class="port">
        <span>Porta</span>
        <input type="number" min="1" max="65535" bind:value={form.port} />
      </label>
    </div>
    <div class="row">
      <label class="grow">
        <span>Usuário</span>
        <input bind:value={form.username} placeholder="root" spellcheck="false" autocomplete="off" />
      </label>
    </div>
    <div class="row">
      <label class="grow">
        <span>Nome</span>
        <input bind:value={form.label} placeholder={form.hostname || 'Servidor de produção'} />
      </label>
      <label class="grow">
        <span>Grupo</span>
        <input bind:value={form.group} placeholder="opcional" />
      </label>
    </div>

    <div class="segmented" role="radiogroup" aria-label="Autenticação">
      <button type="button" class:on={form.auth === 'password'} onclick={() => (form.auth = 'password')}>Senha</button>
      <button type="button" class:on={form.auth === 'key'} onclick={() => (form.auth = 'key')}>Chave SSH</button>
      <button type="button" class:on={form.auth === 'agent'} onclick={() => (form.auth = 'agent')}>Agente SSH</button>
    </div>

    {#if form.auth === 'password'}
      <label>
        <span>Senha</span>
        <input
          type="password"
          bind:value={password}
          disabled={clearPassword}
          placeholder={initial?.hasPassword ? '•••••••• (salva no cofre)' : 'deixe vazio para perguntar ao conectar'}
          autocomplete="new-password" />
      </label>
      {#if initial?.hasPassword}
        <label class="check"><input type="checkbox" bind:checked={clearPassword} /> Remover senha salva</label>
      {/if}
    {:else if form.auth === 'agent'}
      <p class="muted">
        Usa as chaves carregadas no agente do sistema: serviço <em>OpenSSH Authentication Agent</em> ou Pageant no
        Windows, <code>SSH_AUTH_SOCK</code> no Linux/macOS. Nenhum segredo fica salvo no Nexo.
      </p>
    {:else}
      <label>
        <span>Arquivo da chave privada</span>
        <input bind:value={form.keyPath} placeholder="vazio = ~/.ssh/id_ed25519, id_ecdsa, id_rsa e o agente SSH" spellcheck="false" />
      </label>
      <label>
        <span>Passphrase</span>
        <input
          type="password"
          bind:value={passphrase}
          disabled={clearPassphrase}
          placeholder={initial?.hasPassphrase ? '•••••••• (salva no cofre)' : 'se a chave tiver'}
          autocomplete="new-password" />
      </label>
      {#if initial?.hasPassphrase}
        <label class="check"><input type="checkbox" bind:checked={clearPassphrase} /> Remover passphrase salva</label>
      {/if}
    {/if}

    <label>
      <span>Jump host (ProxyJump)</span>
      <select bind:value={form.jumpHostId}>
        <option value="">Nenhum — conexão direta</option>
        {#each jumpChoices as host (host.id)}
          <option value={host.id}>{host.label} — {host.username}@{host.hostname}</option>
        {/each}
      </select>
    </label>

    {#if error}<p class="form-error">{error}</p>{/if}

    <div class="buttons">
      {#if initial}
        <button type="button" class="danger" onclick={remove}>{confirmDelete ? 'Confirmar exclusão' : 'Excluir'}</button>
      {/if}
      <span class="spacer"></span>
      <button type="button" onclick={() => closeDialog(null)}>Cancelar</button>
      <button type="button" disabled={busy} onclick={() => submit(false)}>Salvar</button>
      <button type="submit" class="primary" disabled={busy}>Salvar e conectar</button>
    </div>
  </form>
</Modal>
