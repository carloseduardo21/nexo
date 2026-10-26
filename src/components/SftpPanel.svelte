<script lang="ts">
  import { onMount } from 'svelte';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import Icon from './Icon.svelte';
  import { app } from '../lib/state.svelte';
  import { backend, type RemoteEntry } from '../lib/backend';
  import {
    cancelTransfer,
    cdInTerminal,
    clearTransfers,
    copyText,
    download,
    ensureBrowser,
    forgetBrowser,
    mkdir,
    navigate,
    parentOf,
    refresh,
    remove,
    rename,
    revealTransfer,
    select,
    setWidth,
    sftp,
    sftpTarget,
    upload,
    visibleEntries,
  } from '../lib/sftp.svelte';

  const target = $derived(sftpTarget());
  const meta = $derived(target ? app.meta[target] : undefined);
  const browser = $derived(target ? sftp.browsers[target] : undefined);
  const entries = $derived(browser ? visibleEntries(browser) : []);

  let panel = $state<HTMLElement>();
  let list = $state<HTMLElement>();
  let pathInput = $state('');
  let dropping = $state(false);
  let menu = $state<{ x: number; y: number; entry: RemoteEntry | null } | null>(null);

  // Abre (ou reabre, após reconectar) o SFTP do painel alvo.
  $effect(() => {
    if (target && meta?.sessionId != null) ensureBrowser(target);
  });

  // Esquece navegadores de painéis que foram fechados.
  $effect(() => {
    for (const id of Object.keys(sftp.browsers)) if (!app.meta[id]) forgetBrowser(id);
  });

  $effect(() => {
    pathInput = browser?.path ?? '';
  });

  onMount(() => {
    // Arrastar arquivos do Explorer para o painel = enviar para a pasta atual.
    const inside = (pos: { x: number; y: number }) => {
      const rect = panel?.getBoundingClientRect();
      const x = pos.x / devicePixelRatio;
      const y = pos.y / devicePixelRatio;
      return !!rect && x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
    };
    const unlisten = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === 'leave') {
        dropping = false;
      } else if (payload.type === 'enter' || payload.type === 'over') {
        dropping = !!browser && inside(payload.position);
      } else if (payload.type === 'drop') {
        const ok = dropping && inside(payload.position);
        dropping = false;
        if (ok && target && payload.paths.length > 0) void upload(target, payload.paths);
      }
    });
    return () => void unlisten.then((fn) => fn());
  });

  function startResize(e: PointerEvent) {
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    document.body.classList.add('resizing-col');
    const right = panel!.getBoundingClientRect().right;
    const move = (ev: PointerEvent) => setWidth(right - ev.clientX);
    const up = () => {
      handle.removeEventListener('pointermove', move);
      handle.removeEventListener('pointerup', up);
      document.body.classList.remove('resizing-col');
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', up);
  }

  function open(entry: RemoteEntry) {
    if (!target) return;
    if (entry.isDir) void navigate(target, entry.path);
    else void download(target, [entry.path]);
  }

  /** Itens afetados por uma ação: a seleção, se o item clicado faz parte dela; senão, só ele. */
  function targets(entry: RemoteEntry | null) {
    if (!browser) return [];
    if (!entry) return browser.selected;
    return browser.selected.includes(entry.path) ? browser.selected : [entry.path];
  }

  function showMenu(e: MouseEvent, entry: RemoteEntry | null) {
    e.preventDefault();
    if (!target || !browser) return;
    if (entry && !browser.selected.includes(entry.path)) select(target, entry, new MouseEvent('click'));
    menu = { x: Math.min(e.clientX, innerWidth - 200), y: Math.min(e.clientY, innerHeight - 260), entry };
  }

  function act(fn: () => unknown) {
    // Executa antes de fechar: os `{@const}` do menu (item, seleção) deixam de existir quando ele fecha.
    fn();
    menu = null;
  }

  function onKey(e: KeyboardEvent) {
    if (!target || !browser) return;
    const selected = entries.filter((x) => browser.selected.includes(x.path));
    if (e.key === 'Enter' && selected.length === 1) open(selected[0]);
    else if (e.key === 'Backspace') void navigate(target, parentOf(browser.path));
    else if (e.key === 'Delete' && selected.length > 0) void remove(target, selected.map((x) => x.path));
    else if (e.key === 'F2' && selected.length === 1) void rename(target, selected[0]);
    else if (e.key === 'F5') void refresh(target);
    else if (e.ctrlKey && e.code === 'KeyA') browser.selected = entries.map((x) => x.path);
    else return;
    e.preventDefault();
  }

  async function goHome() {
    if (target && browser) void navigate(target, await backend.sftpHome(browser.sessionId));
  }

  function formatSize(bytes: number) {
    if (bytes < 1024) return `${bytes} B`;
    const units = ['KB', 'MB', 'GB', 'TB'];
    let value = bytes / 1024;
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) {
      value /= 1024;
      unit++;
    }
    return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
  }

  const dateFormat = new Intl.DateTimeFormat('pt-BR', { dateStyle: 'short', timeStyle: 'short' });
  const formatDate = (secs: number | null) => (secs ? dateFormat.format(secs * 1000) : '');
  const percent = (done: number, total: number) => (total > 0 ? Math.min(100, (done / total) * 100) : 0);
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && (menu = null)} />

<aside class="sftp" style:width="{sftp.width}px" bind:this={panel}>
  <div class="sftp-resizer" role="separator" onpointerdown={startResize}></div>

  <header class="sftp-head">
    <Icon name="folder" size={14} />
    <span class="sftp-title">{meta?.spec.kind === 'ssh' ? meta.spec.host.label : 'Arquivos'}</span>
    <span class="spacer"></span>
    <button
      class="icon-button"
      class:on={sftp.showHidden}
      title={sftp.showHidden ? 'Esconder arquivos ocultos' : 'Mostrar arquivos ocultos'}
      onclick={() => (sftp.showHidden = !sftp.showHidden)}><Icon name={sftp.showHidden ? 'eye' : 'eyeOff'} size={14} /></button>
    <button class="icon-button" title="Fechar (Ctrl+Shift+F)" onclick={() => (sftp.open = false)}><Icon name="x" size={14} /></button>
  </header>

  {#if !target}
    <div class="sftp-empty">
      <Icon name="server" size={22} />
      <p>Abra uma sessão SSH para navegar nos arquivos do servidor.</p>
    </div>
  {:else if !browser}
    <div class="sftp-empty"><p>{meta?.exited ? 'Sessão encerrada.' : 'Aguardando a conexão…'}</p></div>
  {:else}
    <div class="sftp-toolbar">
      <button class="icon-button" title="Pasta acima (Backspace)" onclick={() => navigate(target, parentOf(browser.path))}>
        <Icon name="arrowUp" size={14} />
      </button>
      <button class="icon-button" title="Pasta pessoal" onclick={goHome}><Icon name="home" size={14} /></button>
      <form
        class="sftp-path"
        onsubmit={(e) => {
          e.preventDefault();
          void navigate(target, pathInput.trim() || '/');
        }}>
        <input bind:value={pathInput} spellcheck="false" autocomplete="off" title={browser.path} />
      </form>
      <button class="icon-button" class:spinning={browser.loading} title="Atualizar (F5)" onclick={() => refresh(target)}>
        <Icon name="refresh" size={14} />
      </button>
      <button class="icon-button" title="Nova pasta" onclick={() => mkdir(target)}><Icon name="folderPlus" size={14} /></button>
      <button class="icon-button" title="Enviar arquivos (ou arraste do Explorer)" onclick={() => upload(target)}>
        <Icon name="upload" size={14} />
      </button>
    </div>

    {#if browser.error}
      <div class="sftp-error" role="alert">
        <span>{browser.error}</span>
        <button class="icon-button" title="Dispensar" onclick={() => (browser.error = '')}><Icon name="x" size={12} /></button>
      </div>
    {/if}

    <div
      class="sftp-list"
      role="listbox"
      tabindex="0"
      aria-multiselectable="true"
      bind:this={list}
      onkeydown={onKey}
      oncontextmenu={(e) => showMenu(e, null)}
      onmousedown={(e) => {
        list?.focus();
        if (e.target === e.currentTarget) browser.selected = [];
      }}>
      {#each entries as entry (entry.path)}
        <div
          class="sftp-row"
          class:selected={browser.selected.includes(entry.path)}
          role="option"
          tabindex="-1"
          aria-selected={browser.selected.includes(entry.path)}
          title={`${entry.name}\n${entry.perms}${entry.isDir ? '' : ` · ${formatSize(entry.size)}`}`}
          onmousedown={(e) => e.button === 0 && select(target, entry, e)}
          ondblclick={() => open(entry)}
          oncontextmenu={(e) => {
            e.stopPropagation();
            showMenu(e, entry);
          }}>
          <span class="sftp-icon" class:dir={entry.isDir}>
            <Icon name={entry.isDir ? 'folder' : 'file'} size={15} />
            {#if entry.isLink}<span class="sftp-link"><Icon name="link" size={9} /></span>{/if}
          </span>
          <span class="sftp-name">{entry.name}</span>
          <span class="sftp-size">{entry.isDir ? '' : formatSize(entry.size)}</span>
          <span class="sftp-date">{formatDate(entry.modified)}</span>
        </div>
      {:else}
        {#if !browser.loading}
          <div class="sftp-hint">Pasta vazia. Arraste arquivos para cá para enviar.</div>
        {/if}
      {/each}
    </div>
  {/if}

  {#if sftp.transfers.length > 0}
    <section class="transfers">
      <div class="transfers-head">
        <h2>Transferências</h2>
        {#if sftp.transfers.some((t) => t.status !== 'running')}
          <button class="link-button" onclick={clearTransfers}>Limpar</button>
        {/if}
      </div>
      <ul>
        {#each sftp.transfers as t (t.id)}
          <li class="transfer {t.status}">
            <Icon name={t.kind === 'download' ? 'download' : 'upload'} size={13} />
            <div class="transfer-body">
              <div class="transfer-line">
                <span class="transfer-label" title={t.current || t.label}>{t.label}</span>
                {#if t.status === 'running'}
                  <span class="transfer-pct">{Math.floor(percent(t.done, t.total))}%</span>
                {/if}
              </div>
              {#if t.status === 'running'}
                <div class="bar"><div style:width="{percent(t.done, t.total)}%"></div></div>
                <span class="transfer-detail">{formatSize(t.done)} de {formatSize(t.total)}</span>
              {:else if t.status === 'done'}
                <span class="transfer-detail">{t.kind === 'download' ? 'Baixado' : 'Enviado'} · {formatSize(t.total)}</span>
              {:else if t.status === 'canceled'}
                <span class="transfer-detail">Cancelado</span>
              {:else}
                <span class="transfer-detail error" title={t.error}>{t.error}</span>
              {/if}
            </div>
            {#if t.status === 'running'}
              <button class="icon-button" title="Cancelar" onclick={() => cancelTransfer(t)}><Icon name="x" size={12} /></button>
            {:else if t.status === 'done' && t.kind === 'download'}
              <button class="icon-button" title="Mostrar na pasta" onclick={() => revealTransfer(t)}><Icon name="folder" size={13} /></button>
            {/if}
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  {#if dropping}
    <div class="drop-overlay">
      <Icon name="upload" size={26} />
      <span>Soltar para enviar a<br /><strong>{browser?.path}</strong></span>
    </div>
  {/if}
</aside>

{#if menu && target && browser}
  {@const m = menu}
  {@const picked = targets(m.entry)}
  <div class="backdrop" role="presentation" onmousedown={() => (menu = null)} oncontextmenu={(e) => e.preventDefault()}></div>
  <ul class="menu context-menu" style:left="{m.x}px" style:top="{m.y}px">
    {#if m.entry}
      {@const entry = m.entry}
      {#if entry.isDir && picked.length === 1}
        <li><button onclick={() => act(() => navigate(target, entry.path))}><Icon name="folder" size={14} />Abrir</button></li>
        <li>
          <button onclick={() => act(() => cdInTerminal(target, entry.path))}><Icon name="terminal" size={14} />Abrir no terminal</button>
        </li>
      {/if}
      <li><button onclick={() => act(() => download(target, picked))}><Icon name="download" size={14} />Baixar</button></li>
      <li><button onclick={() => act(() => download(target, picked, true))}><Icon name="download" size={14} />Baixar para…</button></li>
      {#if picked.length === 1}
        <li><button onclick={() => act(() => rename(target, entry))}><Icon name="pencil" size={14} />Renomear<kbd>F2</kbd></button></li>
        <li><button onclick={() => act(() => copyText(entry.path))}><Icon name="copy" size={14} />Copiar caminho</button></li>
      {/if}
      <li class="separator"></li>
      <li>
        <button class="danger" onclick={() => act(() => remove(target, picked))}>
          <Icon name="trash" size={14} />Excluir{picked.length > 1 ? ` ${picked.length} itens` : ''}<kbd>Del</kbd>
        </button>
      </li>
    {:else}
      <li><button onclick={() => act(() => upload(target))}><Icon name="upload" size={14} />Enviar arquivos…</button></li>
      <li><button onclick={() => act(() => mkdir(target))}><Icon name="folderPlus" size={14} />Nova pasta</button></li>
      <li><button onclick={() => act(() => refresh(target))}><Icon name="refresh" size={14} />Atualizar<kbd>F5</kbd></button></li>
      <li>
        <button onclick={() => act(() => cdInTerminal(target, browser.path))}><Icon name="terminal" size={14} />Abrir pasta no terminal</button>
      </li>
      <li><button onclick={() => act(() => copyText(browser.path))}><Icon name="copy" size={14} />Copiar caminho</button></li>
    {/if}
  </ul>
{/if}
