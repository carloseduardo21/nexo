<script lang="ts">
  import { save as saveFile } from '@tauri-apps/plugin-dialog';
  import { toast } from 'svelte-sonner';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import TrashIcon from '@lucide/svelte/icons/trash-2';
  import UploadIcon from '@lucide/svelte/icons/upload';
  import EyeIcon from '@lucide/svelte/icons/eye';
  import EyeOffIcon from '@lucide/svelte/icons/eye-off';
  import CopyIcon from '@lucide/svelte/icons/copy';
  import DownloadIcon from '@lucide/svelte/icons/download';
  import * as Dialog from '$lib/components/ui/dialog/index.js';
  import * as Select from '$lib/components/ui/select/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { Label } from '$lib/components/ui/label/index.js';
  import { backend, type SealEntry, type SealScope } from '../lib/backend';
  import { SEALED, k8s, loadList, loadYaml, seal } from '../lib/k8s.svelte';
  import { copyText, errorText } from '../lib/actions.svelte';
  import { settings } from '../lib/settings.svelte';
  import { cn } from '$lib/utils.js';

  const target = $derived(seal.dialog!);
  const editing = $derived(target.mode === 'edit');

  /** `original`: o valor lido (null = desconhecido); sem mexer, o selado atual é mantido. */
  type Row = { id: number; key: string; value: string | null; base64: boolean; original: string | null; originalKey: string };
  let nextId = 0;
  const row = (e: SealEntry, loaded: boolean): Row => ({
    id: nextId++,
    key: e.key,
    value: e.value,
    base64: e.base64,
    original: loaded ? e.value : null,
    originalKey: loaded ? e.key : '',
  });

  let name = $state('');
  let namespace = $state('');
  let scope = $state<SealScope>('strict');
  let type = $state('Opaque');
  let rows = $state<Row[]>([]);
  let loadedScope = $state<SealScope | null>(null);
  let resourceVersion: string | null = null;
  let loading = $state(false);
  let loadError = $state('');
  let valuesError = $state('');
  let status = $state('');
  let reveal = $state(false);
  let busy = $state(false);
  let error = $state('');

  $effect(() => {
    const t = target;
    error = '';
    loadError = '';
    valuesError = '';
    status = '';
    resourceVersion = null;
    loadedScope = null;
    namespace = t.namespace;
    if (t.mode === 'new') {
      name = '';
      scope = 'strict';
      type = 'Opaque';
      rows = [row({ key: '', value: '', base64: false }, false)];
      return;
    }
    name = t.name;
    loading = true;
    const load =
      t.mode === 'edit'
        ? backend.k8sSealed(settings.kubeconfig, k8s.context, t.namespace, t.name)
        : backend.k8sSecretPlain(settings.kubeconfig, k8s.context, t.namespace, t.name).then((p) => ({ ...p, scope: 'strict' as const }));
    load
      .then((s) => {
        if (seal.dialog !== t) return;
        if ('resourceVersion' in s) {
          loadedScope = s.scope;
          resourceVersion = s.resourceVersion;
          valuesError = s.valuesError;
          status = s.status;
        }
        scope = s.scope;
        type = s.type;
        rows = s.entries.map((e) => row(e, t.mode === 'edit'));
        if (!rows.length) rows = [row({ key: '', value: '', base64: false }, false)];
      })
      .catch((err) => seal.dialog === t && (loadError = errorText(err)))
      .finally(() => seal.dialog === t && (loading = false));
  });

  const SCOPES: { value: SealScope; label: string; hint: string }[] = [
    { value: 'strict', label: 'Este nome e namespace', hint: 'O padrão do kubeseal: só abre com este nome, neste namespace.' },
    { value: 'namespaceWide', label: 'Qualquer nome no namespace', hint: 'Dá para renomear o SealedSecret dentro do mesmo namespace.' },
    { value: 'clusterWide', label: 'Qualquer lugar do cluster', hint: 'Abre com qualquer nome, em qualquer namespace.' },
  ];
  const TYPES = ['Opaque', 'kubernetes.io/dockerconfigjson', 'kubernetes.io/tls', 'kubernetes.io/basic-auth', 'kubernetes.io/ssh-auth', 'kubernetes.io/service-account-token'];
  /** Chaves que cada tipo pede (sugeridas quando a lista está vazia). */
  const TYPE_KEYS: Record<string, string[]> = {
    'kubernetes.io/dockerconfigjson': ['.dockerconfigjson'],
    'kubernetes.io/tls': ['tls.crt', 'tls.key'],
    'kubernetes.io/basic-auth': ['username', 'password'],
    'kubernetes.io/ssh-auth': ['ssh-privatekey'],
  };

  function pickType(value: string) {
    type = value;
    const keys = TYPE_KEYS[value];
    if (keys && rows.every((r) => !r.key && !r.value)) rows = keys.map((key) => row({ key, value: '', base64: false }, false));
  }

  const NAME = /^[a-z0-9]([-a-z0-9.]*[a-z0-9])?$/;
  const KEY = /^[-._a-zA-Z0-9]+$/;
  const keyError = (r: Row) =>
    !r.key ? 'sem chave' : !KEY.test(r.key) ? 'use letras, números, -, _ e .' : rows.some((o) => o !== r && o.key === r.key) ? 'repetida' : '';
  const invalid = $derived(!NAME.test(name) || !NAME.test(namespace) || !rows.length || rows.some((r) => keyError(r)));

  /** Sem mexer (e no mesmo escopo), vai null: o valor selado atual fica como está (sem diff no git). */
  function entries(): SealEntry[] {
    const sameScope = loadedScope === null || loadedScope === scope;
    return rows.map((r) => {
      const unchanged = r.value === null || (r.original !== null && r.value === r.original && r.key === r.originalKey);
      return { key: r.key, value: unchanged && sameScope && editing ? null : r.value, base64: r.base64 };
    });
  }

  async function run(apply: boolean, file: string | null = null) {
    if (invalid || busy) return null;
    busy = true;
    error = '';
    const adopt = target.mode === 'convert' && target.name === name && target.namespace === namespace;
    try {
      const request = { namespace, name, scope, type, entries: entries(), resourceVersion: editing ? resourceVersion : null, apply, adopt };
      return await backend.k8sSeal(settings.kubeconfig, k8s.context, request, file);
    } catch (err) {
      error = errorText(err);
      return null;
    } finally {
      busy = false;
    }
  }

  async function apply(e: SubmitEvent) {
    e.preventDefault();
    if ((await run(true)) === null) return;
    toast.success(`${name} ${editing ? 'salvo' : 'criado'}`, { description: 'O controller cria o Secret em instantes.' });
    seal.dialog = null;
    void loadList();
    if (k8s.detail?.kind === SEALED && k8s.detail.name === name && k8s.detail.namespace === namespace) void loadYaml();
  }

  async function copy() {
    const yaml = await run(false);
    if (yaml !== null) void copyText(yaml, 'YAML do SealedSecret copiado');
  }

  async function download() {
    const path = await saveFile({ title: 'Salvar SealedSecret', defaultPath: `${name || 'sealed-secret'}.yaml`, filters: [{ name: 'YAML', extensions: ['yaml', 'yml'] }] });
    if (!path) return;
    if ((await run(false, path)) !== null) toast.success('YAML salvo', { description: path });
  }

  /** Conteúdo de um arquivo: texto se for UTF-8, senão base64. */
  async function loadFile(r: Row, e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = '';
    if (!file) return;
    if (file.size > 1024 * 1024) return void toast.error('Arquivo grande demais (Secrets vão até 1 MiB)');
    const bytes = new Uint8Array(await file.arrayBuffer());
    try {
      r.value = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
      r.base64 = false;
    } catch {
      r.value = btoa(Array.from(bytes, (b) => String.fromCharCode(b)).join(''));
      r.base64 = true;
    }
    if (!r.key) r.key = file.name.replace(/[^-._a-zA-Z0-9]/g, '_');
  }

  const title = $derived(target.mode === 'new' ? 'Novo Sealed Secret' : target.mode === 'edit' ? `Editar ${target.name}` : `Selar ${target.name}`);
</script>

<Dialog.Root open onOpenChange={(open) => !open && (seal.dialog = null)}>
  <Dialog.Content class="flex max-h-[90vh] flex-col sm:max-w-2xl">
    <Dialog.Header>
      <Dialog.Title>{title}</Dialog.Title>
      <Dialog.Description>
        Selado com o certificado do controller do contexto <span class="font-medium text-foreground">{k8s.context}</span>: só ele consegue abrir.
        {#if target.mode === 'convert'}Aplicando com o mesmo nome, o Secret atual passa a ser gerenciado pelo controller.{/if}
      </Dialog.Description>
    </Dialog.Header>

    {#if loading}
      <p class="py-6 text-center text-sm text-muted-foreground">Carregando…</p>
    {:else if loadError}
      <p class="rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive">{loadError}</p>
    {:else}
      <form id="seal-form" class="-mx-1 grid min-h-0 gap-4 overflow-y-auto px-1" onsubmit={apply}>
        {#if status}
          <p class="rounded-md bg-destructive/10 px-3 py-2 text-xs text-destructive">O controller não conseguiu abrir este SealedSecret: {status}</p>
        {/if}
        {#if valuesError}
          <p class="rounded-md bg-amber-500/10 px-3 py-2 text-xs text-amber-700 dark:text-amber-400">
            Valores atuais indisponíveis ({valuesError}). As chaves em branco continuam como estão; digite para substituir.
          </p>
        {/if}

        <div class="grid grid-cols-2 gap-3">
          <div class="grid gap-1.5">
            <Label for="s-name">Nome</Label>
            <Input id="s-name" bind:value={name} disabled={editing} spellcheck={false} placeholder="minha-secret" aria-invalid={!!name && !NAME.test(name)} />
          </div>
          <div class="grid gap-1.5">
            <Label for="s-ns">Namespace</Label>
            <Input id="s-ns" bind:value={namespace} disabled={editing} spellcheck={false} list="s-namespaces" aria-invalid={!NAME.test(namespace)} />
            <datalist id="s-namespaces">
              {#each k8s.namespaces as n (n)}<option value={n}></option>{/each}
            </datalist>
          </div>
          <div class="grid gap-1.5">
            <Label>Escopo</Label>
            <Select.Root type="single" value={scope} onValueChange={(v) => (scope = v as SealScope)}>
              <Select.Trigger class="w-full">{SCOPES.find((s) => s.value === scope)?.label}</Select.Trigger>
              <Select.Content>
                {#each SCOPES as s (s.value)}
                  <Select.Item value={s.value} label={s.label}>
                    <div>
                      <div>{s.label}</div>
                      <div class="text-xs text-muted-foreground">{s.hint}</div>
                    </div>
                  </Select.Item>
                {/each}
              </Select.Content>
            </Select.Root>
          </div>
          <div class="grid gap-1.5">
            <Label>Tipo</Label>
            <Select.Root type="single" value={type} onValueChange={pickType}>
              <Select.Trigger class="w-full"><span class="truncate">{type}</span></Select.Trigger>
              <Select.Content>
                {#each TYPES.includes(type) ? TYPES : [...TYPES, type] as t (t)}
                  <Select.Item value={t} label={t}>{t}</Select.Item>
                {/each}
              </Select.Content>
            </Select.Root>
          </div>
        </div>
        {#if editing && loadedScope && loadedScope !== scope}
          <p class="-mt-2 text-xs text-muted-foreground">Mudar o escopo sela todos os valores de novo: todos precisam estar preenchidos.</p>
        {/if}

        <div class="grid gap-2">
          <div class="flex items-center gap-2">
            <Label class="flex-1">Valores</Label>
            <Button variant="ghost" size="xs" type="button" onclick={() => (reveal = !reveal)}>
              {#if reveal}<EyeOffIcon />Esconder{:else}<EyeIcon />Mostrar{/if}
            </Button>
          </div>
          {#each rows as r (r.id)}
            {@const problem = r.key || rows.length > 1 ? keyError(r) : ''}
            <div class="flex items-start gap-2">
              <div class="grid w-44 shrink-0 gap-0.5">
                <Input
                  bind:value={r.key}
                  placeholder="chave"
                  spellcheck={false}
                  class="font-mono text-xs"
                  disabled={editing && r.original === null && r.value === null}
                  title={editing && r.original === null && r.value === null ? 'Digite um valor para renomear' : undefined}
                  aria-invalid={!!problem} />
                {#if problem}<span class="text-[11px] text-destructive">{problem}</span>{/if}
              </div>
              <textarea
                class={cn(
                  'min-h-9 flex-1 resize-y rounded-md border border-input bg-transparent px-2.5 py-2 font-mono text-xs shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 dark:bg-input/30',
                  !reveal && '[-webkit-text-security:disc]',
                )}
                rows={Math.min(6, Math.max(1, (r.value ?? '').split('\n').length))}
                spellcheck="false"
                placeholder={r.value === null ? 'valor selado atual (mantido)' : 'valor'}
                value={r.value ?? ''}
                oninput={(e) => (r.value = e.currentTarget.value)}></textarea>
              <div class="flex shrink-0 items-center gap-0.5 pt-1">
                <button
                  type="button"
                  class={cn('rounded px-1 text-[10px] hover:bg-accent', r.base64 ? 'bg-muted font-medium text-foreground' : 'text-muted-foreground/60')}
                  title={r.base64 ? 'O valor está em base64 (conteúdo binário): clique para tratar como texto' : 'Tratar o valor como base64 (conteúdo binário)'}
                  onclick={() => (r.base64 = !r.base64)}>
                  b64
                </button>
                <label class="inline-flex size-7 cursor-pointer items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground" title="Carregar de um arquivo">
                  <UploadIcon class="size-3.5" />
                  <input type="file" class="hidden" onchange={(e) => void loadFile(r, e)} />
                </label>
                <Button variant="ghost" size="icon-xs" type="button" title="Remover" disabled={rows.length === 1} onclick={() => (rows = rows.filter((o) => o !== r))}>
                  <TrashIcon />
                </Button>
              </div>
            </div>
          {/each}
          <Button variant="outline" size="sm" type="button" class="justify-self-start" onclick={() => rows.push(row({ key: '', value: '', base64: false }, false))}>
            <PlusIcon />Adicionar valor
          </Button>
        </div>

        {#if error}
          <p class="rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</p>
        {/if}
      </form>
    {/if}

    <Dialog.Footer class="gap-2 sm:justify-between">
      <div class="flex gap-2">
        <Button variant="ghost" size="sm" disabled={invalid || busy || loading || !!loadError} title="Copiar o SealedSecret em YAML (para o git)" onclick={copy}>
          <CopyIcon />Copiar YAML
        </Button>
        <Button variant="ghost" size="sm" disabled={invalid || busy || loading || !!loadError} title="Salvar o SealedSecret num arquivo .yaml" onclick={download}>
          <DownloadIcon />Salvar YAML…
        </Button>
      </div>
      <div class="flex gap-2">
        <Button variant="outline" onclick={() => (seal.dialog = null)}>Cancelar</Button>
        <Button type="submit" form="seal-form" disabled={invalid || busy || loading || !!loadError}>
          {busy ? 'Selando…' : editing ? 'Salvar no cluster' : 'Criar no cluster'}
        </Button>
      </div>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
