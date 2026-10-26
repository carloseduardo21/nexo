/** Painel à direita das sessões: arquivos (SFTP) ou o painel do host (métricas, containers). */
export type SidePanel = 'sftp' | 'host';

export const side = $state({ panel: null as SidePanel | null });

export function togglePanel(panel: SidePanel) {
  side.panel = side.panel === panel ? null : panel;
}
