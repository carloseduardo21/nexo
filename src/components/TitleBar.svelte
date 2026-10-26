<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import XIcon from '@lucide/svelte/icons/x';
  import PlusIcon from '@lucide/svelte/icons/plus';
  import MinusIcon from '@lucide/svelte/icons/minus';
  import SquareIcon from '@lucide/svelte/icons/square';
  import LayoutGridIcon from '@lucide/svelte/icons/layout-grid';
  import TerminalIcon from '@lucide/svelte/icons/terminal';
  import MonitorIcon from '@lucide/svelte/icons/monitor';
  import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
  import ColumnsIcon from '@lucide/svelte/icons/columns-2';
  import RowsIcon from '@lucide/svelte/icons/rows-2';
  import RadioTowerIcon from '@lucide/svelte/icons/radio-tower';
  import SearchIcon from '@lucide/svelte/icons/search';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu/index.js';
  import { app, activeTab, closeTab, localSpec, newTab, openDialog, showTab, showView, split, toggleBroadcast } from '../lib/state.svelte';
  import { leaves } from '../lib/layout';
  import { cn } from '$lib/utils.js';

  const win = getCurrentWindow();
  const current = $derived(app.view === 'session' ? activeTab() : undefined);
  const managing = $derived(app.view !== 'session');
</script>

<header class="flex h-10 shrink-0 items-stretch border-b bg-sidebar" data-tauri-drag-region>
  <div class="flex w-14 shrink-0 items-center justify-center" data-tauri-drag-region>
    <img src="/logo.svg" alt="Nexo" width="18" height="18" data-tauri-drag-region />
  </div>

  <nav class="flex min-w-0 flex-1 items-end gap-0.5 overflow-x-auto pt-1.5 pl-1" data-tauri-drag-region>
    <!-- Aba fixa: páginas de gerenciamento (hosts, credenciais…). -->
    <button
      class={cn(
        'flex h-full shrink-0 items-center gap-1.5 rounded-t-lg px-3 text-[13px] text-muted-foreground hover:text-foreground',
        managing && 'bg-background text-foreground',
      )}
      onclick={() => showView(managing ? app.view : 'hosts')}>
      <LayoutGridIcon class="size-3.5" /> Hosts
    </button>

    {#each app.tabs as tab (tab.id)}
      {@const count = leaves(tab.root).length}
      {@const meta = app.meta[tab.focused]}
      {@const active = tab.id === app.activeTab && !managing}
      <div
        class={cn(
          'group relative flex h-full max-w-56 min-w-0 shrink-0 cursor-default items-center gap-1.5 rounded-t-lg pr-1 pl-3 text-[13px] text-muted-foreground hover:text-foreground',
          active && 'bg-background text-foreground',
          tab.broadcast && 'shadow-[inset_0_2px_0_var(--broadcast)]',
        )}
        role="tab"
        tabindex="-1"
        aria-selected={active}
        onmousedown={(e) => e.button === 0 && showTab(tab.id)}
        onauxclick={(e) => e.button === 1 && closeTab(tab.id)}>
        {#if meta?.kind === 'rdp'}
          <MonitorIcon class="size-3.5 shrink-0" />
        {:else if meta?.kind === 'ssh'}
          <TerminalIcon class="size-3.5 shrink-0" />
        {:else}
          <SquareTerminalIcon class="size-3.5 shrink-0" />
        {/if}
        <span class={cn('truncate', meta?.exited && 'line-through opacity-60')}>{meta?.title ?? 'Terminal'}</span>
        {#if meta?.exited}<span class="size-1.5 shrink-0 rounded-full bg-destructive" title="Conexão encerrada"></span>{/if}
        {#if count > 1}<span class="rounded bg-muted px-1 text-[10px] text-muted-foreground">{count}</span>{/if}
        <button
          class="ml-0.5 flex size-5 shrink-0 items-center justify-center rounded opacity-0 group-hover:opacity-70 hover:bg-muted hover:opacity-100"
          title="Fechar aba"
          onmousedown={(e) => e.stopPropagation()}
          onclick={() => closeTab(tab.id)}><XIcon class="size-3" /></button>
      </div>
    {/each}

    <DropdownMenu.Root>
      <DropdownMenu.Trigger>
        {#snippet child({ props })}
          <button {...props} class="mb-1 ml-1 flex size-7 shrink-0 items-center justify-center self-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground" title="Nova aba">
            <PlusIcon class="size-4" />
          </button>
        {/snippet}
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="start" class="w-56">
        <DropdownMenu.Item onSelect={() => void openDialog({ type: 'palette' })}>
          <SearchIcon /> Conectar a um host… <DropdownMenu.Shortcut>Ctrl+K</DropdownMenu.Shortcut>
        </DropdownMenu.Item>
        <DropdownMenu.Separator />
        <DropdownMenu.Label>Terminal local</DropdownMenu.Label>
        {#each app.profiles as profile (profile.id)}
          <DropdownMenu.Item onSelect={() => newTab(localSpec(profile))}><SquareTerminalIcon /> {profile.name}</DropdownMenu.Item>
        {/each}
      </DropdownMenu.Content>
    </DropdownMenu.Root>
  </nav>

  {#if current}
    <div class="flex items-center gap-0.5 px-2">
      <button class="flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground" title="Dividir à direita (Ctrl+Shift+D)" onclick={() => split('row')}><ColumnsIcon class="size-4" /></button>
      <button class="flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground" title="Dividir abaixo (Ctrl+Shift+E)" onclick={() => split('col')}><RowsIcon class="size-4" /></button>
      <button
        class={cn(
          'flex h-7 items-center gap-1.5 rounded-md px-2 text-xs text-muted-foreground hover:bg-muted hover:text-foreground',
          current.broadcast && 'bg-[color-mix(in_oklch,var(--broadcast)_18%,transparent)] text-[var(--broadcast)]',
        )}
        title="Broadcast: digitar em todos os painéis da aba (Ctrl+Shift+B)"
        onclick={toggleBroadcast}><RadioTowerIcon class="size-4" /> Broadcast</button>
    </div>
  {/if}

  <div class="flex items-stretch">
    <button class="flex w-11 items-center justify-center text-muted-foreground hover:bg-muted hover:text-foreground" title="Minimizar" onclick={() => win.minimize()}><MinusIcon class="size-4" /></button>
    <button class="flex w-11 items-center justify-center text-muted-foreground hover:bg-muted hover:text-foreground" title="Maximizar" onclick={() => win.toggleMaximize()}><SquareIcon class="size-3.5" /></button>
    <button class="flex w-11 items-center justify-center text-muted-foreground hover:bg-destructive hover:text-white" title="Fechar" onclick={() => win.close()}><XIcon class="size-4" /></button>
  </div>
</header>
