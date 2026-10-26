<script lang="ts">
  import { untrack } from 'svelte';
  import { toast } from 'svelte-sonner';
  import KeyRoundIcon from '@lucide/svelte/icons/key-round';
  import * as Sheet from '$lib/components/ui/sheet/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { Label } from '$lib/components/ui/label/index.js';
  import { Switch } from '$lib/components/ui/switch/index.js';
  import { Separator } from '$lib/components/ui/separator/index.js';
  import Segmented from './Segmented.svelte';
  import { app, closeDialog, connectHost, saveHost, showView } from '../lib/state.svelte';
  import { confirmDeleteHost, errorText } from '../lib/actions.svelte';
  import { selectClass } from '../lib/ui';
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
    credentialId: initial?.credentialId ?? '',
    dockerSudo: initial?.dockerSudo ?? false,
  });
  let rdp = $state({ domain: '', width: 0, height: 0, hidpi: false, clipboard: true, captureKeys: true, gfx: true, ...initial?.rdp });
  let resolution = $state(rdp.width && rdp.height ? `${rdp.width}x${rdp.height}` : 'fit');
  let password = $state('');
  let passphrase = $state('');
  let clearPassword = $state(false);
  let clearPassphrase = $state(false);
  let error = $state('');
  let busy = $state(false);

  const credential = $derived(app.credentials.find((c) => c.id === form.credentialId));
  // Hosts RDP só usam usuário e senha: credenciais de chave/agente não servem.
  const credentialChoices = $derived(app.credentials.filter((c) => form.protocol === 'ssh' || c.auth === 'password'));
  const groups = $derived([...new Set(app.hosts.map((h) => h.group).filter(Boolean))].sort());

  function setProtocol(protocol: Protocol) {
    // Porta padrão acompanha o protocolo (se o usuário não escolheu outra).
    if (Number(form.port) === DEFAULT_PORT[form.protocol]) form.port = DEFAULT_PORT[protocol];
    form.protocol = protocol;
    if (protocol === 'rdp') {
      form.auth = 'password';
      if (credential && credential.auth !== 'password') form.credentialId = '';
    }
  }

  // Jump hosts possíveis: hosts SSH que não dependam (direta ou indiretamente) deste.
  const jumpChoices = $derived.by(() => {
    const self = initial?.id;
    const reaches = (id: string): boolean => {
      for (let hop = app.hosts.find((h) => h.id === id), n = 0; hop && n < 10; hop = app.hosts.find((h) => h.id === hop!.jumpHostId), n++) {
        if (hop.id === self) return true;
      }
      return false;
    };
    return app.hosts.filter((h) => h.protocol !== 'rdp' && (!self || !reaches(h.id))).sort((a, b) => a.label.localeCompare(b.label));
  });

  // Cola "usuario@servidor:porta" no campo endereço e ele se divide sozinho.
  function parseAddress() {
    const m = form.hostname.trim().match(/^(?:([^@\s]+)@)?([^:\s]+)(?::(\d+))?$/);
    if (!m) return;
    if (m[1] && !form.username && !form.credentialId) form.username = m[1];
    form.hostname = m[2];
    if (m[3]) form.port = Number(m[3]);
  }

  async function submit(connect: boolean) {
    parseAddress();
    if (!form.hostname.trim()) return void (error = 'Informe o endereço.');
    if (!form.credentialId && !form.username.trim()) return void (error = 'Informe o usuário ou escolha uma credencial.');
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
        // undefined = manter o que está salvo (com credencial, os segredos são os dela)
        form.credentialId ? undefined : clearPassword ? '' : password || undefined,
        form.credentialId ? undefined : clearPassphrase ? '' : passphrase || undefined,
      );
      closeDialog(saved ?? null);
      toast.success(initial ? 'Host salvo' : `${saved?.label ?? 'Host'} criado`);
      if (connect && saved) connectHost(saved);
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (!host) return;
    closeDialog(null);
    await confirmDeleteHost(host);
  }
</script>

<Sheet.Root open onOpenChange={(open) => !open && closeDialog(null)}>
  <Sheet.Content class="flex w-[460px] flex-col gap-0 p-0 sm:max-w-[460px]">
    <Sheet.Header class="border-b px-5 py-4">
      <Sheet.Title>{initial ? 'Editar host' : 'Novo host'}</Sheet.Title>
      <Sheet.Description>{form.protocol === 'rdp' ? 'Área de trabalho remota (RDP / xRDP)' : 'Terminal SSH, SFTP e túneis'}</Sheet.Description>
    </Sheet.Header>

    <form
      id="host-form"
      class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-4"
      onsubmit={(e) => {
        e.preventDefault();
        void submit(true);
      }}>
      <Segmented
        value={form.protocol}
        options={[
          { value: 'ssh', label: 'SSH' },
          { value: 'rdp', label: 'RDP' },
        ]}
        onchange={(v) => setProtocol(v as Protocol)} />

      <div class="flex gap-2">
        <div class="grid flex-1 gap-1.5">
          <Label for="h-address">Endereço</Label>
          <!-- svelte-ignore a11y_autofocus -->
          <Input
            id="h-address"
            bind:value={form.hostname}
            onblur={parseAddress}
            placeholder={`192.168.0.10 ou user@host:${DEFAULT_PORT[form.protocol]}`}
            autofocus
            spellcheck={false} />
        </div>
        <div class="grid w-24 gap-1.5">
          <Label for="h-port">Porta</Label>
          <Input id="h-port" type="number" min="1" max="65535" bind:value={form.port} />
        </div>
      </div>

      <div class="flex gap-2">
        <div class="grid flex-1 gap-1.5">
          <Label for="h-label">Nome</Label>
          <Input id="h-label" bind:value={form.label} placeholder={form.hostname || 'Servidor de produção'} />
        </div>
        <div class="grid flex-1 gap-1.5">
          <Label for="h-group">Pasta</Label>
          <Input id="h-group" bind:value={form.group} placeholder="opcional" list="host-groups" />
          <datalist id="host-groups">
            {#each groups as g (g)}<option value={g}></option>{/each}
          </datalist>
        </div>
      </div>

      <Separator />

      <div class="grid gap-1.5">
        <div class="flex items-center justify-between">
          <Label for="h-credential">Credencial</Label>
          <button
            type="button"
            class="text-xs text-primary hover:underline"
            onclick={() => {
              closeDialog(null);
              showView('credentials');
            }}>Gerenciar credenciais</button>
        </div>
        <select id="h-credential" class={selectClass} bind:value={form.credentialId}>
          <option value="">Nenhuma — informar usuário e senha aqui</option>
          {#each credentialChoices as c (c.id)}
            <option value={c.id}>{c.label}{c.username ? ` — ${c.username}` : ''}</option>
          {/each}
        </select>
      </div>

      {#if credential}
        <div class="flex items-center gap-2.5 rounded-lg border bg-muted/40 px-3 py-2.5 text-xs">
          <KeyRoundIcon class="size-4 text-primary" />
          <span>
            Usuário <strong>{credential.username || '—'}</strong>,
            {credential.auth === 'password' ? (credential.hasPassword ? 'senha salva' : 'senha pedida ao conectar') : credential.auth === 'key' ? 'chave SSH' : 'agente SSH'}.
            Vale para todos os hosts desta credencial.
          </span>
        </div>
      {:else}
        <div class="flex gap-2">
          <div class="grid flex-1 gap-1.5">
            <Label for="h-user">Usuário</Label>
            <Input id="h-user" bind:value={form.username} placeholder={form.protocol === 'rdp' ? 'usuário' : 'root'} spellcheck={false} autocomplete="off" />
          </div>
          {#if form.protocol === 'rdp'}
            <div class="grid flex-1 gap-1.5">
              <Label for="h-domain">Domínio</Label>
              <Input id="h-domain" bind:value={rdp.domain} placeholder="opcional" spellcheck={false} />
            </div>
          {/if}
        </div>

        {#if form.protocol === 'ssh'}
          <Segmented
            value={form.auth}
            options={[
              { value: 'password', label: 'Senha' },
              { value: 'key', label: 'Chave SSH' },
              { value: 'agent', label: 'Agente SSH' },
            ]}
            onchange={(v) => (form.auth = v as typeof form.auth)} />
        {/if}

        {#if form.auth === 'password'}
          <div class="grid gap-1.5">
            <Label for="h-pass">Senha</Label>
            <Input
              id="h-pass"
              type="password"
              bind:value={password}
              disabled={clearPassword}
              placeholder={initial?.hasPassword && !initial.credentialId ? '•••••••• (salva no cofre)' : 'vazio = perguntar ao conectar'}
              autocomplete="new-password" />
            {#if form.protocol === 'rdp'}<p class="text-xs text-muted-foreground">Com a senha salva, o login é automático (o xRDP pula a tela de login).</p>{/if}
            {#if initial?.hasPassword && !initial.credentialId}
              <label class="flex items-center gap-2 text-xs text-muted-foreground"><input type="checkbox" bind:checked={clearPassword} /> Remover senha salva</label>
            {/if}
          </div>
        {:else if form.auth === 'agent'}
          <p class="text-xs text-muted-foreground">
            Usa as chaves do agente do sistema: <em>OpenSSH Authentication Agent</em> ou Pageant no Windows, <code>SSH_AUTH_SOCK</code> no Linux/macOS.
          </p>
        {:else}
          <div class="grid gap-1.5">
            <Label for="h-key">Arquivo da chave privada</Label>
            <Input id="h-key" bind:value={form.keyPath} placeholder="vazio = ~/.ssh/id_ed25519, id_ecdsa, id_rsa e o agente" spellcheck={false} />
          </div>
          <div class="grid gap-1.5">
            <Label for="h-passphrase">Passphrase</Label>
            <Input
              id="h-passphrase"
              type="password"
              bind:value={passphrase}
              disabled={clearPassphrase}
              placeholder={initial?.hasPassphrase && !initial.credentialId ? '•••••••• (salva no cofre)' : 'se a chave tiver'}
              autocomplete="new-password" />
            {#if initial?.hasPassphrase && !initial.credentialId}
              <label class="flex items-center gap-2 text-xs text-muted-foreground"><input type="checkbox" bind:checked={clearPassphrase} /> Remover passphrase salva</label>
            {/if}
          </div>
        {/if}
      {/if}

      {#if credential && form.protocol === 'rdp'}
        <div class="grid gap-1.5">
          <Label for="h-domain2">Domínio</Label>
          <Input id="h-domain2" bind:value={rdp.domain} placeholder="opcional" spellcheck={false} />
        </div>
      {/if}

      <div class="grid gap-1.5">
        <Label for="h-jump">{form.protocol === 'rdp' ? 'Gateway SSH (túnel até o servidor RDP)' : 'Jump host (ProxyJump)'}</Label>
        <select id="h-jump" class={selectClass} bind:value={form.jumpHostId}>
          <option value="">Nenhum — conexão direta</option>
          {#each jumpChoices as j (j.id)}
            <option value={j.id}>{j.label} — {j.hostname}</option>
          {/each}
        </select>
      </div>

      {#if form.protocol === 'ssh'}
        <label class="flex items-start justify-between gap-4">
          <span class="grid gap-0.5">
            <span class="text-sm">Docker com sudo</span>
            <span class="text-xs text-muted-foreground">Para usuários fora do grupo docker: o painel de containers roda <code>sudo docker</code>.</span>
          </span>
          <Switch checked={form.dockerSudo} onCheckedChange={(v) => (form.dockerSudo = v)} />
        </label>
      {/if}

      {#if form.protocol === 'rdp'}
        <Separator />
        <div class="grid gap-1.5">
          <Label for="h-res">Resolução</Label>
          <select id="h-res" class={selectClass} bind:value={resolution}>
            <option value="fit">Ajustar ao painel (muda junto com a janela)</option>
            {#each RESOLUTIONS as r (r)}<option value={r}>{r.replace('x', ' × ')}</option>{/each}
          </select>
        </div>
        {#snippet toggle(label: string, hint: string, checked: boolean, set: (v: boolean) => void, disabled = false)}
          <label class="flex items-start justify-between gap-4" class:opacity-50={disabled}>
            <span class="grid gap-0.5">
              <span class="text-sm">{label}</span>
              <span class="text-xs text-muted-foreground">{hint}</span>
            </span>
            <Switch {checked} {disabled} onCheckedChange={set} />
          </label>
        {/snippet}
        {@render toggle('Alta resolução', 'Pixels físicos da tela: mais nítido, letras menores.', rdp.hidpi, (v) => (rdp.hidpi = v), resolution !== 'fit')}
        {@render toggle('Área de transferência', 'Copiar e colar texto entre esta máquina e o servidor.', rdp.clipboard, (v) => (rdp.clipboard = v))}
        {@render toggle('Capturar todos os atalhos', 'Win, Alt+Tab, Alt+F4 vão para o servidor. Ctrl+Alt+Home libera.', rdp.captureKeys, (v) => (rdp.captureKeys = v))}
        {@render toggle('Pipeline gráfico (GFX/H.264)', 'Mais fluido. Desligue se a tela falhar.', rdp.gfx, (v) => (rdp.gfx = v))}
      {/if}

      {#if error}<p class="rounded-md bg-destructive/10 px-3 py-2 text-xs text-destructive">{error}</p>{/if}
    </form>

    <Sheet.Footer class="flex-row items-center gap-2 border-t px-5 py-3">
      {#if host}<Button variant="destructive" onclick={remove}>Excluir</Button>{/if}
      <span class="flex-1"></span>
      <Button variant="outline" disabled={busy} onclick={() => submit(false)}>Salvar</Button>
      <Button type="submit" form="host-form" disabled={busy}>Salvar e conectar</Button>
    </Sheet.Footer>
  </Sheet.Content>
</Sheet.Root>
