// Só em desenvolvimento, fora do Tauri (navegador comum): simula o backend com dados de exemplo
// para ver e testar a interface. `?view=credentials|tunnels|settings|session` abre direto a página.
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';

const rdp = { domain: '', width: 0, height: 0, hidpi: false, clipboard: true, captureKeys: true, gfx: true };
const base = { auth: 'password', keyPath: '', jumpHostId: '', protocol: 'ssh', rdp, credentialId: '', hasPassword: true, hasPassphrase: false };

const hosts = [
  { ...base, id: 'h1', label: 'web-01', hostname: '10.0.1.11', port: 22, username: 'deploy', group: 'Produção', credentialId: 'c1' },
  { ...base, id: 'h2', label: 'web-02', hostname: '10.0.1.12', port: 22, username: 'deploy', group: 'Produção', credentialId: 'c1' },
  { ...base, id: 'h3', label: 'db-primary', hostname: '10.0.2.5', port: 22, username: 'postgres', group: 'Produção', jumpHostId: 'h5' },
  { ...base, id: 'h4', label: 'desktop-dev', hostname: '192.168.0.40', port: 3389, username: 'carlos', group: 'Laboratório', protocol: 'rdp' },
  { ...base, id: 'h5', label: 'bastion', hostname: 'bastion.empresa.com', port: 2222, username: 'carlos', group: '', auth: 'key' },
  { ...base, id: 'h6', label: 'pi-casa', hostname: '192.168.0.2', port: 22, username: 'pi', group: '' },
  { ...base, id: 'h7', label: 'k8s-node-1', hostname: '10.10.0.21', port: 22, username: 'ubuntu', group: 'Laboratório', auth: 'agent' },
];
const credentials = [
  { id: 'c1', label: 'Deploy produção', username: 'deploy', auth: 'password', keyPath: '', hasPassword: true, hasPassphrase: false, hasKey: false, publicKey: null, usedBy: ['web-01', 'web-02'] },
  { id: 'c2', label: 'Chave do Nexo', username: 'carlos', auth: 'key', keyPath: '', hasPassword: false, hasPassphrase: false, hasKey: true, publicKey: 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJqk… carlos@nexo', usedBy: [] },
];
const tunnels = [
  { id: 't1', label: 'Postgres produção', hostId: 'h3', kind: 'local', bindHost: '127.0.0.1', bindPort: 5432, destHost: 'localhost', destPort: 5432, autoStart: true },
  { id: 't2', label: '', hostId: 'h5', kind: 'dynamic', bindHost: '127.0.0.1', bindPort: 1080, destHost: '', destPort: 0, autoStart: false },
];
const shells = [
  { id: 'pwsh', name: 'PowerShell', kind: 'powershell', program: 'pwsh.exe', args: [] },
  { id: 'wsl', name: 'Ubuntu (WSL)', kind: 'wsl', program: 'wsl.exe', args: [] },
];

export function installDevMock() {
  mockWindows('main');
  const view = new URLSearchParams(location.search).get('view');
  mockIPC((cmd) => {
    switch (cmd) {
      case 'list_shells':
        return shells;
      case 'hosts_list':
        return hosts;
      case 'credentials_list':
        return credentials;
      case 'tunnels_list':
        return tunnels;
      case 'tunnel_statuses':
        return { t1: { state: 'running', connections: 2 } };
      case 'settings_get':
        return { startWithTerminal: view === 'session', theme: new URLSearchParams(location.search).get('theme') ?? 'dark' };
      case 'plugin:app|version':
        return '0.1.0';
      case 'pty_spawn':
        return 1;
      default:
        return null;
    }
  });
}
