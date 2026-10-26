<script lang="ts">
  import PlusIcon from '@lucide/svelte/icons/plus';
  import KeyRoundIcon from '@lucide/svelte/icons/key-round';
  import LockIcon from '@lucide/svelte/icons/lock';
  import UsbIcon from '@lucide/svelte/icons/usb';
  import SparklesIcon from '@lucide/svelte/icons/sparkles';
  import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
  import SearchIcon from '@lucide/svelte/icons/search';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { Badge } from '$lib/components/ui/badge/index.js';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu/index.js';
  import { app, openDialog } from '../lib/state.svelte';
  import { confirmDeleteCredential, copyText } from '../lib/actions.svelte';
  import type { CredentialView } from '../lib/backend';

  let query = $state('');
  const visible = $derived(
    app.credentials
      .filter((c) => !query.trim() || [c.label, c.username].some((v) => v.toLowerCase().includes(query.trim().toLowerCase())))
      .sort((a, b) => a.label.localeCompare(b.label)),
  );

  const edit = (credential: CredentialView | null) => void openDialog({ type: 'credentialForm', credential });

  function kind(c: CredentialView) {
    if (c.auth === 'agent') return { icon: UsbIcon, text: 'Agente SSH' };
    if (c.auth === 'key') return { icon: KeyRoundIcon, text: c.hasKey ? 'Chave no cofre' : c.keyPath ? 'Arquivo de chave' : 'Chaves padrão (~/.ssh)' };
    return { icon: LockIcon, text: c.hasPassword ? 'Senha salva' : 'Senha pedida ao conectar' };
  }
</script>

<div class="flex h-full flex-col">
  <header class="flex flex-wrap items-center gap-2 border-b px-6 py-4">
    <div class="mr-auto">
      <h1 class="text-lg font-semibold">Credenciais</h1>
      <p class="text-xs text-muted-foreground">Usuário e senha ou chave, reaproveitados por vários hosts (SSH e RDP).</p>
    </div>
    <div class="relative w-56">
      <SearchIcon class="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
      <Input bind:value={query} placeholder="Buscar" class="pl-8" spellcheck={false} />
    </div>
    <Button variant="outline" onclick={() => void openDialog({ type: 'generateKey' })}><SparklesIcon /> Gerar chave SSH</Button>
    <Button onclick={() => edit(null)}><PlusIcon /> Nova credencial</Button>
  </header>

  <div class="min-h-0 flex-1 overflow-y-auto px-6 py-5">
    {#if app.credentials.length === 0}
      <div class="mx-auto mt-20 flex max-w-sm flex-col items-center gap-3 text-center">
        <div class="flex size-14 items-center justify-center rounded-2xl bg-muted"><KeyRoundIcon class="size-6 text-muted-foreground" /></div>
        <h2 class="text-base font-semibold">Nenhuma credencial</h2>
        <p class="text-muted-foreground">
          Salve um usuário com senha ou chave uma vez e escolha nos hosts. Trocou a senha? Muda num lugar só.
        </p>
        <div class="flex gap-2">
          <Button variant="outline" onclick={() => void openDialog({ type: 'generateKey' })}><SparklesIcon /> Gerar chave</Button>
          <Button onclick={() => edit(null)}><PlusIcon /> Nova credencial</Button>
        </div>
      </div>
    {:else}
      <div class="grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-3">
        {#each visible as c (c.id)}
          {@const k = kind(c)}
          <div
            role="button"
            tabindex="0"
            class="group flex cursor-pointer items-start gap-3 rounded-xl border bg-card p-3 transition-colors hover:border-primary/40 hover:bg-accent"
            onclick={() => edit(c)}
            onkeydown={(e) => e.key === 'Enter' && edit(c)}>
            <span class="flex size-10 shrink-0 items-center justify-center rounded-lg bg-primary/12 text-primary"><k.icon class="size-4" /></span>
            <div class="min-w-0 flex-1">
              <div class="truncate font-medium">{c.label}</div>
              <div class="truncate text-xs text-muted-foreground">{c.username || 'sem usuário'}</div>
              <div class="mt-1.5 flex flex-wrap gap-1">
                <Badge variant="secondary" class="text-[10px]">{k.text}</Badge>
                {#if c.usedBy.length}
                  <Badge variant="outline" class="text-[10px]" title={c.usedBy.join(', ')}>{c.usedBy.length === 1 ? '1 host' : `${c.usedBy.length} hosts`}</Badge>
                {/if}
              </div>
            </div>
            <DropdownMenu.Root>
              <DropdownMenu.Trigger>
                {#snippet child({ props })}
                  <Button {...props} variant="ghost" size="icon-sm" class="opacity-0 group-hover:opacity-100 aria-expanded:opacity-100" onclick={(e: MouseEvent) => e.stopPropagation()}>
                    <EllipsisIcon />
                  </Button>
                {/snippet}
              </DropdownMenu.Trigger>
              <DropdownMenu.Content align="end">
                <DropdownMenu.Item onSelect={() => edit(c)}>Editar</DropdownMenu.Item>
                {#if c.publicKey}
                  <DropdownMenu.Item onSelect={() => copyText(c.publicKey!, 'Chave pública copiada')}>Copiar chave pública</DropdownMenu.Item>
                  <DropdownMenu.Item onSelect={() => void openDialog({ type: 'publicKey', label: c.label, publicKey: c.publicKey! })}>Ver chave pública</DropdownMenu.Item>
                {/if}
                <DropdownMenu.Separator />
                <DropdownMenu.Item variant="destructive" onSelect={() => confirmDeleteCredential(c)}>Excluir</DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Root>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
