<script lang="ts">
  import Icon from './Icon.svelte';
  import { app, connectHost, localSpec, newTab, openDialog, startTunnel, stopTunnel } from '../lib/state.svelte';
  import { toggleSftp } from '../lib/sftp.svelte';
  import type { HostView, Tunnel, TunnelStatus } from '../lib/backend';

  let filter = $state('');

  const groups = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    const matches = app.hosts.filter(
      (h) => !q || [h.label, h.hostname, h.username, h.group].some((v) => v.toLowerCase().includes(q)),
    );
    const byGroup = new Map<string, HostView[]>();
    for (const host of [...matches].sort((a, b) => a.label.localeCompare(b.label))) {
      const list = byGroup.get(host.group) ?? [];
      list.push(host);
      byGroup.set(host.group, list);
    }
    // Sem grupo primeiro, depois grupos em ordem alfabética.
    return [...byGroup.entries()].sort(([a], [b]) => (a === '' ? -1 : b === '' ? 1 : a.localeCompare(b)));
  });

  const initials = (label: string) =>
    label
      .split(/[\s._-]+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((w) => w[0].toUpperCase())
      .join('');

  // Cor estável por host, para reconhecer de relance.
  const hue = (id: string) => [...id].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7);

  const editHost = (host: HostView | null) => void openDialog({ type: 'hostForm', host });
  const editTunnel = (tunnel: Tunnel | null) => void openDialog({ type: 'tunnelForm', tunnel });

  function route(t: Tunnel) {
    if (t.kind === 'dynamic') return `SOCKS ${t.bindHost}:${t.bindPort}`;
    const from = t.kind === 'remote' ? `servidor:${t.bindPort}` : `${t.bindHost === '127.0.0.1' ? 'localhost' : t.bindHost}:${t.bindPort}`;
    return `${from} → ${t.destHost}:${t.destPort}`;
  }

  function describeStatus(status: TunnelStatus | undefined) {
    switch (status?.state) {
      case undefined: return 'parado';
      case 'connecting': return 'conectando…';
      case 'running': return status.connections === 1 ? '1 conexão' : status.connections ? `${status.connections} conexões` : 'ativo';
      case 'reconnecting': return `reconectando (tentativa ${status.attempt})…`;
      case 'error': return status.message;
    }
  }
</script>

<aside class="sidebar">
  <section class="hosts-section">
    <div class="section-head">
      <h2>Hosts</h2>
      <button class="icon-button" title="Importar do ~/.ssh/config" onclick={() => void openDialog({ type: 'sshImport' })}>
        <Icon name="download" size={14} />
      </button>
      <button class="icon-button" title="Novo host" onclick={() => editHost(null)}><Icon name="plus" size={15} /></button>
    </div>

    {#if app.hosts.length > 0}
      <label class="search">
        <Icon name="search" size={13} />
        <input bind:value={filter} placeholder="Buscar hosts" spellcheck="false" />
      </label>
      <div class="host-groups">
        {#each groups as [group, hosts] (group)}
          {#if group}<h3>{group}</h3>{/if}
          <ul class="list hosts">
            {#each hosts as host (host.id)}
              <li>
                <button class="host" onclick={() => connectHost(host)} title="Conectar">
                  <span class="avatar" style:--h={hue(host.id)}>{initials(host.label)}</span>
                  <span class="host-text">
                    <span class="host-label">{host.label}</span>
                    <span class="host-sub">
                      {#if host.protocol === 'rdp'}<span class="proto">RDP</span>{/if}
                      {host.username}@{host.hostname}{host.port !== (host.protocol === 'rdp' ? 3389 : 22) ? `:${host.port}` : ''}
                    </span>
                  </span>
                </button>
                <button class="host-edit" title="Editar" onclick={() => editHost(host)}><Icon name="pencil" size={13} /></button>
              </li>
            {:else}
              <li class="no-match">Nenhum host encontrado.</li>
            {/each}
          </ul>
        {/each}
      </div>
    {:else}
      <div class="empty">
        <Icon name="server" size={22} />
        <p>Cadastre seus servidores para conectar com um clique.</p>
        <button class="ghost" onclick={() => editHost(null)}><Icon name="plus" size={14} /> Novo host</button>
      </div>
    {/if}
  </section>

  <section class="tunnels-section">
    <div class="section-head">
      <h2>Túneis</h2>
      <button class="icon-button" title="Novo túnel" disabled={!app.hosts.some((h) => h.protocol !== 'rdp')} onclick={() => editTunnel(null)}>
        <Icon name="plus" size={15} />
      </button>
    </div>
    {#if app.tunnels.length > 0}
      <ul class="list hosts tunnels">
        {#each app.tunnels as tunnel (tunnel.id)}
          {@const status = app.tunnelStatus[tunnel.id]}
          {@const host = app.hosts.find((h) => h.id === tunnel.hostId)}
          <li>
            <div class="tunnel" title={`${route(tunnel)} via ${host?.label ?? '?'}\n${describeStatus(status)}`}>
              <span class="tunnel-dot {status?.state ?? 'stopped'}"></span>
              <span class="host-text">
                <span class="host-label">{tunnel.label || route(tunnel)}</span>
                <span class="host-sub" class:error={status?.state === 'error'}>
                  {status ? describeStatus(status) : tunnel.label ? route(tunnel) : `via ${host?.label ?? '?'}`}
                </span>
              </span>
              {#if status && status.state !== 'error'}
                <button class="tunnel-toggle on" title="Parar" onclick={() => stopTunnel(tunnel.id)}><Icon name="stop" size={12} /></button>
              {:else}
                <button class="tunnel-toggle" title="Iniciar" onclick={() => startTunnel(tunnel.id)}><Icon name="play" size={12} /></button>
              {/if}
            </div>
            <button class="host-edit tunnel-edit" title="Editar" onclick={() => editTunnel(tunnel)}><Icon name="pencil" size={13} /></button>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="section-empty">Acesse portas da VM (banco, painel web) como se fossem locais.</p>
    {/if}
  </section>

  <section>
    <h2>Terminais locais</h2>
    <ul class="list">
      {#each app.profiles as profile (profile.id)}
        <li>
          <button onclick={() => newTab(localSpec(profile))} title={`${profile.program} ${profile.args.join(' ')}`}>
            <span class="kind-dot kind-{profile.kind}"></span>
            <span>{profile.name}</span>
          </button>
        </li>
      {/each}
    </ul>
  </section>

  <nav class="sidebar-footer">
    <button disabled title="Chaves SSH (em breve)"><Icon name="key" /></button>
    <button title="Arquivos SFTP (Ctrl+Shift+F)" onclick={toggleSftp}><Icon name="folder" /></button>
    <button disabled title="Configurações (em breve)"><Icon name="settings" /></button>
  </nav>
</aside>
