<script lang="ts">
  import { toast } from 'svelte-sonner';
  import { open as openFile, save as saveFile } from '@tauri-apps/plugin-dialog';
  import { getVersion } from '@tauri-apps/api/app';
  import MoonIcon from '@lucide/svelte/icons/moon';
  import SunIcon from '@lucide/svelte/icons/sun';
  import MonitorIcon from '@lucide/svelte/icons/monitor';
  import CheckIcon from '@lucide/svelte/icons/check';
  import DownloadIcon from '@lucide/svelte/icons/download';
  import UploadIcon from '@lucide/svelte/icons/upload';
  import FileInputIcon from '@lucide/svelte/icons/file-input';
  import RefreshIcon from '@lucide/svelte/icons/refresh-cw';
  import * as Card from '$lib/components/ui/card/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { Label } from '$lib/components/ui/label/index.js';
  import { Switch } from '$lib/components/ui/switch/index.js';
  import { backend } from '../lib/backend';
  import { openDialog, refreshCredentials, refreshHosts, refreshTunnels } from '../lib/state.svelte';
  import { DEFAULT_FONT, settings, updateSettings, type Accent, type ThemeMode } from '../lib/settings.svelte';
  import { errorText } from '../lib/actions.svelte';
  import { checkForUpdates, scheduleUpdateChecks, updater } from '../lib/updater.svelte';
  import { cn } from '$lib/utils.js';

  const themes: { value: ThemeMode; label: string; icon: typeof MoonIcon }[] = [
    { value: 'dark', label: 'Escuro', icon: MoonIcon },
    { value: 'light', label: 'Claro', icon: SunIcon },
    { value: 'system', label: 'Sistema', icon: MonitorIcon },
  ];
  const accents: { value: Accent; label: string; color: string }[] = [
    { value: 'teal', label: 'Verde-água', color: 'oklch(0.75 0.14 175)' },
    { value: 'blue', label: 'Azul', color: 'oklch(0.65 0.18 262)' },
    { value: 'violet', label: 'Violeta', color: 'oklch(0.65 0.2 295)' },
    { value: 'orange', label: 'Laranja', color: 'oklch(0.72 0.17 52)' },
    { value: 'rose', label: 'Rosa', color: 'oklch(0.66 0.2 15)' },
  ];

  let version = $state('');
  void getVersion().then((v) => (version = v));

  async function exportVault() {
    const path = await saveFile({ title: 'Exportar hosts', defaultPath: 'nexo-hosts.json', filters: [{ name: 'JSON', extensions: ['json'] }] });
    if (!path) return;
    try {
      const count = await backend.exportVault(path);
      toast.success(`${count} ${count === 1 ? 'host exportado' : 'hosts exportados'}`, { description: 'Senhas e chaves não vão no arquivo.' });
    } catch (err) {
      toast.error(errorText(err));
    }
  }

  async function importVault() {
    const path = await openFile({ title: 'Importar hosts do Nexo', multiple: false, filters: [{ name: 'JSON', extensions: ['json'] }] });
    if (typeof path !== 'string') return;
    try {
      const count = await backend.importVault(path);
      await Promise.all([refreshHosts(), refreshCredentials(), refreshTunnels()]);
      toast.success(`${count} ${count === 1 ? 'host importado' : 'hosts importados'}`);
    } catch (err) {
      toast.error(errorText(err));
    }
  }
</script>

{#snippet sshImport()}<Button variant="outline" onclick={() => void openDialog({ type: 'sshImport' })}><FileInputIcon /> Importar</Button>{/snippet}
{#snippet importJson()}<Button variant="outline" onclick={importVault}><DownloadIcon /> Importar</Button>{/snippet}
{#snippet autoUpdate()}
  <Switch
    checked={settings.checkUpdates}
    onCheckedChange={(v) => {
      updateSettings({ checkUpdates: v });
      scheduleUpdateChecks();
    }} />
{/snippet}
{#snippet checkNow()}
  <Button variant="outline" disabled={updater.checking || updater.busy} onclick={() => void checkForUpdates(true)}>
    <RefreshIcon class={cn(updater.checking && 'animate-spin')} /> Verificar agora
  </Button>
{/snippet}
{#snippet exportJson()}<Button variant="outline" onclick={exportVault}><UploadIcon /> Exportar</Button>{/snippet}

{#snippet row(label: string, hint: string, control: import('svelte').Snippet)}
  <div class="flex items-center justify-between gap-6 py-3">
    <div class="grid gap-0.5">
      <span class="text-sm font-medium">{label}</span>
      <span class="text-xs text-muted-foreground">{hint}</span>
    </div>
    {@render control()}
  </div>
{/snippet}

<div class="h-full overflow-y-auto">
  <div class="mx-auto grid max-w-3xl gap-5 px-6 py-6">
    <h1 class="text-lg font-semibold">Configurações</h1>

    <Card.Root>
      <Card.Header>
        <Card.Title>Aparência</Card.Title>
        <Card.Description>Tema do app e dos terminais.</Card.Description>
      </Card.Header>
      <Card.Content class="grid gap-5">
        <div class="grid grid-cols-3 gap-2">
          {#each themes as t (t.value)}
            <button
              class={cn(
                'flex items-center justify-center gap-2 rounded-lg border py-3 text-sm transition-colors hover:bg-accent',
                settings.theme === t.value && 'border-primary bg-primary/8 text-primary',
              )}
              onclick={() => updateSettings({ theme: t.value })}>
              <t.icon class="size-4" />
              {t.label}
            </button>
          {/each}
        </div>
        <div class="grid gap-2">
          <Label>Cor de destaque</Label>
          <div class="flex gap-2">
            {#each accents as a (a.value)}
              <button
                class="flex size-8 items-center justify-center rounded-full ring-offset-2 ring-offset-background transition hover:scale-110"
                class:ring-2={settings.accent === a.value}
                style:background={a.color}
                style:--tw-ring-color={a.color}
                title={a.label}
                aria-label={a.label}
                onclick={() => updateSettings({ accent: a.value })}>
                {#if settings.accent === a.value}<CheckIcon class="size-4 text-white" />{/if}
              </button>
            {/each}
          </div>
        </div>
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>Terminal</Card.Title>
      </Card.Header>
      <Card.Content class="grid gap-4">
        <div class="flex gap-3">
          <div class="grid flex-1 gap-1.5">
            <Label for="s-font">Fonte</Label>
            <Input
              id="s-font"
              value={settings.fontFamily === DEFAULT_FONT ? '' : settings.fontFamily}
              placeholder="Cascadia Mono (padrão)"
              onchange={(e) => updateSettings({ fontFamily: e.currentTarget.value.trim() || DEFAULT_FONT })} />
          </div>
          <div class="grid w-28 gap-1.5">
            <Label for="s-size">Tamanho</Label>
            <Input
              id="s-size"
              type="number"
              min="8"
              max="32"
              value={settings.fontSize}
              onchange={(e) => updateSettings({ fontSize: Math.min(32, Math.max(8, Number(e.currentTarget.value) || 14)) })} />
          </div>
        </div>
        <div class="divide-y">
          {#snippet blink()}<Switch checked={settings.cursorBlink} onCheckedChange={(v) => updateSettings({ cursorBlink: v })} />{/snippet}
          {@render row('Cursor piscando', 'No terminal (local e SSH).', blink)}
          {#snippet start()}<Switch checked={settings.startWithTerminal} onCheckedChange={(v) => updateSettings({ startWithTerminal: v })} />{/snippet}
          {@render row('Abrir um terminal local ao iniciar', 'Desligado, o Nexo abre na página de hosts.', start)}
        </div>
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>Dados</Card.Title>
        <Card.Description>Hosts, credenciais e túneis. Senhas e chaves ficam só no cofre desta máquina (nunca são exportadas).</Card.Description>
      </Card.Header>
      <Card.Content class="divide-y">
        {@render row('Importar do ~/.ssh/config', 'Hosts, usuários, portas, chaves e ProxyJump do OpenSSH.', sshImport)}
        {@render row('Importar arquivo do Nexo', 'Um JSON exportado por outro Nexo (os itens são acrescentados).', importJson)}
        {@render row('Exportar hosts', 'Para backup ou para levar a outra máquina.', exportJson)}
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>Atualizações</Card.Title>
        <Card.Description>Versões novas vêm das releases do GitHub, assinadas; o Nexo avisa e só instala quando você pede.</Card.Description>
      </Card.Header>
      <Card.Content class="divide-y">
        {@render row('Procurar automaticamente', 'Ao abrir o Nexo e a cada 6 horas.', autoUpdate)}
        {@render row(`Nexo ${version}`, 'A versão instalada.', checkNow)}
      </Card.Content>
    </Card.Root>

    <Card.Root>
      <Card.Header>
        <Card.Title>Atalhos</Card.Title>
      </Card.Header>
      <Card.Content>
        <dl class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1.5 text-sm">
          {#each [['Ctrl+K', 'Conectar a um host / buscar'], ['Ctrl+Shift+T', 'Nova aba'], ['Ctrl+Shift+D / E', 'Dividir à direita / abaixo'], ['Ctrl+Shift+W', 'Fechar painel'], ['Ctrl+Shift+B', 'Broadcast na aba'], ['Ctrl+Shift+F', 'Arquivos (SFTP)'], ['Alt+setas', 'Navegar entre painéis'], ['Ctrl+Tab', 'Próxima aba'], ['Ctrl+Alt+Home', 'Soltar o teclado de um painel RDP']] as [keys, action] (keys)}
            <dt><kbd class="rounded border bg-muted px-1.5 py-0.5 font-mono text-[11px]">{keys}</kbd></dt>
            <dd class="text-muted-foreground">{action}</dd>
          {/each}
        </dl>
      </Card.Content>
    </Card.Root>
  </div>
</div>
