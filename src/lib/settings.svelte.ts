import { invoke } from '@tauri-apps/api/core';

export type ThemeMode = 'dark' | 'light' | 'system';
export type Accent = 'teal' | 'blue' | 'violet' | 'orange' | 'rose';

export interface Settings {
  theme: ThemeMode;
  accent: Accent;
  fontFamily: string;
  fontSize: number;
  cursorBlink: boolean;
  /** Abrir um terminal local ao iniciar (senão, começa na página de hosts). */
  startWithTerminal: boolean;
  /** Lista de hosts ao lado das sessões. */
  hostsSidebar: boolean;
  /** Kubeconfig da página Kubernetes; '' = o padrão (KUBECONFIG ou ~/.kube/config). */
  kubeconfig: string;
  /** Terminais locais (ids de perfil) em que o painel usa `sudo docker`. */
  dockerSudoLocal: string[];
}

export const DEFAULT_FONT = '"Cascadia Mono", "Cascadia Code", "JetBrains Mono", Consolas, "DejaVu Sans Mono", monospace';

const defaults: Settings = {
  theme: 'dark',
  accent: 'teal',
  fontFamily: DEFAULT_FONT,
  fontSize: 14,
  cursorBlink: true,
  startWithTerminal: false,
  hostsSidebar: true,
  kubeconfig: '',
  dockerSudoLocal: [],
};

export const settings = $state<Settings>({ ...defaults });

/** Tema efetivo (o "sistema" resolvido). */
export const theme = $state({ dark: true });

const media = window.matchMedia('(prefers-color-scheme: dark)');
const listeners = new Set<() => void>();

/** Chamado quando tema ou fonte mudam (os terminais se atualizam). */
export function onAppearanceChange(fn: () => void) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

function apply() {
  theme.dark = settings.theme === 'system' ? media.matches : settings.theme === 'dark';
  const root = document.documentElement;
  root.classList.toggle('dark', theme.dark);
  root.style.colorScheme = theme.dark ? 'dark' : 'light';
  if (settings.accent === 'teal') delete root.dataset.accent;
  else root.dataset.accent = settings.accent;
  for (const fn of listeners) fn();
}

media.addEventListener('change', () => settings.theme === 'system' && apply());

export async function loadSettings() {
  const saved = await invoke<Partial<Settings> | null>('settings_get').catch(() => null);
  Object.assign(settings, defaults, saved ?? {});
  apply();
}

let saveTimer = 0;

/** Muda uma ou mais preferências: aplica na hora e grava (com um pequeno atraso para agrupar). */
export function updateSettings(patch: Partial<Settings>) {
  Object.assign(settings, patch);
  apply();
  clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => void invoke('settings_set', { settings: $state.snapshot(settings) }).catch(console.error), 300);
}

apply();
