import type { HostView, Tunnel, TunnelStatus } from './backend';

/** `<select>` nativo no mesmo visual do `Input` do shadcn. */
export const selectClass =
  'h-9 w-full rounded-md border border-input bg-transparent px-2.5 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 dark:bg-input/30 [&>option]:bg-popover [&>option]:text-popover-foreground';

/** Iniciais do nome, para o avatar do host. */
export function initials(label: string) {
  return label
    .split(/[\s._-]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0].toUpperCase())
    .join('');
}

/** Cor estável por id (matiz 0–359), para reconhecer o host de relance. */
export function hue(id: string) {
  const hash = [...id].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);
  // Ângulo áureo: ids parecidos ainda caem em cores bem diferentes.
  return Math.round((hash * 137.508) % 360);
}

export const defaultPort = (host: Pick<HostView, 'protocol'>) => (host.protocol === 'rdp' ? 3389 : 22);

/** `user@host[:porta]`, omitindo a porta padrão do protocolo. */
export function address(host: HostView, username = host.username) {
  const port = host.port !== defaultPort(host) ? `:${host.port}` : '';
  return `${username ? `${username}@` : ''}${host.hostname}${port}`;
}

export function tunnelRoute(t: Tunnel) {
  if (t.kind === 'dynamic') return `SOCKS ${t.bindHost}:${t.bindPort}`;
  const from = t.kind === 'remote' ? `servidor:${t.bindPort}` : `${t.bindHost === '127.0.0.1' ? 'localhost' : t.bindHost}:${t.bindPort}`;
  return `${from} → ${t.destHost}:${t.destPort}`;
}

export function tunnelStatusText(status: TunnelStatus | undefined) {
  switch (status?.state) {
    case undefined:
      return 'parado';
    case 'connecting':
      return 'conectando…';
    case 'running':
      return status.connections === 1 ? '1 conexão' : status.connections ? `${status.connections} conexões` : 'ativo';
    case 'reconnecting':
      return `reconectando (tentativa ${status.attempt})…`;
    case 'error':
      return status.message;
  }
}

/** Grupos (pastas) com seus hosts: sem pasta primeiro, depois em ordem alfabética. */
export function groupHosts(hosts: HostView[]) {
  const byGroup = new Map<string, HostView[]>();
  for (const host of [...hosts].sort((a, b) => a.label.localeCompare(b.label))) {
    const list = byGroup.get(host.group) ?? [];
    list.push(host);
    byGroup.set(host.group, list);
  }
  return [...byGroup.entries()].sort(([a], [b]) => (a === '' ? -1 : b === '' ? 1 : a.localeCompare(b)));
}

export function matchHost(host: HostView, query: string) {
  const q = query.trim().toLowerCase();
  return !q || [host.label, host.hostname, host.username, host.group].some((v) => v.toLowerCase().includes(q));
}

/** `1.4 GB`, `512 KB`… (base 1024). */
export function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KB', 'MB', 'GB', 'TB', 'PB'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

/** `3d 4h`, `5h 12min`, `8min`. */
export function formatUptime(seconds: number) {
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}min`;
  return `${m}min`;
}
