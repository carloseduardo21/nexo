import { mount } from 'svelte';
import '@xterm/xterm/css/xterm.css';
import './app.css';
import App from './App.svelte';
import { app as state, connectHost, openDialog, showView, type View } from './lib/state.svelte';
import { side, type SidePanel } from './lib/side.svelte';
import { hostinfo } from './lib/hostinfo.svelte';

// Fora do Tauri (navegador comum, só em dev): backend simulado para ver a interface.
const mocked = import.meta.env.DEV && !('__TAURI_INTERNALS__' in window);
if (mocked) {
  const { installDevMock } = await import('./dev-mock');
  installDevMock();
}

const app = mount(App, { target: document.getElementById('app')! });

if (mocked) {
  const params = new URLSearchParams(location.search);
  const view = params.get('view') as View | null;
  if (view && view !== 'session') showView(view);
  // `?panel=host|sftp` conecta no primeiro host SSH e abre o painel lateral (`&tab=containers`).
  const panel = params.get('panel') as SidePanel | null;
  if (panel) {
    setTimeout(() => {
      const host = state.hosts.find((h) => h.protocol === 'ssh');
      if (host) void connectHost(host);
      side.panel = panel;
      if (params.get('tab') === 'containers') hostinfo.tab = 'containers';
    }, 300);
  }
  // `?dialog=host|newhost|credential|tunnel|palette|generate` abre um diálogo (depois de carregar os dados).
  const dialog = params.get('dialog');
  if (dialog) {
    setTimeout(() => {
      const host = state.hosts.find((h) => h.protocol === (params.get('rdp') ? 'rdp' : 'ssh'));
      const open = {
        host: () => openDialog({ type: 'hostForm', host: host ?? null }),
        newhost: () => openDialog({ type: 'hostForm', host: null }),
        credential: () => openDialog({ type: 'credentialForm', credential: state.credentials[1] ?? null }),
        tunnel: () => openDialog({ type: 'tunnelForm', tunnel: state.tunnels[0] ?? null }),
        palette: () => openDialog({ type: 'palette' }),
        generate: () => openDialog({ type: 'generateKey' }),
      }[dialog];
      void open?.();
    }, 300);
  }
}

export default app;
