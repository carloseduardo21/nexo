<script lang="ts">
  import { untrack } from 'svelte';
  import Modal from './Modal.svelte';
  import { app, closeDialog, deleteTunnel, saveTunnel } from '../lib/state.svelte';
  import type { Tunnel, TunnelKind } from '../lib/backend';

  let { tunnel, hostId }: { tunnel: Tunnel | null; hostId?: string } = $props();

  const initial = untrack(() => $state.snapshot(tunnel));
  let form = $state({
    label: initial?.label ?? '',
    hostId: initial?.hostId ?? untrack(() => hostId) ?? app.hosts.find((h) => h.protocol !== 'rdp')?.id ?? '',
    kind: initial?.kind ?? ('local' as TunnelKind),
    bindHost: initial?.bindHost ?? '127.0.0.1',
    bindPort: initial?.bindPort ?? ('' as number | ''),
    destHost: initial?.destHost ?? 'localhost',
    destPort: initial?.destPort ?? ('' as number | ''),
    autoStart: initial?.autoStart ?? false,
  });
  let error = $state('');
  let busy = $state(false);
  let confirmDelete = $state(false);

  const kinds: { kind: TunnelKind; label: string; hint: string }[] = [
    { kind: 'local', label: 'Local', hint: 'Uma porta desta máquina leva a um destino acessível pelo servidor (ex.: banco de dados da VM).' },
    { kind: 'remote', label: 'Remoto', hint: 'Uma porta no servidor leva a um destino acessível por esta máquina (ex.: expor um serviço local).' },
    { kind: 'dynamic', label: 'SOCKS', hint: 'Proxy SOCKS5 nesta máquina: navegador ou apps saem pela rede do servidor.' },
  ];
  const hint = $derived(kinds.find((k) => k.kind === form.kind)?.hint);

  // "host:porta" colado no campo de host se divide sozinho.
  function splitAddress(field: 'bind' | 'dest') {
    const hostKey = field === 'bind' ? 'bindHost' : 'destHost';
    const portKey = field === 'bind' ? 'bindPort' : 'destPort';
    const m = String(form[hostKey]).trim().match(/^\[?([^\]]+?)\]?:(\d+)$/);
    if (!m) return;
    form[hostKey] = m[1];
    form[portKey] = Number(m[2]);
  }

  async function submit(start: boolean) {
    splitAddress('bind');
    splitAddress('dest');
    if (!form.hostId) {
      error = 'Cadastre um host antes de criar túneis.';
      return;
    }
    busy = true;
    error = '';
    try {
      await saveTunnel(
        {
          id: initial?.id ?? '',
          ...form,
          label: form.label.trim(),
          bindHost: form.bindHost.trim() || (form.kind === 'remote' ? 'localhost' : '127.0.0.1'),
          bindPort: Number(form.bindPort) || 0,
          destHost: form.kind === 'dynamic' ? '' : form.destHost.trim(),
          destPort: form.kind === 'dynamic' ? 0 : Number(form.destPort) || 0,
        },
        start,
      );
      closeDialog(null);
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
    await deleteTunnel(initial.id);
    closeDialog(null);
  }
</script>

<Modal title={initial ? 'Editar túnel' : 'Novo túnel'} width={480}>
  <form
    class="form"
    onsubmit={(e) => {
      e.preventDefault();
      void submit(true);
    }}>
    <div class="row">
      <label class="grow">
        <span>Host SSH</span>
        <select bind:value={form.hostId}>
          {#each app.hosts.filter((h) => h.protocol !== 'rdp') as host (host.id)}
            <option value={host.id}>{host.label} — {host.username}@{host.hostname}</option>
          {/each}
        </select>
      </label>
    </div>

    <div class="segmented" role="radiogroup" aria-label="Tipo de túnel">
      {#each kinds as k (k.kind)}
        <button type="button" class:on={form.kind === k.kind} onclick={() => (form.kind = k.kind)}>{k.label}</button>
      {/each}
    </div>
    <p class="muted">{hint}</p>

    <div class="row">
      <label class="grow">
        <span>{form.kind === 'remote' ? 'Escutar no servidor' : 'Escutar nesta máquina'}</span>
        <input bind:value={form.bindHost} onblur={() => splitAddress('bind')} spellcheck="false" />
      </label>
      <label class="port">
        <span>Porta</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input type="number" min="1" max="65535" bind:value={form.bindPort} placeholder="8080" autofocus />
      </label>
    </div>

    {#if form.kind !== 'dynamic'}
      <div class="tunnel-arrow">↓</div>
      <div class="row">
        <label class="grow">
          <span>{form.kind === 'local' ? 'Destino (visto pelo servidor)' : 'Destino (visto desta máquina)'}</span>
          <input bind:value={form.destHost} onblur={() => splitAddress('dest')} placeholder="localhost" spellcheck="false" />
        </label>
        <label class="port">
          <span>Porta</span>
          <input type="number" min="1" max="65535" bind:value={form.destPort} placeholder="5432" />
        </label>
      </div>
    {/if}

    <label>
      <span>Nome</span>
      <input bind:value={form.label} placeholder="opcional (ex.: Postgres produção)" />
    </label>
    <label class="check"><input type="checkbox" bind:checked={form.autoStart} /> Iniciar automaticamente ao abrir o Nexo</label>

    {#if error}<p class="form-error">{error}</p>{/if}

    <div class="buttons">
      {#if initial}
        <button type="button" class="danger" onclick={remove}>{confirmDelete ? 'Confirmar exclusão' : 'Excluir'}</button>
      {/if}
      <span class="spacer"></span>
      <button type="button" onclick={() => closeDialog(null)}>Cancelar</button>
      <button type="button" disabled={busy} onclick={() => submit(false)}>Salvar</button>
      <button type="submit" class="primary" disabled={busy}>Salvar e iniciar</button>
    </div>
  </form>
</Modal>
