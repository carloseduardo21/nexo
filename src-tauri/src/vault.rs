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

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuthMethod {
    #[default]
    Password,
    Key,
    /// Chaves do agente SSH do sistema (OpenSSH Agent, Pageant, SSH_AUTH_SOCK).
    Agent,
}

#[derive(Clone, Copy, Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Protocol {
    #[default]
    Ssh,
    Rdp,
}

/// Opções de hosts RDP (usuário e senha são os campos comuns do host).
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RdpSettings {
    pub domain: String,
    /// Resolução fixa; 0×0 = acompanha o tamanho do painel.
    pub width: u16,
    pub height: u16,
    /// Resolução em pixels físicos da tela (nítido em telas com escala; a fonte remota fica menor).
    pub hidpi: bool,
    /// Área de transferência compartilhada com o servidor (texto).
    pub clipboard: bool,
    /// Atalhos do sistema (Win, Alt+Tab…) e do Nexo vão para o servidor enquanto o painel tem foco.
    pub capture_keys: bool,
    /// Pipeline gráfico (GFX/H.264): mais fluido; desligar volta aos bitmaps (RemoteFX).
    pub gfx: bool,
}

impl Default for RdpSettings {
    fn default() -> Self {
        Self { domain: String::new(), width: 0, height: 0, hidpi: false, clipboard: true, capture_keys: true, gfx: true }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
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
    /// Em hosts RDP, é o gateway SSH por onde a conexão passa.
    #[serde(default)]
    pub jump_host_id: String,
    #[serde(default)]
    pub protocol: Protocol,
    #[serde(default)]
    pub rdp: RdpSettings,
    /// Credencial salva usada pelo host (usuário, método e segredos vêm dela); vazio = dados do próprio host.
    #[serde(default)]
    pub credential_id: String,
}

/// Credencial reutilizável (como as "identities" do Termius): vários hosts, SSH ou RDP, usam a mesma.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Credential {
    pub id: String,
    pub label: String,
    pub username: String,
    pub auth: AuthMethod,
    /// Arquivo da chave privada; vazio com chave importada (texto no cofre) ou para tentar as padrão.
    pub key_path: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Secret {
    pub password: Option<String>,
    pub passphrase: Option<String>,
    /// Chave privada (formato OpenSSH) guardada no próprio cofre: importada ou gerada pelo Nexo.
    #[serde(default)]
    pub private_key: Option<String>,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialView {
    #[serde(flatten)]
    pub credential: Credential,
    pub has_password: bool,
    pub has_passphrase: bool,
    /// Chave privada guardada no cofre.
    pub has_key: bool,
    /// Chave pública (OpenSSH) da chave guardada, para copiar para o `authorized_keys`.
    pub public_key: Option<String>,
    /// Hosts que usam esta credencial.
    pub used_by: Vec<String>,
}

/// Segredos novos de um formulário: None mantém o salvo, Some("") apaga, Some(x) troca.
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SecretUpdate {
    pub password: Option<String>,
    pub passphrase: Option<String>,
    pub private_key: Option<String>,
}

impl SecretUpdate {
    fn apply(self, secret: &mut Secret) {
        let set = |slot: &mut Option<String>, value: Option<String>| {
            if let Some(v) = value {
                *slot = (!v.is_empty()).then_some(v);
            }
        };
        set(&mut secret.password, self.password);
        set(&mut secret.passphrase, self.passphrase);
        set(&mut secret.private_key, self.private_key);
    }
}

/// Arquivo de exportação: tudo menos os segredos (senhas e chaves ficam só no cofre desta máquina).
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Export {
    pub version: u32,
    pub hosts: Vec<Host>,
    pub credentials: Vec<Credential>,
    pub tunnels: Vec<Tunnel>,
}

impl Default for Export {
    fn default() -> Self {
        Self { version: 1, hosts: Vec::new(), credentials: Vec::new(), tunnels: Vec::new() }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct VaultData {
    hosts: Vec<Host>,
    /// Segredos por id de host ou de credencial (uuids: não colidem).
    secrets: HashMap<String, Secret>,
    #[serde(default)]
    credentials: Vec<Credential>,
    #[serde(default)]
    tunnels: Vec<Tunnel>,
    /// Certificados TLS de servidores RDP confiados: "host:porta" → SHA-256 (como o known_hosts do SSH).
    #[serde(default)]
    rdp_certs: HashMap<String, String>,
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
                let (_, secret) = effective(&data, h);
                HostView { host: h.clone(), has_password: secret.password.is_some(), has_passphrase: secret.passphrase.is_some() }
            })
            .collect()
    }

    /// O host com usuário, método e segredos já resolvidos (da credencial, se tiver uma).
    pub fn host(&self, id: &str) -> Option<(Host, Secret)> {
        let data = self.data.lock().unwrap();
        let host = data.hosts.iter().find(|h| h.id == id)?;
        Some(effective(&data, host))
    }

    // ---------- Credenciais ----------

    pub fn credentials(&self) -> Vec<CredentialView> {
        let data = self.data.lock().unwrap();
        data.credentials
            .iter()
            .map(|c| {
                let secret = data.secrets.get(&c.id);
                let key = secret.and_then(|s| s.private_key.as_deref());
                CredentialView {
                    credential: c.clone(),
                    has_password: secret.is_some_and(|s| s.password.is_some()),
                    has_passphrase: secret.is_some_and(|s| s.passphrase.is_some()),
                    has_key: key.is_some(),
                    public_key: key.and_then(public_key_of),
                    used_by: data.hosts.iter().filter(|h| h.credential_id == c.id).map(|h| h.label.clone()).collect(),
                }
            })
            .collect()
    }

    pub fn save_credential(&self, mut credential: Credential, secrets: SecretUpdate) -> Result<Credential, String> {
        if let Some(key) = secrets.private_key.as_deref().filter(|k| !k.is_empty()) {
            // Confere o formato já ao salvar (a passphrase só é exigida ao conectar).
            russh::keys::PrivateKey::from_openssh(key.trim()).map_err(|e| format!("chave privada inválida: {e}"))?;
        }
        let mut data = self.data.lock().unwrap();
        if credential.id.is_empty() {
            credential.id = uuid::Uuid::new_v4().to_string();
        }
        match data.credentials.iter_mut().find(|c| c.id == credential.id) {
            Some(existing) => *existing = credential.clone(),
            None => data.credentials.push(credential.clone()),
        }
        secrets.apply(data.secrets.entry(credential.id.clone()).or_default());
        self.persist(&data)?;
        Ok(credential)
    }

    /// Exclui a credencial. Os hosts que a usavam ficam com o usuário dela e passam a pedir a senha ao conectar.
    pub fn delete_credential(&self, id: &str) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        let Some(credential) = data.credentials.iter().find(|c| c.id == id).cloned() else { return Ok(()) };
        for host in data.hosts.iter_mut().filter(|h| h.credential_id == id) {
            host.credential_id.clear();
            if host.username.is_empty() {
                host.username = credential.username.clone();
            }
            if host.protocol == Protocol::Ssh {
                host.auth = if credential.auth == AuthMethod::Key && credential.key_path.is_empty() { AuthMethod::Password } else { credential.auth.clone() };
                host.key_path = credential.key_path.clone();
            }
        }
        data.credentials.retain(|c| c.id != id);
        data.secrets.remove(id);
        self.persist(&data)
    }

    /// Gera um par de chaves ed25519 numa credencial nova; devolve a credencial (com a chave pública).
    pub fn generate_key_credential(&self, label: String, username: String) -> Result<CredentialView, String> {
        let seed: [u8; 32] = new_key().try_into().map_err(|_| "falha ao gerar chave")?;
        let keypair = russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&seed);
        let mut key = russh::keys::PrivateKey::from(keypair);
        key.set_comment(format!("{username}@nexo"));
        let pem = key.to_openssh(russh::keys::ssh_key::LineEnding::LF).map_err(|e| e.to_string())?;
        let credential = Credential { label, username, auth: AuthMethod::Key, ..Default::default() };
        let saved = self.save_credential(credential, SecretUpdate { private_key: Some(pem.to_string()), ..Default::default() })?;
        self.credentials().into_iter().find(|c| c.credential.id == saved.id).ok_or_else(|| "credencial não encontrada".into())
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
        if !host.credential_id.is_empty() && !data.credentials.iter().any(|c| c.id == host.credential_id) {
            return Err("a credencial escolhida não existe mais".into());
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

    /// Atualiza o segredo de um host; se ele usa uma credencial, o segredo é o da credencial.
    pub fn update_secret(&self, id: &str, update: impl FnOnce(&mut Secret)) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        let owner = data
            .hosts
            .iter()
            .find(|h| h.id == id && !h.credential_id.is_empty() && data.credentials.iter().any(|c| c.id == h.credential_id))
            .map_or_else(|| id.to_string(), |h| h.credential_id.clone());
        update(data.secrets.entry(owner).or_default());
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

    /// Hosts, credenciais (sem segredos) e túneis, para exportar.
    pub fn export(&self) -> Export {
        let data = self.data.lock().unwrap();
        Export { version: 1, hosts: data.hosts.clone(), credentials: data.credentials.clone(), tunnels: data.tunnels.clone() }
    }

    /// Importa um arquivo exportado. Ids repetidos ganham ids novos (e as referências entre hosts,
    /// credenciais e túneis acompanham). Devolve quantos hosts entraram.
    pub fn import(&self, export: Export) -> Result<usize, String> {
        let mut data = self.data.lock().unwrap();
        let mut remap: HashMap<String, String> = HashMap::new();
        let taken = |data: &VaultData, id: &str| {
            data.hosts.iter().any(|h| h.id == id) || data.credentials.iter().any(|c| c.id == id) || data.tunnels.iter().any(|t| t.id == id)
        };
        let mut fresh = |data: &VaultData, id: &str| -> String {
            let new = if id.is_empty() || taken(data, id) { uuid::Uuid::new_v4().to_string() } else { id.to_string() };
            remap.insert(id.to_string(), new.clone());
            new
        };
        let mut credentials = export.credentials;
        for c in &mut credentials {
            c.id = fresh(&data, &c.id);
        }
        let mut hosts = export.hosts;
        for h in &mut hosts {
            h.id = fresh(&data, &h.id);
        }
        let mut tunnels = export.tunnels;
        for t in &mut tunnels {
            t.id = fresh(&data, &t.id);
        }
        let map = |id: &str| if id.is_empty() { String::new() } else { remap.get(id).cloned().unwrap_or_else(|| id.to_string()) };
        for h in &mut hosts {
            h.jump_host_id = map(&h.jump_host_id);
            h.credential_id = map(&h.credential_id);
        }
        for t in &mut tunnels {
            t.host_id = map(&t.host_id);
        }
        let count = hosts.len();
        data.credentials.extend(credentials);
        data.hosts.extend(hosts);
        data.tunnels.extend(tunnels.into_iter().filter(|t| !t.host_id.is_empty()));
        // Referências que não existem (arquivo editado à mão) não podem quebrar o cofre.
        let host_ids: Vec<String> = data.hosts.iter().map(|h| h.id.clone()).collect();
        let credential_ids: Vec<String> = data.credentials.iter().map(|c| c.id.clone()).collect();
        for h in &mut data.hosts {
            if !h.jump_host_id.is_empty() && !host_ids.contains(&h.jump_host_id) {
                h.jump_host_id.clear();
            }
            if !h.credential_id.is_empty() && !credential_ids.contains(&h.credential_id) {
                h.credential_id.clear();
            }
        }
        data.tunnels.retain(|t| host_ids.contains(&t.host_id));
        self.persist(&data)?;
        Ok(count)
    }

    pub fn rdp_cert(&self, endpoint: &str) -> Option<String> {
        self.data.lock().unwrap().rdp_certs.get(endpoint).cloned()
    }

    pub fn trust_rdp_cert(&self, endpoint: &str, fingerprint: &str) -> Result<(), String> {
        let mut data = self.data.lock().unwrap();
        data.rdp_certs.insert(endpoint.to_string(), fingerprint.to_string());
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
        match data.hosts.iter().find(|h| h.id == tunnel.host_id) {
            None => return Err("host não encontrado".into()),
            Some(h) if h.protocol != Protocol::Ssh => return Err("túneis precisam de um host SSH".into()),
            Some(_) => {}
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
        if !route.is_empty() && host.protocol != Protocol::Ssh {
            return Err(format!("o jump host \"{}\" não é um host SSH", host.label));
        }
        route.push(effective(data, host));
        if host.jump_host_id.is_empty() {
            break;
        }
        next = &host.jump_host_id;
    }
    route.reverse();
    Ok(route)
}

/// Host pronto para conectar: com credencial, o usuário, o método, a chave e os segredos vêm dela
/// (o id continua o do host, para os erros e os prompts apontarem o host certo).
fn effective(data: &VaultData, host: &Host) -> (Host, Secret) {
    let credential = (!host.credential_id.is_empty()).then(|| data.credentials.iter().find(|c| c.id == host.credential_id)).flatten();
    match credential {
        Some(c) => {
            let mut host = host.clone();
            if !c.username.is_empty() {
                host.username = c.username.clone();
            }
            host.auth = c.auth.clone();
            host.key_path = c.key_path.clone();
            (host, data.secrets.get(&c.id).cloned().unwrap_or_default())
        }
        None => (host.clone(), data.secrets.get(&host.id).cloned().unwrap_or_default()),
    }
}

/// Chave pública (linha do `authorized_keys`) de uma chave privada OpenSSH.
fn public_key_of(private_key: &str) -> Option<String> {
    let key = russh::keys::PrivateKey::from_openssh(private_key.trim()).ok()?;
    key.public_key().to_openssh().ok()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Cofre num diretório temporário, com chave aleatória (sem tocar no keyring do sistema).
    fn vault() -> Vault {
        let dir = std::env::temp_dir().join(format!("nexo-vault-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cipher = XChaCha20Poly1305::new_from_slice(&new_key()).unwrap();
        Vault { path: dir.join("vault.bin"), cipher, data: Mutex::new(VaultData::default()) }
    }

    fn host(label: &str) -> Host {
        Host { label: label.into(), hostname: format!("{label}.lan"), port: 22, username: "local".into(), ..Default::default() }
    }

    #[test]
    fn hosts_use_the_credential() {
        let v = vault();
        let cred = v
            .save_credential(
                Credential { label: "prod".into(), username: "deploy".into(), auth: AuthMethod::Password, ..Default::default() },
                SecretUpdate { password: Some("s3nha".into()), ..Default::default() },
            )
            .unwrap();
        let web = v.save_host(Host { credential_id: cred.id.clone(), ..host("web") }, None, None).unwrap();
        let rdp = v.save_host(Host { credential_id: cred.id.clone(), protocol: Protocol::Rdp, port: 3389, ..host("desk") }, None, None).unwrap();

        for id in [&web.id, &rdp.id] {
            let (h, s) = v.host(id).unwrap();
            assert_eq!(h.username, "deploy");
            assert_eq!(s.password.as_deref(), Some("s3nha"));
            assert_eq!(&h.id, id, "o id continua o do host");
        }
        assert!(v.hosts().iter().all(|h| h.has_password));
        assert_eq!(v.credentials()[0].used_by.len(), 2);

        // Senha lembrada num prompt vai para a credencial (vale para todos os hosts dela).
        v.update_secret(&web.id, |s| s.password = Some("nova".into())).unwrap();
        assert_eq!(v.host(&rdp.id).unwrap().1.password.as_deref(), Some("nova"));
    }

    #[test]
    fn deleting_a_credential_keeps_the_hosts() {
        let v = vault();
        let cred = v
            .save_credential(Credential { label: "c".into(), username: "ana".into(), ..Default::default() }, SecretUpdate::default())
            .unwrap();
        let h = v.save_host(Host { credential_id: cred.id.clone(), username: String::new(), ..host("a") }, None, None).unwrap();
        v.delete_credential(&cred.id).unwrap();
        let (host, secret) = v.host(&h.id).unwrap();
        assert!(host.credential_id.is_empty());
        assert_eq!(host.username, "ana");
        assert!(secret.password.is_none());
        assert!(v.credentials().is_empty());
    }

    #[test]
    fn generated_key_is_usable() {
        let v = vault();
        let view = v.generate_key_credential("minha chave".into(), "ana".into()).unwrap();
        assert!(view.has_key);
        assert!(view.public_key.as_deref().unwrap().starts_with("ssh-ed25519 "));
        let (_, secret) = {
            let h = v.save_host(Host { credential_id: view.credential.id.clone(), ..host("k") }, None, None).unwrap();
            v.host(&h.id).unwrap()
        };
        russh::keys::decode_secret_key(secret.private_key.as_deref().unwrap(), None).unwrap();
        assert!(v.save_credential(Credential::default(), SecretUpdate { private_key: Some("lixo".into()), ..Default::default() }).is_err());
    }

    #[test]
    fn import_remaps_ids() {
        let v = vault();
        let cred = v.save_credential(Credential { label: "c".into(), ..Default::default() }, SecretUpdate::default()).unwrap();
        let jump = v.save_host(host("jump"), None, None).unwrap();
        v.save_host(Host { jump_host_id: jump.id.clone(), credential_id: cred.id.clone(), ..host("inner") }, None, None).unwrap();

        // Importar o próprio export duplica tudo com ids novos, mantendo as ligações.
        let count = v.import(v.export()).unwrap();
        assert_eq!(count, 2);
        let hosts = v.hosts();
        assert_eq!(hosts.len(), 4);
        let inner: Vec<_> = hosts.iter().filter(|h| h.host.label == "inner").collect();
        assert_ne!(inner[0].host.jump_host_id, inner[1].host.jump_host_id);
        assert_ne!(inner[0].host.credential_id, inner[1].host.credential_id);
        for h in inner {
            assert!(v.route(&h.host.id).is_ok());
            assert!(v.credentials().iter().any(|c| c.credential.id == h.host.credential_id));
        }
    }
}
