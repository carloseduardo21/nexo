import { Terminal, type ITheme } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebglAddon } from '@xterm/addon-webgl';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { asFailure, backend, type ConnectError, type ConnectFailure, type HostView, type RouteOptions, type ShellProfile } from './backend';

/** O que um painel mostra: terminal (local ou SSH) ou área de trabalho RDP (`rdp.ts`). */
export type TermSpec = { kind: 'local'; profile: ShellProfile } | { kind: 'ssh'; host: HostView };
export type PaneSpec = TermSpec | { kind: 'rdp'; host: HostView };

/** Acks em lotes: o backend só pausa a leitura acima de 2 MB pendentes, então folga de 32 KB é irrelevante. */
const ACK_BATCH = 32 * 1024;

const isWindows = navigator.userAgent.includes('Windows');

/** Linha do cursor terminando num pedido de senha: `[sudo] password for x:`, `Senha:`, `Enter passphrase…:`. */
const PASSWORD_PROMPT = /(password|senha|passphrase|contraseña|passwort)[^\n]{0,80}:\s*$/i;

export const terminalTheme: ITheme = {
  background: '#0f1320',
  foreground: '#d7deef',
  cursor: '#3ee8c4',
  cursorAccent: '#0f1320',
  selectionBackground: '#3a4f8a99',
  black: '#1b2032',
  red: '#ff6b81',
  green: '#3ee8a0',
  yellow: '#ffd166',
  blue: '#5b8cff',
  magenta: '#c77dff',
  cyan: '#3ee8c4',
  white: '#d7deef',
  brightBlack: '#5c6788',
  brightRed: '#ff8fa0',
  brightGreen: '#6ff5bd',
  brightYellow: '#ffe09a',
  brightBlue: '#8aaeff',
  brightMagenta: '#dba8ff',
  brightCyan: '#7ff2db',
  brightWhite: '#ffffff',
};

export interface TermCallbacks {
  onInput(data: string): void;
  onExit(code: number | null): void;
  onTitle(title: string): void;
  onFocus(): void;
  /** Sessão do backend começou (id) ou terminou (null). */
  onSession(id: number | null): void;
  /** Latência em ms: `echo` = tecla até o eco chegar no app; `network` = medido no Rust (rede + servidor). */
  onLatency(latency: { echo?: number; network?: number }): void;
  /** O terminal está (posição do cursor na tela) ou deixou de estar (null) num prompt de senha. */
  onPasswordPrompt(at: { x: number; y: number } | null): void;
  /** Tecla enquanto há prompt de senha; true = consumida (ex.: navegar nas sugestões). */
  onPromptKey(e: KeyboardEvent): boolean;
  /** Decide o que fazer com um erro de conexão SSH: novas opções para tentar de novo, ou null para desistir. */
  resolveSsh(error: ConnectFailure, options: RouteOptions): Promise<RouteOptions | null>;
  /** Nome do jump host, para a mensagem de "conectando". */
  hostLabel(id: string): string | undefined;
}

/**
 * Um terminal xterm.js + a sessão do backend por trás dele.
 *
 * O xterm vive num `host` próprio, fora do ciclo de vida do Svelte: quando a árvore
 * de painéis muda (split, fechar), o componente só move o `host` de lugar no DOM,
 * sem recriar o terminal nem perder o scrollback.
 */
export class TermPane {
  readonly host = document.createElement('div');
  readonly term: Terminal;
  sessionId: number | null = null;
  exited = false;

  private fitAddon = new FitAddon();
  private opened = false;
  private started = false;
  private unacked = 0;
  private probeStart = 0;
  private echoSamples: number[] = [];
  private promptTimer = 0;
  private promptActive = false;
  /** Tecla consumida no keydown: ignora também o keypress/keyup dela. */
  private swallowKey = '';

  constructor(
    readonly spec: TermSpec,
    private cb: TermCallbacks,
  ) {
    this.host.className = 'term-host';
    this.term = new Terminal({
      fontFamily: '"Cascadia Mono", "Cascadia Code", "JetBrains Mono", Consolas, "DejaVu Sans Mono", monospace',
      fontSize: 14,
      lineHeight: 1.15,
      cursorBlink: true,
      scrollback: 10_000,
      allowProposedApi: true,
      theme: terminalTheme,
      windowsPty: isWindows && spec.kind === 'local' && spec.profile.kind !== 'unix' ? { backend: 'conpty' } : undefined,
    });
    this.term.loadAddon(this.fitAddon);
    this.term.loadAddon(new Unicode11Addon());
    this.term.unicode.activeVersion = '11';

    this.term.onData((data) => {
      if (this.probeStart === 0) this.probeStart = performance.now();
      if (this.promptActive) this.setPrompt(null);
      this.cb.onInput(data);
    });
    this.term.onTitleChange((title) => this.cb.onTitle(title));
    this.term.attachCustomKeyEventHandler((e) => this.handleKey(e));
    this.host.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      void (this.term.hasSelection() ? this.copy() : this.paste());
    });
  }

  /** Coloca o terminal dentro do container (primeira vez: abre e liga a GPU). */
  attach(container: HTMLElement) {
    if (this.host.parentElement !== container) container.appendChild(this.host);
    if (!this.opened) {
      this.opened = true;
      this.term.open(this.host);
      this.loadWebgl();
      this.term.textarea?.addEventListener('focus', () => this.cb.onFocus());
    }
    this.fit();
  }

  async start() {
    if (this.started) return;
    this.started = true;
    this.exited = false;
    if (this.spec.kind === 'ssh') return this.connectSsh(this.spec.host);
    try {
      this.sessionId = await backend.spawnPty(this.spec.profile.id, this.term.cols, this.term.rows, this.onData, this.onExit);
      this.cb.onSession(this.sessionId);
    } catch (err) {
      this.term.write(`\x1b[31mFalha ao iniciar ${this.spec.profile.name}: ${err}\x1b[0m\r\n`);
      this.handleExit(null);
    }
  }

  /** Reconecta uma sessão SSH encerrada. */
  restart() {
    this.started = false;
    this.term.write('\r\n');
    void this.start();
  }

  private async connectSsh(host: HostView) {
    const jump = host.jumpHostId ? this.cb.hostLabel(host.jumpHostId) : undefined;
    const via = jump ? ` via ${jump}` : '';
    this.term.write(`\x1b[2mConectando a ${host.username}@${host.hostname}:${host.port}${via}…\x1b[0m\r\n`);
    let options: RouteOptions = {};
    for (;;) {
      try {
        this.sessionId = await backend.connectSsh(host.id, options, this.term.cols, this.term.rows, this.onData, this.onExit, (network) =>
          this.cb.onLatency({ network }),
        );
        this.cb.onSession(this.sessionId);
        return;
      } catch (err) {
        const error = asFailure(err, host.id);
        const next = await this.cb.resolveSsh(error, options);
        if (!next) {
          const where = error.hostId !== host.id ? `[${this.cb.hostLabel(error.hostId) ?? 'jump host'}] ` : '';
          this.term.write(`\x1b[31m${where}${describeError(error)}\x1b[0m\r\n`);
          this.handleExit(null);
          return;
        }
        options = next;
      }
    }
  }

  private onData = (data: Uint8Array) => {
    if (this.probeStart !== 0) this.recordEcho(performance.now() - this.probeStart);
    this.term.write(data, () => {
      this.schedulePromptCheck();
      this.unacked += data.length;
      if (this.unacked >= ACK_BATCH && this.sessionId !== null) {
        backend.ack(this.sessionId, this.unacked);
        this.unacked = 0;
      }
    });
  };

  private onExit = (code: number | null) => this.handleExit(code);

  private recordEcho(ms: number) {
    this.probeStart = 0;
    if (ms > 1000) return; // sem eco (senha, comando longo): não é latência
    this.echoSamples.push(ms);
    if (this.echoSamples.length > 15) this.echoSamples.shift();
    const sorted = [...this.echoSamples].sort((a, b) => a - b);
    this.cb.onLatency({ echo: sorted[sorted.length >> 1] });
  }

  private schedulePromptCheck() {
    if (this.promptTimer) return;
    // Espera a saída assentar: o prompt costuma chegar junto com outras linhas.
    this.promptTimer = window.setTimeout(() => {
      this.promptTimer = 0;
      this.checkPrompt();
    }, 40);
  }

  private checkPrompt() {
    const buf = this.term.buffer.active;
    if (buf.type === 'alternate') return this.setPrompt(null); // vim, less, htop…
    const line = buf.getLine(buf.baseY + buf.cursorY)?.translateToString(true, 0, buf.cursorX) ?? '';
    if (!PASSWORD_PROMPT.test(line)) return this.setPrompt(null);
    const screen = this.host.querySelector('.xterm-screen');
    if (!screen) return;
    const rect = screen.getBoundingClientRect();
    const cellW = rect.width / this.term.cols;
    const cellH = rect.height / this.term.rows;
    this.setPrompt({ x: rect.left + buf.cursorX * cellW, y: rect.top + (buf.cursorY + 1) * cellH });
  }

  private setPrompt(at: { x: number; y: number } | null) {
    if (!at && !this.promptActive) return;
    this.promptActive = !!at;
    this.cb.onPasswordPrompt(at);
  }

  /** Esconde as sugestões de senha (Esc, ou depois de preencher). */
  dismissPrompt() {
    this.setPrompt(null);
  }

  fit() {
    if (!this.opened || this.host.clientWidth === 0 || this.host.clientHeight === 0) return;
    const { cols, rows } = this.term;
    this.fitAddon.fit();
    if (this.sessionId !== null && (cols !== this.term.cols || rows !== this.term.rows)) {
      backend.resize(this.sessionId, this.term.cols, this.term.rows);
    }
  }

  focus() {
    this.term.focus();
  }

  async copy() {
    const text = this.term.getSelection();
    if (!text) return;
    await navigator.clipboard.writeText(text).catch(() => {});
    this.term.clearSelection();
  }

  async paste() {
    const text = await navigator.clipboard.readText().catch(() => '');
    if (text) this.term.paste(text);
  }

  dispose() {
    if (this.sessionId !== null) backend.kill(this.sessionId);
    this.term.dispose();
    this.host.remove();
  }

  private loadWebgl() {
    try {
      const webgl = new WebglAddon();
      // Se a GPU resetar, cai para o renderer DOM em vez de mostrar tela preta.
      webgl.onContextLoss(() => webgl.dispose());
      this.term.loadAddon(webgl);
    } catch (err) {
      console.warn('[nexo] WebGL indisponível, usando renderer DOM', err);
    }
  }

  private handleKey(e: KeyboardEvent): boolean {
    if (this.swallowKey && e.key === this.swallowKey && e.type !== 'keydown') {
      if (e.type === 'keyup') this.swallowKey = '';
      return false;
    }
    if (this.promptActive && e.type === 'keydown' && this.cb.onPromptKey(e)) {
      e.preventDefault();
      this.swallowKey = e.key;
      return false;
    }
    if (e.type !== 'keydown' || !e.ctrlKey || e.shiftKey || e.altKey) return true;
    // Ctrl+C com texto selecionado copia (como no Windows Terminal); sem seleção, vira ^C normal.
    if (e.code === 'KeyC' && this.term.hasSelection()) {
      void this.copy();
      return false;
    }
    // Ctrl+V: deixa o navegador disparar o evento de paste, que o xterm já trata.
    if (e.code === 'KeyV') return false;
    return true;
  }

  private handleExit(code: number | null) {
    this.setPrompt(null);
    this.exited = true;
    this.sessionId = null;
    this.cb.onSession(null);
    const detail = code === null ? '' : ` com código ${code}`;
    const hint = this.spec.kind === 'ssh' ? 'Enter reconecta, Ctrl+Shift+W fecha' : 'Enter fecha o painel';
    const what = this.spec.kind === 'ssh' ? 'conexão encerrada' : 'processo encerrado';
    this.unacked = 0;
    this.term.write(`\r\n\x1b[2m[${what}${detail} — ${hint}]\x1b[0m\r\n`);
    this.cb.onExit(code);
  }
}

function describeError(error: ConnectError): string {
  switch (error.kind) {
    case 'unknownHost':
      return 'Conexão cancelada: host não confiado.';
    case 'hostKeyChanged':
      return `ATENÇÃO: a chave do servidor mudou (${error.keyType} ${error.fingerprint}). Possível ataque man-in-the-middle. Se a mudança é esperada, remova a linha ${error.line} do ~/.ssh/known_hosts.`;
    case 'certChanged':
    case 'needPassword':
    case 'needPassphrase':
      return 'Conexão cancelada.';
    case 'authFailed':
    case 'other':
      return error.message;
  }
}
