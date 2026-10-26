import { asFailure, backend, type ConnectFailure, type HostView, type RdpInput, type RouteOptions } from './backend';

// Mensagens binárias do backend (ver `rdp.rs`).
const MSG_RECTS = 1;
const MSG_SIZE = 2;
const MSG_POINTER = 3;
const MSG_POINTER_DEFAULT = 4;
const MSG_POINTER_HIDDEN = 5;

/** Espera o redimensionamento assentar: no xRDP antigo, cada mudança de resolução reconecta. */
const RESIZE_DELAY = 400;

/** `KeyboardEvent.code` → scancode (conjunto 1; 0xE0xx = estendido). Independe do layout local. */
const SCANCODES: Record<string, number> = {
  Escape: 0x01, Digit1: 0x02, Digit2: 0x03, Digit3: 0x04, Digit4: 0x05, Digit5: 0x06, Digit6: 0x07,
  Digit7: 0x08, Digit8: 0x09, Digit9: 0x0a, Digit0: 0x0b, Minus: 0x0c, Equal: 0x0d, Backspace: 0x0e,
  Tab: 0x0f, KeyQ: 0x10, KeyW: 0x11, KeyE: 0x12, KeyR: 0x13, KeyT: 0x14, KeyY: 0x15, KeyU: 0x16,
  KeyI: 0x17, KeyO: 0x18, KeyP: 0x19, BracketLeft: 0x1a, BracketRight: 0x1b, Enter: 0x1c,
  ControlLeft: 0x1d, KeyA: 0x1e, KeyS: 0x1f, KeyD: 0x20, KeyF: 0x21, KeyG: 0x22, KeyH: 0x23,
  KeyJ: 0x24, KeyK: 0x25, KeyL: 0x26, Semicolon: 0x27, Quote: 0x28, Backquote: 0x29, ShiftLeft: 0x2a,
  Backslash: 0x2b, KeyZ: 0x2c, KeyX: 0x2d, KeyC: 0x2e, KeyV: 0x2f, KeyB: 0x30, KeyN: 0x31, KeyM: 0x32,
  Comma: 0x33, Period: 0x34, Slash: 0x35, ShiftRight: 0x36, NumpadMultiply: 0x37, AltLeft: 0x38,
  Space: 0x39, CapsLock: 0x3a, F1: 0x3b, F2: 0x3c, F3: 0x3d, F4: 0x3e, F5: 0x3f, F6: 0x40, F7: 0x41,
  F8: 0x42, F9: 0x43, F10: 0x44, NumLock: 0x45, ScrollLock: 0x46, Numpad7: 0x47, Numpad8: 0x48,
  Numpad9: 0x49, NumpadSubtract: 0x4a, Numpad4: 0x4b, Numpad5: 0x4c, Numpad6: 0x4d, NumpadAdd: 0x4e,
  Numpad1: 0x4f, Numpad2: 0x50, Numpad3: 0x51, Numpad0: 0x52, NumpadDecimal: 0x53,
  IntlBackslash: 0x56, F11: 0x57, F12: 0x58, F13: 0x64, F14: 0x65, F15: 0x66, F16: 0x67, F17: 0x68,
  F18: 0x69, F19: 0x6a, F20: 0x6b, F21: 0x6c, F22: 0x6d, F23: 0x6e, F24: 0x76,
  // ABNT2: tecla "/?" ao lado do Shift direito e "." do teclado numérico.
  IntlRo: 0x73, IntlYen: 0x7d, NumpadComma: 0x7e,
  NumpadEnter: 0xe01c, ControlRight: 0xe01d, NumpadDivide: 0xe035, PrintScreen: 0xe037,
  AltRight: 0xe038, Home: 0xe047, ArrowUp: 0xe048, PageUp: 0xe049, ArrowLeft: 0xe04b,
  ArrowRight: 0xe04d, End: 0xe04f, ArrowDown: 0xe050, PageDown: 0xe051, Insert: 0xe052,
  Delete: 0xe053, MetaLeft: 0xe05b, MetaRight: 0xe05c, ContextMenu: 0xe05d,
  AudioVolumeMute: 0xe020, AudioVolumeDown: 0xe02e, AudioVolumeUp: 0xe030,
  MediaPlayPause: 0xe022, MediaStop: 0xe024, MediaTrackPrevious: 0xe010, MediaTrackNext: 0xe019,
};

export interface RdpCallbacks {
  onFocus(): void;
  onSession(id: number | null): void;
  onExit(): void;
  resolveSsh(error: ConnectFailure, options: RouteOptions): Promise<RouteOptions | null>;
  hostLabel(id: string): string | undefined;
}

/**
 * Área de trabalho remota num painel: um canvas com a tela do servidor e o input
 * (teclado por scancode, mouse, roda) indo para o backend.
 *
 * Como o `TermPane`, vive num `host` próprio fora do Svelte: mudar a árvore de painéis
 * só move o elemento, sem derrubar a sessão.
 */
export class RdpPane {
  readonly host = document.createElement('div');
  sessionId: number | null = null;
  exited = false;

  private canvas = document.createElement('canvas');
  private ctx = this.canvas.getContext('2d', { alpha: false })!;
  private status = document.createElement('div');
  private started = false;
  private connecting = false;
  /** Acks de mensagens desenhadas antes de o backend devolver o id da sessão. */
  private earlyAcks = 0;
  private resizeTimer = 0;
  private requested = { width: 0, height: 0 };
  private queue: RdpInput[] = [];
  private moveFrame = 0;
  private pendingMove: { x: number; y: number } | null = null;
  /** Sincroniza as travas (Caps/Num Lock) no primeiro evento de teclado depois de ganhar o foco. */
  private needSync = true;
  private ctrlDownAt = -1;

  constructor(
    readonly spec: { kind: 'rdp'; host: HostView },
    private cb: RdpCallbacks,
  ) {
    this.host.className = 'rdp-host';
    this.host.tabIndex = 0;
    this.canvas.className = 'rdp-canvas';
    this.canvas.hidden = true;
    this.status.className = 'rdp-status';
    this.host.append(this.canvas, this.status);
    this.bindInput();
  }

  attach(container: HTMLElement) {
    if (this.host.parentElement !== container) container.appendChild(this.host);
    this.layout();
  }

  async start() {
    if (this.started) return;
    this.started = true;
    this.exited = false;
    await this.connect();
  }

  restart() {
    this.started = false;
    void this.start();
  }

  private async connect() {
    const host = this.spec.host;
    const jump = host.jumpHostId ? this.cb.hostLabel(host.jumpHostId) : undefined;
    this.showStatus(`Conectando a ${host.username}@${host.hostname}${host.port !== 3389 ? `:${host.port}` : ''}${jump ? ` via ${jump}` : ''}…`);
    this.connecting = true;
    const size = this.desiredSize();
    this.requested = size;
    let options: RouteOptions = {};
    for (;;) {
      try {
        this.sessionId = await backend.connectRdp(host.id, options, size.width, size.height, this.onFrame, this.onExit);
        this.connecting = false;
        if (this.earlyAcks) backend.ack(this.sessionId, this.earlyAcks);
        this.earlyAcks = 0;
        this.cb.onSession(this.sessionId);
        // O painel pode ter mudado de tamanho durante a conexão.
        this.scheduleResize();
        return;
      } catch (err) {
        const error = asFailure(err, host.id);
        const next = await this.cb.resolveSsh(error, options);
        if (!next) {
          this.connecting = false;
          const where = error.hostId !== host.id ? `[${this.cb.hostLabel(error.hostId) ?? 'gateway SSH'}] ` : '';
          this.finish(`${where}${describeError(error)}`);
          return;
        }
        options = next;
      }
    }
  }

  private onFrame = (buf: ArrayBuffer) => {
    const view = new DataView(buf);
    switch (view.getUint8(0)) {
      case MSG_RECTS: {
        const count = view.getUint16(1, true);
        let at = 3;
        for (let i = 0; i < count; i++) {
          const x = view.getUint16(at, true);
          const y = view.getUint16(at + 2, true);
          const w = view.getUint16(at + 4, true);
          const h = view.getUint16(at + 6, true);
          at += 8;
          const pixels = new Uint8ClampedArray(buf, at, w * h * 4);
          this.ctx.putImageData(new ImageData(pixels, w, h), x, y);
          at += w * h * 4;
        }
        if (this.sessionId !== null) backend.ack(this.sessionId, 1);
        else this.earlyAcks++;
        break;
      }
      case MSG_SIZE: {
        this.canvas.width = view.getUint16(1, true);
        this.canvas.height = view.getUint16(3, true);
        this.canvas.hidden = false;
        this.hideStatus();
        this.layout();
        break;
      }
      case MSG_POINTER:
        this.setPointer(buf);
        break;
      case MSG_POINTER_DEFAULT:
        this.canvas.style.cursor = 'default';
        break;
      case MSG_POINTER_HIDDEN:
        this.canvas.style.cursor = 'none';
        break;
    }
  };

  private onExit = (reason: string | null) => this.finish(reason ? `Sessão encerrada: ${reason}` : 'Sessão encerrada.');

  private finish(message: string) {
    this.exited = true;
    this.sessionId = null;
    this.cb.onSession(null);
    this.canvas.style.cursor = 'default';
    this.showStatus(message, true);
    this.cb.onExit();
  }

  private setPointer(buf: ArrayBuffer) {
    const view = new DataView(buf);
    const [w, h, hx, hy] = [1, 3, 5, 7].map((at) => view.getUint16(at, true));
    if (w === 0 || h === 0) {
      this.canvas.style.cursor = 'none';
      return;
    }
    const cursor = document.createElement('canvas');
    cursor.width = w;
    cursor.height = h;
    cursor.getContext('2d')!.putImageData(new ImageData(new Uint8ClampedArray(buf, 9, w * h * 4), w, h), 0, 0);
    this.canvas.style.cursor = `url(${cursor.toDataURL()}) ${hx} ${hy}, default`;
  }

  private showStatus(message: string, ended = false) {
    this.status.replaceChildren();
    const text = document.createElement('p');
    text.textContent = message;
    this.status.append(text);
    if (ended) {
      const button = document.createElement('button');
      button.className = 'primary';
      button.textContent = 'Reconectar';
      button.onclick = () => this.restart();
      const hint = document.createElement('span');
      hint.className = 'rdp-hint';
      hint.textContent = 'ou Enter';
      this.status.append(button, hint);
    }
    this.status.hidden = false;
    this.status.classList.toggle('ended', ended);
  }

  private hideStatus() {
    this.status.hidden = true;
  }

  /** Tamanho de tela a pedir ao servidor: o do painel (ou o fixo do host). */
  private desiredSize() {
    const { width, height, hidpi } = this.spec.host.rdp;
    if (width > 0 && height > 0) return { width, height };
    const scale = hidpi ? devicePixelRatio : 1;
    // Largura par: exigência do canal de resolução dinâmica.
    return {
      width: Math.max(200, Math.floor((this.host.clientWidth * scale) / 2) * 2),
      height: Math.max(200, Math.floor(this.host.clientHeight * scale)),
    };
  }

  /** Encaixa o canvas no painel mantendo a proporção (só reduz; nunca amplia além de 1:1). */
  private layout() {
    const { width, height } = this.canvas;
    const boxW = this.host.clientWidth;
    const boxH = this.host.clientHeight;
    if (!width || !height || !boxW || !boxH) return;
    const natural = this.spec.host.rdp.hidpi ? 1 / devicePixelRatio : 1;
    const scale = Math.min(natural, boxW / width, boxH / height);
    this.canvas.style.width = `${Math.round(width * scale)}px`;
    this.canvas.style.height = `${Math.round(height * scale)}px`;
  }

  fit() {
    this.layout();
    this.scheduleResize();
  }

  private scheduleResize() {
    const { width, height } = this.spec.host.rdp;
    if ((width > 0 && height > 0) || this.host.clientWidth === 0) return;
    clearTimeout(this.resizeTimer);
    this.resizeTimer = window.setTimeout(() => {
      const size = this.desiredSize();
      if (this.sessionId === null || (size.width === this.requested.width && size.height === this.requested.height)) return;
      this.requested = size;
      backend.resize(this.sessionId, size.width, size.height);
    }, RESIZE_DELAY);
  }

  focus() {
    this.host.focus({ preventScroll: true });
  }

  // Área de transferência chega na próxima etapa (canal CLIPRDR).
  async copy() {}
  async paste() {}
  dismissPrompt() {}

  dispose() {
    clearTimeout(this.resizeTimer);
    cancelAnimationFrame(this.moveFrame);
    if (this.sessionId !== null) backend.kill(this.sessionId);
    this.host.remove();
  }

  // ---------- Input ----------

  private send(event: RdpInput) {
    if (this.pendingMove) {
      this.queue.push({ t: 'move', ...this.pendingMove });
      this.pendingMove = null;
    }
    this.queue.push(event);
    this.flush();
  }

  private flush() {
    if (this.sessionId === null || this.queue.length === 0) {
      this.queue.length = 0;
      return;
    }
    backend.rdpInput(this.sessionId, this.queue);
    this.queue = [];
  }

  /** Posição do mouse em pixels da tela remota. */
  private point(e: MouseEvent) {
    const rect = this.canvas.getBoundingClientRect();
    const x = ((e.clientX - rect.left) / rect.width) * this.canvas.width;
    const y = ((e.clientY - rect.top) / rect.height) * this.canvas.height;
    return {
      x: Math.max(0, Math.min(this.canvas.width - 1, Math.round(x))),
      y: Math.max(0, Math.min(this.canvas.height - 1, Math.round(y))),
    };
  }

  private bindInput() {
    const c = this.canvas;
    c.addEventListener('pointermove', (e) => {
      this.pendingMove = this.point(e);
      // Movimentos são agrupados por quadro; cliques e teclas saem na hora (levando o último movimento).
      if (!this.moveFrame) {
        this.moveFrame = requestAnimationFrame(() => {
          this.moveFrame = 0;
          if (this.pendingMove) {
            this.queue.push({ t: 'move', ...this.pendingMove });
            this.pendingMove = null;
            this.flush();
          }
        });
      }
    });
    c.addEventListener('pointerdown', (e) => {
      e.preventDefault();
      this.focus();
      c.setPointerCapture(e.pointerId);
      this.pendingMove = this.point(e);
      this.send({ t: 'button', button: e.button, down: true });
    });
    c.addEventListener('pointerup', (e) => {
      this.pendingMove = this.point(e);
      this.send({ t: 'button', button: e.button, down: false });
    });
    c.addEventListener('contextmenu', (e) => e.preventDefault());
    c.addEventListener(
      'wheel',
      (e) => {
        e.preventDefault();
        const vertical = Math.abs(e.deltaY) >= Math.abs(e.deltaX);
        const delta = vertical ? e.deltaY : e.deltaX;
        if (!delta) return;
        // Um "dente" da roda = 120 unidades no RDP (~100 px no navegador). Para cima é positivo.
        const pixels = e.deltaMode === WheelEvent.DOM_DELTA_LINE ? delta * 40 : delta;
        const units = Math.max(-255, Math.min(255, Math.round(-pixels * 1.2)));
        if (units) this.send({ t: 'wheel', delta: vertical ? units : -units, vertical });
      },
      { passive: false },
    );
    // Botões laterais do mouse não devem navegar a webview.
    c.addEventListener('auxclick', (e) => e.preventDefault());

    this.host.addEventListener('keydown', (e) => this.onKey(e, true));
    this.host.addEventListener('keyup', (e) => this.onKey(e, false));
    this.host.addEventListener('focus', () => {
      this.needSync = true;
      this.cb.onFocus();
    });
    this.host.addEventListener('blur', () => {
      if (this.sessionId !== null) this.send({ t: 'releaseAll' });
    });
  }

  private onKey(e: KeyboardEvent, down: boolean) {
    if (this.exited) {
      if (down && e.key === 'Enter' && !this.connecting) {
        e.preventDefault();
        this.restart();
      }
      return;
    }
    if (this.sessionId === null) return;
    e.preventDefault();
    e.stopPropagation();
    if (this.needSync) {
      this.needSync = false;
      this.queue.push({
        t: 'sync',
        caps: e.getModifierState('CapsLock'),
        num: e.getModifierState('NumLock'),
        scroll: e.getModifierState('ScrollLock'),
      });
    }
    // AltGr no Windows chega como Ctrl esquerdo + Alt direito no mesmo instante: o Ctrl é falso.
    if (e.code === 'ControlLeft' && down) this.ctrlDownAt = e.timeStamp;
    if (e.code === 'AltRight' && down && e.timeStamp - this.ctrlDownAt < 5) {
      this.queue.push({ t: 'key', code: SCANCODES.ControlLeft, down: false });
    }
    const code = SCANCODES[e.code];
    if (code !== undefined) this.send({ t: 'key', code, down });
    else if (e.key.length === 1) this.send({ t: 'unicode', ch: e.key, down });
  }
}

function describeError(error: ConnectFailure): string {
  switch (error.kind) {
    case 'unknownHost':
      return 'Conexão cancelada: servidor não confiado.';
    case 'hostKeyChanged':
      return `ATENÇÃO: a chave do gateway SSH mudou (${error.keyType} ${error.fingerprint}). Possível ataque man-in-the-middle. Se a mudança é esperada, remova a linha ${error.line} do ~/.ssh/known_hosts.`;
    case 'certChanged':
      return 'Conexão cancelada: o certificado do servidor mudou.';
    case 'needPassword':
    case 'needPassphrase':
      return 'Conexão cancelada.';
    case 'authFailed':
    case 'other':
      return error.message;
  }
}
