<script lang="ts">
  import { untrack } from 'svelte';
  import Modal from './Modal.svelte';
  import { app, closeDialog, connectHost, deleteHost, saveHost } from '../lib/state.svelte';
  import type { HostView, Protocol } from '../lib/backend';

  const DEFAULT_PORT: Record<Protocol, number> = { ssh: 22, rdp: 3389 };
  const RESOLUTIONS = ['1280x720', '1366x768', '1600x900', '1920x1080', '2560x1440'];

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
    protocol: initial?.protocol ?? ('ssh' as Protocol),
  });
  let rdp = $state({ domain: '', width: 0, height: 0, hidpi: false, clipboard: true, captureKeys: true, gfx: true, ...initial?.rdp });
  let resolution = $state(rdp.width && rdp.height ? `${rdp.width}x${rdp.height}` : 'fit');

  function setProtocol(protocol: Protocol) {
    // Porta padrão acompanha o protocolo (se o usuário não escolheu outra).
    if (Number(form.port) === DEFAULT_PORT[form.protocol]) form.port = DEFAULT_PORT[protocol];
    form.protocol = protocol;
    if (protocol === 'rdp') form.auth = 'password';
  }

  // Jump hosts possíveis: qualquer outro host que não dependa (direta ou indiretamente) deste.
  const jumpChoices = $derived.by(() => {
    const self = initial?.id;
    const reaches = (id: string): boolean => {
      for (let hop = app.hosts.find((h) => h.id === id), n = 0; hop && n < 10; hop = app.hosts.find((h) => h.id === hop!.jumpHostId), n++) {
        if (hop.id === self) return true;
      }
      return false;
    };
    return app.hosts
      .filter((h) => h.protocol !== 'rdp' && (!self || !reaches(h.id)))
      .sort((a, b) => a.label.localeCompare(b.label));
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
      const [width, height] = resolution === 'fit' ? [0, 0] : resolution.split('x').map(Number);
      const saved = await saveHost(
        {
          id: initial?.id ?? '',
          ...form,
          label: form.label.trim() || form.hostname.trim(),
          hostname: form.hostname.trim(),
          username: form.username.trim(),
          port: Number(form.port) || DEFAULT_PORT[form.protocol],
          group: form.group.trim(),
          rdp: { ...rdp, domain: rdp.domain.trim(), width, height },
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
    <div class="segmented" role="radiogroup" aria-label="Protocolo">
      <button type="button" class:on={form.protocol === 'ssh'} onclick={() => setProtocol('ssh')}>SSH</button>
      <button type="button" class:on={form.protocol === 'rdp'} onclick={() => setProtocol('rdp')}>RDP (área de trabalho)</button>
    </div>

    <div class="row">
      <label class="grow">
        <span>Endereço</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input
          bind:value={form.hostname}
          onblur={parseAddress}
          placeholder={`192.168.0.10 ou user@host:${DEFAULT_PORT[form.protocol]}`}
          autofocus
          spellcheck="false" />
      </label>
      <label class="port">
        <span>Porta</span>
        <input type="number" min="1" max="65535" bind:value={form.port} />
      </label>
    </div>
    <div class="row">
      <label class="grow">
        <span>Usuário</span>
        <input bind:value={form.username} placeholder={form.protocol === 'rdp' ? 'usuário' : 'root'} spellcheck="false" autocomplete="off" />
      </label>
      {#if form.protocol === 'rdp'}
        <label class="grow">
          <span>Domínio</span>
          <input bind:value={rdp.domain} placeholder="opcional" spellcheck="false" autocomplete="off" />
        </label>
      {/if}
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

    {#if form.protocol === 'rdp'}
      <div class="row">
        <label class="grow">
          <span>Resolução</span>
          <select bind:value={resolution}>
            <option value="fit">Ajustar ao painel (muda junto com a janela)</option>
            {#each RESOLUTIONS as r (r)}<option value={r}>{r.replace('x', ' × ')}</option>{/each}
          </select>
        </label>
      </div>
      {#if resolution === 'fit'}
        <label class="check">
          <input type="checkbox" bind:checked={rdp.hidpi} /> Alta resolução (pixels físicos da tela: mais nítido, letras menores)
        </label>
      {/if}
      <label class="check">
        <input type="checkbox" bind:checked={rdp.clipboard} /> Compartilhar área de transferência (copiar e colar texto)
      </label>
      <label class="check">
        <input type="checkbox" bind:checked={rdp.captureKeys} /> Capturar todos os atalhos (Win, Alt+Tab, Alt+F4…) — Ctrl+Alt+Home libera
      </label>
      <label class="check">
        <input type="checkbox" bind:checked={rdp.gfx} /> Pipeline gráfico GFX/H.264 (mais fluido; desligue se a tela falhar)
      </label>
    {:else}
    <div class="segmented" role="radiogroup" aria-label="Autenticação">
      <button type="button" class:on={form.auth === 'password'} onclick={() => (form.auth = 'password')}>Senha</button>
      <button type="button" class:on={form.auth === 'key'} onclick={() => (form.auth = 'key')}>Chave SSH</button>
      <button type="button" class:on={form.auth === 'agent'} onclick={() => (form.auth = 'agent')}>Agente SSH</button>
    </div>
    {/if}

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
      {#if form.protocol === 'rdp'}
        <p class="muted">Com a senha salva, o login é automático (o xRDP pula a tela de login).</p>
      {/if}
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
      <span>{form.protocol === 'rdp' ? 'Gateway SSH (túnel até o servidor RDP)' : 'Jump host (ProxyJump)'}</span>
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
