//! Cofre local: hosts e credenciais num único arquivo criptografado.
//!
//! O arquivo usa XChaCha20-Poly1305 com uma chave aleatória de 256 bits guardada no
//! cofre do sistema operacional (Gerenciador de Credenciais no Windows, Keychain no
//! macOS, Secret Service no Linux). Sem keyring disponível, a chave fica num arquivo
//! ao lado do cofre — menos seguro, mas o app continua funcionando.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chacha20poly1305::aead::{Aead, Generate, Key, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};

use crate::ssh::Route;
use crate::tunnel::Tunnel;

const MAGIC: &[u8] = b"NEXOV1";
const KEYRING_SERVICE: &str = "Nexo";
const KEYRING_USER: &str = "vault-key";
/// Limite de saltos (jump hosts) num caminho; também pega cadeias circulares.
const MAX_HOPS: usize = 8;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuthMethod {
    Password,
    Key,
    /// Chaves do agente SSH do sistema (OpenSSH Agent, Pageant, SSH_AUTH_SOCK).
    Agent,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub id: String,
    pub label: String,
    pub hostname: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
    /// Vazio = tenta as chaves padrão (~/.ssh/id_ed25519, id_ecdsa, id_rsa).
    #[serde(default)]
    pub key_path: String,
    #[serde(default)]
    pub group: String,
    /// Host salvo usado como jump host (ProxyJump); vazio = conexão direta.
    #[serde(default)]
    pub jump_host_id: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Secret {
    pub password: Option<String>,
    pub passphrase: Option<String>,
}

/// O que o frontend recebe: o host, mais se há segredo salvo (nunca o segredo em si).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostView {
    #[serde(flatten)]
    pub host: Host,
    pub has_password: bool,
    pub has_passphrase: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct VaultData {
    hosts: Vec<Host>,
    secrets: HashMap<String, Secret>,
    #[serde(default)]
    tunnels: Vec<Tunnel>,
}

pub struct Vault {
    path: PathBuf,
    cipher: XChaCha20Poly1305,
    data: Mutex<VaultData>,
}

impl Vault {
    pub fn open(dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let key = load_or_create_key(dir)?;
        let cipher = XChaCha20Poly1305::new_from_slice(&key).map_err(|e| e.to_string())?;
        let path = dir.join("vault.bin");
        let data = match std::fs::read(&path) {
            Ok(bytes) => decrypt(&cipher, &bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => VaultData::default(),
            Err(e) => return Err(e.to_string()),
        };
        Ok(Self { path, cipher, data: Mutex::new(data) })
    }

    pub fn hosts(&self) -> Vec<HostView> {
        let data = self.data.lock().unwrap();
        data.hosts
            .iter()
            .map(|h| {
                let secret = data.secrets.get(&h.id);
                HostView {
                    host: h.clone(),
                    has_password: secret.is_some_and(|s| s.password.is_some()),
                    has_passphrase: secret.is_some_and(|s| s.passphrase.is_some()),
                }
            })
            .collect()
    }

    pub fn host(&self, id: &str) -> Option<(Host, Secret)> {
        let data = self.data.lock().unwrap();
        let host = data.hosts.iter().find(|h| h.id == id)?.clone();
        Some((host, data.secrets.get(id).cloned().unwrap_or_default()))
    }

    /// Caminho até o host: os jump hosts (do primeiro ao último) e o próprio host.
    pub fn route(&self, id: &str) -> Result<Route, String> {
        route_in(&self.data.lock().unwrap(), id)
    }

    /// `password`/`passphrase`: None mantém o valor salvo, Some("") apaga, Some(x) troca.
    pub fn save_host(&self, mut host: Host, password: Option<String>, passphrase: Option<String>) -> Result<Host, String> {
        let mut data = self.data.lock().unwrap();
        if host.id.is_empty() {
            host.id = uuid::Uuid::new_v4().to_string();
        }
        let previous = match data.hosts.iter_mut().find(|h| h.id == host.id) {
            Some(existing) => Some(std::mem::replace(existing, host.clone())),
            None => {
                data.hosts.push(host.clone());
                None
            }
        };
        // Jump host inexistente ou que cria um ciclo: desfaz e recusa.
        if let Err(e) = route_in(&data, &host.id) {
            match previous {
                Some(previous) => *data.hosts.iter_mut().find(|h| h.id == host.id).unwrap() = previous,
                None => data.hosts.retain(|h| h.id != host.id),
            }
            return Err(e);
        }
        let secret = data.secrets.entry(host.id.clone()).or_default();
        if let Some(p) = password {
            secret.password = (!p.is_empty()).then_some(p);
        }
        if let Some(p) = passphrase {
            secret.passphrase = (!p.is_empty()).then_some(p);
        }
        self.persist(&data)?;
        Ok(host)
    }

    pub fn update_secret(&self, id: &str, update: impl FnOnce(&mut Secret)) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        update(data.secrets.entry(id.to_string()).or_default());
        self.persist(&data)
    }

    /// Acrescenta hosts novos de uma vez (importação).
    pub fn add_hosts(&self, hosts: Vec<Host>) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        data.hosts.extend(hosts);
        self.persist(&data)
    }

    pub fn delete_host(&self, id: &str) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        let dependents: Vec<&str> = data.hosts.iter().filter(|h| h.jump_host_id == id).map(|h| h.label.as_str()).collect();
        if !dependents.is_empty() {
            return Err(format!("este host é o jump host de: {}. Troque o jump host deles antes de excluir.", dependents.join(", ")));
        }
        data.hosts.retain(|h| h.id != id);
        data.secrets.remove(id);
        data.tunnels.retain(|t| t.host_id != id);
        self.persist(&data)
    }

    pub fn tunnels(&self) -> Vec<Tunnel> {
        self.data.lock().unwrap().tunnels.clone()
    }

    pub fn tunnel(&self, id: &str) -> Option<Tunnel> {
        self.data.lock().unwrap().tunnels.iter().find(|t| t.id == id).cloned()
    }

    pub fn save_tunnel(&self, mut tunnel: Tunnel) -> Result<Tunnel, String> {
        let mut data = self.data.lock().unwrap();
        if !data.hosts.iter().any(|h| h.id == tunnel.host_id) {
            return Err("host não encontrado".into());
        }
        if tunnel.id.is_empty() {
            tunnel.id = uuid::Uuid::new_v4().to_string();
        }
        match data.tunnels.iter_mut().find(|t| t.id == tunnel.id) {
            Some(existing) => *existing = tunnel.clone(),
            None => data.tunnels.push(tunnel.clone()),
        }
        self.persist(&data)?;
        Ok(tunnel)
    }

    pub fn delete_tunnel(&self, id: &str) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        data.tunnels.retain(|t| t.id != id);
        self.persist(&data)
    }

    fn persist(&self, data: &VaultData) -> Result<(), String> {
        let plain = serde_json::to_vec(data).map_err(|e| e.to_string())?;
        let nonce = XNonce::generate();
        let sealed = self.cipher.encrypt(&nonce, plain.as_ref()).map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(MAGIC.len() + nonce.len() + sealed.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&sealed);
        // Escreve num temporário e renomeia: uma queda no meio nunca corrompe o cofre.
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, out).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }
}

fn route_in(data: &VaultData, id: &str) -> Result<Route, String> {
    let mut route = Vec::new();
    let mut next = id;
    loop {
        if route.len() == MAX_HOPS {
            return Err("o caminho de jump hosts é circular (ou longo demais)".into());
        }
        let host = data.hosts.iter().find(|h| h.id == next).ok_or_else(|| {
            if route.is_empty() { "host não encontrado".to_string() } else { "o jump host configurado não existe mais".to_string() }
        })?;
        route.push((host.clone(), data.secrets.get(&host.id).cloned().unwrap_or_default()));
        if host.jump_host_id.is_empty() {
            break;
        }
        next = &host.jump_host_id;
    }
    route.reverse();
    Ok(route)
}

fn decrypt(cipher: &XChaCha20Poly1305, bytes: &[u8]) -> Result<VaultData, String> {
    let body = bytes.strip_prefix(MAGIC).ok_or("cofre em formato desconhecido")?;
    if body.len() < 24 {
        return Err("cofre corrompido".into());
    }
    let (nonce, sealed) = body.split_at(24);
    let nonce = XNonce::try_from(nonce).map_err(|_| "cofre corrompido")?;
    let plain = cipher
        .decrypt(&nonce, sealed)
        .map_err(|_| "não foi possível abrir o cofre: chave incorreta ou arquivo corrompido")?;
    serde_json::from_slice(&plain).map_err(|e| e.to_string())
}

fn load_or_create_key(dir: &Path) -> Result<Vec<u8>, String> {
    let key_file = dir.join("vault.key");
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        match entry.get_secret() {
            Ok(key) if key.len() == 32 => return Ok(key),
            Ok(_) | Err(keyring::Error::NoEntry) => {
                // Migração: se havia uma chave em arquivo (keyring indisponível antes), reaproveita.
                let key = std::fs::read(&key_file).ok().filter(|k| k.len() == 32).unwrap_or_else(new_key);
                if entry.set_secret(&key).is_ok() {
                    let _ = std::fs::remove_file(&key_file);
                    return Ok(key);
                }
            }
            Err(_) => {}
        }
    }
    // Sem keyring do sistema.
    if let Ok(key) = std::fs::read(&key_file) {
        if key.len() == 32 {
            return Ok(key);
        }
    }
    let key = new_key();
    std::fs::write(&key_file, &key).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&key_file, std::fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}

fn new_key() -> Vec<u8> {
    Key::<XChaCha20Poly1305>::generate().to_vec()
}
