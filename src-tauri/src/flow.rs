//! Controle de fluxo entre o backend e o xterm.js.
//!
//! O backend conta os bytes enviados ao frontend; o frontend confirma (ack) depois
//! que o xterm os processou. Se o atraso passa de HIGH, paramos de ler da sessão —
//! o PTY/servidor SSH fica esperando — até o frontend alcançar. Assim um `cat` de
//! vários GB não enche a memória e o Ctrl+C continua respondendo.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

const HIGH_WATER: usize = 2 * 1024 * 1024;
const LOW_WATER: usize = 512 * 1024;

#[derive(Default)]
pub struct Flow {
    pending: AtomicUsize,
    paused: std::sync::atomic::AtomicBool,
    async_wake: tokio::sync::Notify,
    lock: Mutex<()>,
    sync_wake: Condvar,
}

impl Flow {
    pub fn sent(&self, bytes: usize) {
        if self.pending.fetch_add(bytes, Ordering::AcqRel) + bytes > HIGH_WATER {
            self.paused.store(true, Ordering::Release);
        }
    }

    pub fn ack(&self, bytes: usize) {
        let prev = self
            .pending
            .try_update(Ordering::AcqRel, Ordering::Acquire, |p| Some(p.saturating_sub(bytes)))
            .unwrap_or(0);
        if self.is_paused() && prev.saturating_sub(bytes) <= LOW_WATER {
            self.paused.store(false, Ordering::Release);
            self.async_wake.notify_one();
            let _guard = self.lock.lock().unwrap();
            self.sync_wake.notify_all();
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }

    /// Para threads de leitura bloqueantes (PTY local).
    pub fn wait_blocking(&self) {
        let mut guard = self.lock.lock().unwrap();
        while self.is_paused() {
            // Timeout como rede de segurança: nunca dorme para sempre por um aviso perdido.
            guard = self.sync_wake.wait_timeout(guard, Duration::from_millis(200)).unwrap().0;
        }
    }

    /// Para tasks async (SSH).
    pub async fn wait_resumed(&self) {
        while self.is_paused() {
            let _ = tokio::time::timeout(Duration::from_millis(200), self.async_wake.notified()).await;
        }
    }
}
