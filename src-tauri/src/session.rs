//! Abstração de sessão (terminal local ou SSH). O frontend só fala com IDs.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// Sessão de terminal (PTY local, SSH) ou de área de trabalho remota (RDP).
pub trait Session: Send + Sync {
    fn write(&self, data: &[u8]) -> std::io::Result<()>;
    fn resize(&self, cols: u16, rows: u16) -> Result<(), String>;
    fn kill(&self);
    /// O frontend terminou de processar `bytes` (controle de fluxo, ver `flow.rs`).
    fn ack(&self, bytes: usize);
    /// Sessões SSH expõem a conexão (para SFTP); terminais locais não.
    fn as_ssh(&self) -> Option<&crate::ssh::SshSession> {
        None
    }
    fn as_rdp(&self) -> Option<&crate::rdp::RdpSession> {
        None
    }
    /// Terminais locais: o perfil (o painel do host roda comandos nele, se for Linux/WSL).
    fn local_profile(&self) -> Option<&crate::shells::ShellProfile> {
        None
    }
}

#[derive(Default)]
pub struct SessionManager {
    next_id: AtomicU32,
    sessions: Mutex<HashMap<u32, Arc<dyn Session>>>,
}

impl SessionManager {
    pub fn next_id(&self) -> u32 {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn insert(&self, id: u32, session: Arc<dyn Session>) {
        self.sessions.lock().unwrap().insert(id, session);
    }

    pub fn get(&self, id: u32) -> Option<Arc<dyn Session>> {
        self.sessions.lock().unwrap().get(&id).cloned()
    }

    pub fn remove(&self, id: u32) -> Option<Arc<dyn Session>> {
        self.sessions.lock().unwrap().remove(&id)
    }

    /// Escreve o mesmo input em várias sessões (é assim que o broadcast funciona).
    /// Cada `write` só enfileira os bytes na thread da sessão, então uma sessão lenta não atrasa as outras.
    pub fn write_many(&self, ids: &[u32], data: &[u8]) {
        let targets: Vec<_> = {
            let map = self.sessions.lock().unwrap();
            ids.iter().filter_map(|id| map.get(id).cloned()).collect()
        };
        for session in targets {
            let _ = session.write(data);
        }
    }
}
