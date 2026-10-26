//! Captura de teclado para sessões RDP (Windows): Win, Alt+Tab, Alt+F4, Ctrl+Esc e os atalhos do
//! próprio Nexo vão para o servidor, como no mstsc. O navegador nunca recebe essas teclas (o Windows
//! as trata antes), então a captura é um hook de teclado de baixo nível (`WH_KEYBOARD_LL`).
//!
//! O hook só existe enquanto um painel RDP tem o foco (`capture`/`release`) e só age com a janela
//! do Nexo em primeiro plano. Ctrl+Alt+Home solta o teclado (evento `rdp-release` para o frontend).
//! Ctrl+Alt+Del e Win+L não podem ser capturados por nenhum programa.

use std::sync::Arc;

use crate::session::Session;

/// Sessão que recebe as teclas, ou None para soltar o teclado.
#[cfg(windows)]
pub fn capture(app: &tauri::AppHandle, session: Option<Arc<dyn Session>>) -> bool {
    imp::capture(app, session)
}

#[cfg(not(windows))]
pub fn capture(_app: &tauri::AppHandle, _session: Option<Arc<dyn Session>>) -> bool {
    false
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use std::sync::{Arc, Mutex};

    use tauri::{Emitter, Manager};
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VK_CAPITAL, VK_CONTROL, VK_HOME, VK_LCONTROL, VK_LMENU, VK_MENU, VK_NUMLOCK, VK_PAUSE, VK_RCONTROL,
        VK_RMENU, VK_SCROLL,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetForegroundWindow, GetMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
        HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, LLKHF_INJECTED, LLKHF_UP, MSG, WH_KEYBOARD_LL, WM_QUIT,
    };

    use crate::rdp::Input;
    use crate::session::Session;

    struct State {
        session: Option<Arc<dyn Session>>,
        app: Option<tauri::AppHandle>,
        /// Thread do hook (0 = sem hook).
        thread: u32,
    }

    static STATE: Mutex<State> = Mutex::new(State { session: None, app: None, thread: 0 });
    /// Janela do Nexo: o hook só age com ela em primeiro plano.
    static HWND: AtomicIsize = AtomicIsize::new(0);
    // Modificadores vistos pelo hook: teclas engolidas não atualizam o estado do sistema.
    static CTRL: AtomicBool = AtomicBool::new(false);
    static ALT: AtomicBool = AtomicBool::new(false);

    pub fn capture(app: &tauri::AppHandle, session: Option<Arc<dyn Session>>) -> bool {
        if HWND.load(Ordering::Relaxed) == 0 {
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(hwnd) = window.hwnd() {
                    HWND.store(hwnd.0 as isize, Ordering::Relaxed);
                }
            }
        }
        let mut state = STATE.lock().unwrap();
        let active = session.is_some();
        if let Some(session) = &session {
            // Travas (Caps/Num/Scroll Lock) iguais às daqui ao começar.
            // SAFETY: GetKeyState só lê o estado do teclado da thread.
            let on = |vk| unsafe { GetKeyState(i32::from(vk)) } & 1 != 0;
            if let Some(rdp) = session.as_rdp() {
                rdp.input(vec![Input::Sync { caps: on(VK_CAPITAL), num: on(VK_NUMLOCK), scroll: on(VK_SCROLL) }]);
            }
        } else if let Some(rdp) = state.session.as_ref().and_then(|s| s.as_rdp()) {
            rdp.input(vec![Input::ReleaseAll]);
        }
        state.session = session;
        state.app = Some(app.clone());
        if active && state.thread == 0 {
            CTRL.store(false, Ordering::Relaxed);
            ALT.store(false, Ordering::Relaxed);
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || hook_thread(tx));
            state.thread = rx.recv().unwrap_or(0);
        } else if !active && state.thread != 0 {
            // SAFETY: só posta uma mensagem para a thread do hook, que sai do laço e remove o hook.
            unsafe { PostThreadMessageW(state.thread, WM_QUIT, 0, 0) };
            state.thread = 0;
        }
        active && state.thread != 0
    }

    /// Instala o hook e roda o laço de mensagens que ele exige; sai com WM_QUIT.
    fn hook_thread(ready: std::sync::mpsc::Sender<u32>) {
        // SAFETY: chamadas Win32 sem pré-condições além de rodar nesta thread, que também remove o hook.
        unsafe {
            let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), GetModuleHandleW(std::ptr::null()), 0);
            if hook.is_null() {
                let _ = ready.send(0);
                return;
            }
            let thread = GetCurrentThreadId();
            let _ = ready.send(thread);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
            UnhookWindowsHookEx(hook);
        }
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            // SAFETY: para WH_KEYBOARD_LL com HC_ACTION, lparam aponta para um KBDLLHOOKSTRUCT válido.
            let kb = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
            if handle(kb) {
                return 1;
            }
        }
        // SAFETY: repassa ao próximo hook com os mesmos argumentos.
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    /// true = tecla engolida (foi para o servidor).
    fn handle(kb: &KBDLLHOOKSTRUCT) -> bool {
        // Teclas sintetizadas por outros programas (ex.: AutoHotkey) seguem o caminho normal.
        if kb.flags & LLKHF_INJECTED != 0 {
            return false;
        }
        // SAFETY: sem pré-condições.
        let foreground = unsafe { GetForegroundWindow() } as isize;
        if foreground == 0 || foreground != HWND.load(Ordering::Relaxed) {
            return false;
        }
        let Ok(state) = STATE.try_lock() else { return false };
        let Some(rdp) = state.session.as_ref().and_then(|s| s.as_rdp()) else { return false };
        if rdp.is_closed() {
            return false;
        }

        let down = kb.flags & LLKHF_UP == 0;
        let vk = kb.vkCode as u16;
        match vk {
            VK_CONTROL | VK_LCONTROL | VK_RCONTROL => CTRL.store(down, Ordering::Relaxed),
            VK_MENU | VK_LMENU | VK_RMENU => ALT.store(down, Ordering::Relaxed),
            _ => {}
        }
        if down && vk == VK_HOME && CTRL.load(Ordering::Relaxed) && ALT.load(Ordering::Relaxed) {
            rdp.input(vec![Input::ReleaseAll]);
            if let Some(app) = &state.app {
                let _ = app.emit("rdp-release", ());
            }
            return true;
        }
        // O Ctrl esquerdo falso que o Windows gera junto com o AltGr vem com o bit 0x200 no scancode.
        if kb.scanCode & 0x200 != 0 {
            return true;
        }
        let code = match vk {
            // NumLock chega marcado como estendido, mas no RDP é 0x45 simples.
            VK_NUMLOCK => 0x45,
            // Pause é uma sequência própria (Ctrl+NumLock); sem suporte por ora.
            VK_PAUSE => return true,
            _ if kb.scanCode == 0 => return false,
            _ => (kb.scanCode as u16 & 0xFF) | if kb.flags & LLKHF_EXTENDED != 0 { 0xE000 } else { 0 },
        };
        rdp.input(vec![Input::Key { code, down }]);
        true
    }
}
