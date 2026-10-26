<script lang="ts">
  import { untrack } from 'svelte';
  import { toast } from 'svelte-sonner';
  import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
  import * as Dialog from '$lib/components/ui/dialog/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { Label } from '$lib/components/ui/label/index.js';
  import { Switch } from '$lib/components/ui/switch/index.js';
  import Segmented from './Segmented.svelte';
  import { app, closeDialog, deleteTunnel, saveTunnel } from '../lib/state.svelte';
  import { errorText } from '../lib/actions.svelte';
  import { selectClass } from '../lib/ui';
  import type { Tunnel, TunnelKind } from '../lib/backend';

  let { tunnel, hostId }: { tunnel: Tunnel | null; hostId?: string } = $props();

  const sshHosts = $derived(app.hosts.filter((h) => h.protocol !== 'rdp').sort((a, b) => a.label.localeCompare(b.label)));
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

  const kinds: { value: TunnelKind; label: string; hint: string }[] = [
    { value: 'local', label: 'Local (-L)', hint: 'Uma porta desta máquina leva a um destino acessível pelo servidor (ex.: banco de dados da VM).' },
    { value: 'remote', label: 'Remoto (-R)', hint: 'Uma porta no servidor leva a um destino acessível por esta máquina (ex.: expor um serviço local).' },
    { value: 'dynamic', label: 'SOCKS (-D)', hint: 'Proxy SOCKS5 nesta máquina: navegador ou apps saem pela rede do servidor.' },
  ];
  const hint = $derived(kinds.find((k) => k.value === form.kind)?.hint);

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
    if (!form.hostId) return void (error = 'Cadastre um host SSH antes de criar túneis.');
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
      toast.success(start ? 'Túnel salvo e iniciando' : 'Túnel salvo');
    } catch (err) {
      error = errorText(err);
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (!initial) return;
    await deleteTunnel(initial.id);
    closeDialog(null);
    toast.success('Túnel excluído');
  }
</script>

<Dialog.Root open onOpenChange={(open) => !open && closeDialog(null)}>
  <Dialog.Content class="sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title>{initial ? 'Editar túnel' : 'Novo túnel'}</Dialog.Title>
      <Dialog.Description>{hint}</Dialog.Description>
    </Dialog.Header>

    <form
      id="tunnel-form"
      class="grid gap-4"
      onsubmit={(e) => {
        e.preventDefault();
        void submit(true);
      }}>
      <div class="grid gap-1.5">
        <Label for="t-host">Host SSH</Label>
        <select id="t-host" class={selectClass} bind:value={form.hostId}>
          {#each sshHosts as host (host.id)}
            <option value={host.id}>{host.label} — {host.hostname}</option>
          {/each}
        </select>
      </div>

      <Segmented value={form.kind} options={kinds} onchange={(v) => (form.kind = v as TunnelKind)} />

      <div class="flex gap-2">
        <div class="grid flex-1 gap-1.5">
          <Label for="t-bind">{form.kind === 'remote' ? 'Escutar no servidor' : 'Escutar nesta máquina'}</Label>
          <Input id="t-bind" bind:value={form.bindHost} onblur={() => splitAddress('bind')} spellcheck={false} />
        </div>
        <div class="grid w-24 gap-1.5">
          <Label for="t-bport">Porta</Label>
          <!-- svelte-ignore a11y_autofocus -->
          <Input id="t-bport" type="number" min="1" max="65535" bind:value={form.bindPort} placeholder="8080" autofocus />
        </div>
      </div>

      {#if form.kind !== 'dynamic'}
        <ArrowDownIcon class="mx-auto -my-2 size-4 text-muted-foreground" />
        <div class="flex gap-2">
          <div class="grid flex-1 gap-1.5">
            <Label for="t-dest">{form.kind === 'local' ? 'Destino (visto pelo servidor)' : 'Destino (visto desta máquina)'}</Label>
            <Input id="t-dest" bind:value={form.destHost} onblur={() => splitAddress('dest')} placeholder="localhost" spellcheck={false} />
          </div>
          <div class="grid w-24 gap-1.5">
            <Label for="t-dport">Porta</Label>
            <Input id="t-dport" type="number" min="1" max="65535" bind:value={form.destPort} placeholder="5432" />
          </div>
        </div>
      {/if}

      <div class="grid gap-1.5">
        <Label for="t-label">Nome</Label>
        <Input id="t-label" bind:value={form.label} placeholder="opcional (ex.: Postgres produção)" />
      </div>
      <label class="flex items-center justify-between gap-4">
        <span class="text-sm">Iniciar automaticamente ao abrir o Nexo</span>
        <Switch checked={form.autoStart} onCheckedChange={(v) => (form.autoStart = v)} />
      </label>

      {#if error}<p class="rounded-md bg-destructive/10 px-3 py-2 text-xs text-destructive">{error}</p>{/if}
    </form>

    <Dialog.Footer class="sm:justify-between">
      <div>{#if initial}<Button variant="destructive" onclick={remove}>Excluir</Button>{/if}</div>
      <div class="flex gap-2">
        <Button variant="outline" disabled={busy} onclick={() => submit(false)}>Salvar</Button>
        <Button type="submit" form="tunnel-form" disabled={busy}>Salvar e iniciar</Button>
      </div>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
