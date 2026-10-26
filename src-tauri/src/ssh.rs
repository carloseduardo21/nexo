//! Sessões SSH (russh). Uma task tokio por sessão: lê do canal, escreve o input
//! na ordem em que chegou e respeita o controle de fluxo.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use russh::client::{self, KeyboardInteractiveAuthResponse};
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::agent::AgentIdentity;
use russh::keys::{self, HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{ChannelMsg, Disconnect};
use russh_sftp::client::SftpSession;
use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;

use crate::flow::Flow;
use crate::session::{Session, SessionManager};
use crate::vault::{AuthMethod, Host, Secret};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(12);

/// Erros que o frontend sabe tratar (pedir senha, confirmar fingerprint...).
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ConnectError {
    UnknownHost { fingerprint: String, key_type: String },
    HostKeyChanged { fingerprint: String, key_type: String, line: usize },
    /// Certificado TLS de um servidor RDP diferente do confiado antes.
    CertChanged { fingerprint: String },
    NeedPassword,
    NeedPassphrase { key_path: String },
    AuthFailed { message: String },
    Other { message: String },
}

impl ConnectError {
    pub fn other(e: impl std::fmt::Display) -> Self {
        Self::Other { message: e.to_string() }
    }
}

/// Erro de conexão e o host em que aconteceu (o próprio ou um jump host do caminho).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub host_id: String,
    #[serde(flatten)]
    pub error: ConnectError,
}

impl Failure {
    pub fn other(host_id: &str, message: impl std::fmt::Display) -> Self {
        Self { host_id: host_id.to_string(), error: ConnectError::other(message) }
    }
}

/// Caminho até o servidor: jump hosts na ordem e, por último, o destino.
pub type Route = Vec<(Host, Secret)>;

/// Dados que o usuário informou num prompt ao tentar conectar.
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectOptions {
    pub password: Option<String>,
    pub passphrase: Option<String>,
    /// Fingerprint que o usuário aceitou para um host desconhecido.
    pub trust_fingerprint: Option<String>,
    /// Salvar no cofre a senha/passphrase informada, se a autenticação der certo.
    #[serde(default)]
    pub remember: bool,
}

/// Opções por host (id): o destino e cada jump host podem pedir senha/fingerprint.
pub type Options = HashMap<String, ConnectOptions>;

enum Command {
    Data(Vec<u8>),
    Resize(u16, u16),
    Close,
}

pub struct SshSession {
    commands: mpsc::UnboundedSender<Command>,
    flow: Arc<Flow>,
    conn: Arc<Connection>,
    /// Aberto sob demanda, na primeira vez que o painel de arquivos pede.
    sftp: tokio::sync::Mutex<Option<Arc<SftpSession>>>,
}

impl SshSession {
    /// Canal SFTP na mesma conexão do terminal (sem novo login).
    pub async fn sftp(&self) -> Result<Arc<SftpSession>, String> {
        let mut slot = self.sftp.lock().await;
        if let Some(sftp) = slot.as_ref() {
            return Ok(sftp.clone());
        }
        let channel = self.conn.handle.channel_open_session().await.map_err(|e| format!("não foi possível abrir o canal SFTP: {e}"))?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| format!("o servidor recusou o SFTP: {e}"))?;
        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| format!("o servidor não respondeu ao SFTP: {e}"))?;
        sftp.set_timeout(30);
        let sftp = Arc::new(sftp);
        *slot = Some(sftp.clone());
        Ok(sftp)
    }

    /// Descarta o canal SFTP (ex.: depois de um erro de I/O); o próximo pedido abre outro.
    pub async fn reset_sftp(&self) {
        self.sftp.lock().await.take();
    }

    /// Roda um comando curto num canal extra da mesma conexão do terminal.
    pub async fn run(&self, command: &str) -> Result<Output, String> {
        self.conn.run(command).await
    }

    pub async fn run_with_input(&self, command: &str, input: Option<&[u8]>) -> Result<Output, String> {
        self.conn.run_with_input(command, input).await
    }

    /// A conexão do terminal, para abrir outro terminal nela (`connect`) sem novo login.
    pub fn connection(&self) -> Option<Arc<Connection>> {
        (!self.conn.is_closed()).then(|| self.conn.share())
    }
}

const RUN_TIMEOUT: Duration = Duration::from_secs(20);

/// Resultado de `SshSession::run`.
#[derive(Default)]
pub struct Output {
    pub status: Option<u32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Output {
    pub fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.stderr).trim().to_string()
    }
}

impl Session for SshSession {
    fn write(&self, data: &[u8]) -> std::io::Result<()> {
        self.commands
            .send(Command::Data(data.to_vec()))
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        self.commands.send(Command::Resize(cols, rows)).map_err(|e| e.to_string())
    }

    fn kill(&self) {
        let _ = self.commands.send(Command::Close);
    }

    fn ack(&self, bytes: usize) {
        self.flow.ack(bytes);
    }

    fn as_ssh(&self) -> Option<&SshSession> {
        Some(self)
    }
}

/// Encaminhamentos remotos (`-R`) ativos na conexão: porta no servidor → destino local.
pub type Forwards = Arc<Mutex<HashMap<u32, (String, u16)>>>;

pub struct Client {
    host: String,
    port: u16,
    trust: Option<String>,
    problem: Arc<Mutex<Option<ConnectError>>>,
    forwards: Forwards,
}

/// Uma conexão SSH autenticada (sem shell): base para terminal, SFTP e túneis.
pub struct Connection {
    pub handle: client::Handle<Client>,
    pub forwards: Forwards,
    /// Conexões com os jump hosts do caminho; precisam viver enquanto esta viver.
    jumps: Vec<client::Handle<Client>>,
    /// Terminais usando a conexão (o do usuário e os abertos a partir dele, ex.: logs de um
    /// container). A conexão fecha quando o último sai (`release`).
    users: AtomicUsize,
}

impl Connection {
    /// Mais um terminal na mesma conexão.
    pub fn share(self: &Arc<Self>) -> Arc<Self> {
        self.users.fetch_add(1, Ordering::AcqRel);
        self.clone()
    }

    /// Um terminal saiu; o último fecha a conexão.
    pub async fn release(&self) {
        if self.users.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.close().await;
        }
    }

    /// Encerra a conexão e, depois, os jump hosts (do mais próximo ao mais distante).
    pub async fn close(&self) {
        let _ = self.handle.disconnect(Disconnect::ByApplication, "", "pt-BR").await;
        for jump in self.jumps.iter().rev() {
            let _ = jump.disconnect(Disconnect::ByApplication, "", "pt-BR").await;
        }
    }

    /// Roda um comando curto num canal extra (sem PTY) e junta a saída.
    pub async fn run(&self, command: &str) -> Result<Output, String> {
        self.run_with_input(command, None).await
    }

    /// Como `run`, mandando `input` na entrada do comando (ex.: a senha para `sudo -S`).
    pub async fn run_with_input(&self, command: &str, input: Option<&[u8]>) -> Result<Output, String> {
        let collect = async {
            let mut channel = self.handle.channel_open_session().await.map_err(|e| format!("não foi possível abrir um canal: {e}"))?;
            channel.exec(true, command).await.map_err(|e| format!("o servidor recusou o comando: {e}"))?;
            if let Some(input) = input {
                channel.data(input).await.map_err(|e| format!("não foi possível enviar a entrada: {e}"))?;
            }
            channel.eof().await.map_err(|e| format!("não foi possível enviar a entrada: {e}"))?;
            let mut out = Output::default();
            while let Some(msg) = channel.wait().await {
                match msg {
                    ChannelMsg::Data { data } => out.stdout.extend_from_slice(&data),
                    ChannelMsg::ExtendedData { data, .. } => out.stderr.extend_from_slice(&data),
                    ChannelMsg::ExitStatus { exit_status } => out.status = Some(exit_status),
                    ChannelMsg::Close => break,
                    _ => {}
                }
            }
            Ok(out)
        };
        tokio::time::timeout(RUN_TIMEOUT, collect).await.map_err(|_| "o servidor não respondeu a tempo".to_string())?
    }

    /// Caiu a conexão com o destino ou com algum jump host.
    pub fn is_closed(&self) -> bool {
        self.handle.is_closed() || self.jumps.iter().any(|j| j.is_closed())
    }
}

impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(&mut self, server_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = server_key else {
            *self.problem.lock().unwrap() = Some(ConnectError::other("certificados de host ainda não são suportados"));
            return Ok(false);
        };
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
        let key_type = key.algorithm().as_str().to_string();

        // Usa o mesmo ~/.ssh/known_hosts do OpenSSH: hosts já confiados no `ssh` continuam confiados aqui.
        match keys::check_known_hosts(&self.host, self.port, key) {
            Ok(true) => Ok(true),
            Err(keys::Error::KeyChanged { line }) => {
                *self.problem.lock().unwrap() = Some(ConnectError::HostKeyChanged { fingerprint, key_type, line });
                Ok(false)
            }
            Ok(false) | Err(_) if self.trust.as_deref() == Some(fingerprint.as_str()) => {
                if let Err(e) = keys::known_hosts::learn_known_hosts(&self.host, self.port, key) {
                    eprintln!("[nexo] não foi possível gravar em known_hosts: {e}");
                }
                Ok(true)
            }
            Ok(false) | Err(_) => {
                *self.problem.lock().unwrap() = Some(ConnectError::UnknownHost { fingerprint, key_type });
                Ok(false)
            }
        }
    }

    /// Conexão chegando num encaminhamento remoto (`-R`): liga ao destino local.
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<client::Msg>,
        _connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let target = self.forwards.lock().unwrap().get(&connected_port).cloned();
        // Sem encaminhamento nosso nessa porta: soltar o `reply` recusa o canal.
        let Some((host, port)) = target else { return Ok(()) };
        reply.accept().await;
        tauri::async_runtime::spawn(async move {
            match tokio::net::TcpStream::connect((host.as_str(), port)).await {
                Ok(mut socket) => {
                    let _ = socket.set_nodelay(true);
                    let mut stream = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
                }
                Err(e) => {
                    eprintln!("[nexo] túnel remoto: {host}:{port} — {e}");
                    let _ = channel.close().await;
                }
            }
        });
        Ok(())
    }
}

/// Abre um terminal na conexão (que passa a ser deste terminal também; ver `Connection::share`).
/// Sem `command`, o shell do usuário; com, roda o comando num PTY (ex.: `docker logs -f`).
#[allow(clippy::too_many_arguments)]
pub async fn connect(
    app: AppHandle,
    id: u32,
    conn: Arc<Connection>,
    command: Option<&str>,
    cols: u16,
    rows: u16,
    on_data: Channel<InvokeResponseBody>,
    on_exit: Channel<Option<u32>>,
    on_rtt: Channel<f64>,
) -> Result<SshSession, String> {
    let channel = match open_shell(&conn.handle, command, cols, rows).await {
        Ok(channel) => channel,
        Err(e) => {
            conn.release().await;
            return Err(e.to_string());
        }
    };

    let (commands, mut command_rx) = mpsc::unbounded_channel();
    let flow = Arc::new(Flow::default());
    let task_flow = flow.clone();
    let task_conn = conn.clone();

    tauri::async_runtime::spawn(async move {
        let mut channel = channel;
        let mut exit_code = None;
        let mut rtt = RttProbe::default();
        loop {
            tokio::select! {
                // `biased`: input do usuário tem prioridade sobre ler mais saída.
                biased;
                cmd = command_rx.recv() => match cmd {
                    Some(Command::Data(data)) => {
                        rtt.sent();
                        if channel.data_bytes(data).await.is_err() { break; }
                    }
                    Some(Command::Resize(cols, rows)) => {
                        let _ = channel.window_change(cols.into(), rows.into(), 0, 0).await;
                    }
                    Some(Command::Close) | None => {
                        let _ = channel.close().await;
                        break;
                    }
                },
                _ = task_flow.wait_resumed(), if task_flow.is_paused() => {}
                msg = channel.wait(), if !task_flow.is_paused() => match msg {
                    Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                        if let Some(ms) = rtt.received() {
                            let _ = on_rtt.send(ms);
                        }
                        task_flow.sent(data.len());
                        if on_data.send(InvokeResponseBody::Raw(data.to_vec())).is_err() { break; }
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => exit_code = Some(exit_status),
                    Some(ChannelMsg::Close) | None => break,
                    _ => {}
                },
            }
        }
        app.state::<SessionManager>().remove(id);
        task_conn.release().await;
        let _ = on_exit.send(exit_code);
    });

    Ok(SshSession { commands, flow, conn, sftp: Default::default() })
}

async fn open_shell(
    handle: &client::Handle<Client>,
    command: Option<&str>,
    cols: u16,
    rows: u16,
) -> Result<russh::Channel<client::Msg>, russh::Error> {
    let channel = handle.channel_open_session().await?;
    channel.request_pty(false, "xterm-256color", cols.into(), rows.into(), 0, 0, &[]).await?;
    match command {
        Some(command) => channel.exec(false, command).await?,
        None => channel.request_shell(false).await?,
    }
    Ok(channel)
}

/// Conecta e autentica, passando pelos jump hosts do caminho (cada um abre um túnel
/// `direct-tcpip` até o próximo). Erros que pedem ação do usuário vêm tipados, com o host.
pub async fn establish(route: &[(Host, Secret)], opts: &Options) -> Result<Connection, Failure> {
    let none = ConnectOptions::default();
    let mut jumps: Vec<client::Handle<Client>> = Vec::new();
    for (i, (host, secret)) in route.iter().enumerate() {
        let host_opts = opts.get(&host.id).unwrap_or(&none);
        let attempt = async {
            match jumps.last() {
                None => {
                    let socket = tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::TcpStream::connect((host.hostname.as_str(), host.port)))
                        .await
                        .map_err(|_| ConnectError::other(format!("tempo esgotado conectando em {}:{}", host.hostname, host.port)))?
                        .map_err(|e| ConnectError::other(format!("{}:{} — {e}", host.hostname, host.port)))?;
                    // Sem Nagle: cada tecla sai imediatamente, sem esperar juntar pacote.
                    let _ = socket.set_nodelay(true);
                    handshake(socket, host, secret, host_opts).await
                }
                Some(jump) => {
                    let channel = tokio::time::timeout(
                        CONNECT_TIMEOUT,
                        jump.channel_open_direct_tcpip(host.hostname.clone(), host.port.into(), "127.0.0.1", 0),
                    )
                    .await
                    .map_err(|_| ConnectError::other(format!("tempo esgotado: o jump host não alcançou {}:{}", host.hostname, host.port)))?
                    .map_err(|e| ConnectError::other(format!("o jump host não alcançou {}:{} — {e}", host.hostname, host.port)))?;
                    handshake(channel.into_stream(), host, secret, host_opts).await
                }
            }
        };
        match attempt.await {
            Ok((handle, forwards)) if i + 1 == route.len() => return Ok(Connection { handle, forwards, jumps, users: AtomicUsize::new(1) }),
            Ok((handle, _)) => jumps.push(handle),
            Err(error) => {
                for jump in jumps.iter().rev() {
                    let _ = jump.disconnect(Disconnect::ByApplication, "", "pt-BR").await;
                }
                return Err(Failure { host_id: host.id.clone(), error });
            }
        }
    }
    Err(Failure::other("", "nenhum host para conectar"))
}

/// Handshake SSH + autenticação sobre um stream já aberto (TCP ou canal de um jump host).
async fn handshake<S>(stream: S, host: &Host, secret: &Secret, opts: &ConnectOptions) -> Result<(client::Handle<Client>, Forwards), ConnectError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let config = Arc::new(client::Config {
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        nodelay: true,
        ..Default::default()
    });
    let problem = Arc::new(Mutex::new(None));
    let forwards = Forwards::default();
    let handler = Client {
        host: host.hostname.clone(),
        port: host.port,
        trust: opts.trust_fingerprint.clone(),
        problem: problem.clone(),
        forwards: forwards.clone(),
    };
    let mut handle = match tokio::time::timeout(CONNECT_TIMEOUT, client::connect_stream(config, stream, handler)).await {
        Ok(Ok(handle)) => handle,
        Ok(Err(e)) => return Err(problem.lock().unwrap().take().unwrap_or_else(|| ConnectError::other(e))),
        Err(_) => return Err(ConnectError::other(format!("{}:{} não respondeu ao handshake SSH", host.hostname, host.port))),
    };
    if let Err(e) = authenticate(&mut handle, host, secret, opts).await {
        let _ = handle.disconnect(Disconnect::ByApplication, "", "pt-BR").await;
        return Err(e);
    }
    Ok((handle, forwards))
}

/// Mede o tempo entre enviar input e receber a próxima saída (eco): rede + servidor.
/// Reporta a mediana das últimas amostras, no máximo duas vezes por segundo.
#[derive(Default)]
struct RttProbe {
    pending: Option<Instant>,
    samples: Vec<f64>,
    last_report: Option<Instant>,
}

impl RttProbe {
    fn sent(&mut self) {
        self.pending.get_or_insert_with(Instant::now);
    }

    fn received(&mut self) -> Option<f64> {
        let elapsed = self.pending.take()?.elapsed();
        // Input sem eco (senha, comando demorado) gera amostras enormes que não são latência.
        if elapsed > Duration::from_secs(1) {
            return None;
        }
        if self.samples.len() == 15 {
            self.samples.remove(0);
        }
        self.samples.push(elapsed.as_secs_f64() * 1000.0);
        if self.last_report.is_some_and(|t| t.elapsed() < Duration::from_millis(500)) {
            return None;
        }
        self.last_report = Some(Instant::now());
        let mut sorted = self.samples.clone();
        sorted.sort_by(f64::total_cmp);
        Some(sorted[sorted.len() / 2])
    }
}

async fn authenticate(
    handle: &mut client::Handle<Client>,
    host: &Host,
    secret: &Secret,
    opts: &ConnectOptions,
) -> Result<(), ConnectError> {
    let user = host.username.as_str();
    match host.auth {
        AuthMethod::Password => {
            let password = opts.password.clone().or_else(|| secret.password.clone()).ok_or(ConnectError::NeedPassword)?;
            let result = handle.authenticate_password(user, password.clone()).await.map_err(ConnectError::other)?;
            if result.success() {
                return Ok(());
            }
            // Muitos servidores só aceitam senha via keyboard-interactive.
            if keyboard_interactive(handle, user, &password).await? {
                return Ok(());
            }
            Err(ConnectError::AuthFailed { message: format!("senha recusada para {user}@{}", host.hostname) })
        }
        AuthMethod::Agent => {
            let rsa_hash = handle.best_supported_rsa_hash().await.ok().flatten().flatten();
            match with_agent(handle, user, rsa_hash).await? {
                Some(true) => Ok(()),
                Some(false) => Err(ConnectError::AuthFailed { message: format!("nenhuma chave do agente foi aceita por {user}@{}", host.hostname) }),
                None => Err(ConnectError::other(NO_AGENT)),
            }
        }
        AuthMethod::Key => {
            let passphrase = opts.passphrase.clone().or_else(|| secret.passphrase.clone());
            let rsa_hash = handle.best_supported_rsa_hash().await.ok().flatten().flatten();
            // Chave guardada no cofre (importada ou gerada pelo Nexo): só ela é tentada.
            if let Some(pem) = &secret.private_key {
                const VAULT_KEY: &str = "chave guardada no cofre";
                let key = match keys::decode_secret_key(pem.trim(), passphrase.as_deref()) {
                    Ok(key) => key,
                    Err(keys::Error::KeyIsEncrypted) => return Err(ConnectError::NeedPassphrase { key_path: VAULT_KEY.into() }),
                    Err(e) if passphrase.is_some() && is_bad_passphrase(&e) => {
                        return Err(ConnectError::NeedPassphrase { key_path: VAULT_KEY.into() });
                    }
                    Err(e) => return Err(ConnectError::other(format!("{VAULT_KEY}: {e}"))),
                };
                let key = PrivateKeyWithHashAlg::new(Arc::new(key), rsa_hash);
                if handle.authenticate_publickey(user, key).await.map_err(ConnectError::other)?.success() {
                    return Ok(());
                }
                return Err(ConnectError::AuthFailed { message: format!("chave recusada para {user}@{}", host.hostname) });
            }
            let explicit = !host.key_path.trim().is_empty();
            let paths = if explicit { vec![expand_home(&host.key_path)] } else { default_keys() };
            for path in &paths {
                let key = match keys::load_secret_key(path, passphrase.as_deref()) {
                    Ok(key) => key,
                    Err(keys::Error::KeyIsEncrypted) => {
                        return Err(ConnectError::NeedPassphrase { key_path: path.display().to_string() });
                    }
                    Err(e) if passphrase.is_some() && is_bad_passphrase(&e) => {
                        return Err(ConnectError::NeedPassphrase { key_path: path.display().to_string() });
                    }
                    Err(e) => return Err(ConnectError::other(format!("{}: {e}", path.display()))),
                };
                let key = PrivateKeyWithHashAlg::new(Arc::new(key), rsa_hash);
                let result = handle.authenticate_publickey(user, key).await.map_err(ConnectError::other)?;
                if result.success() {
                    return Ok(());
                }
            }
            // Sem arquivo de chave definido: como o OpenSSH, tenta também as chaves do agente.
            if !explicit {
                match with_agent(handle, user, rsa_hash).await? {
                    Some(true) => return Ok(()),
                    None if paths.is_empty() => {
                        return Err(ConnectError::other(format!("nenhuma chave em ~/.ssh (id_ed25519, id_ecdsa, id_rsa) e {NO_AGENT}")));
                    }
                    _ => {}
                }
            }
            Err(ConnectError::AuthFailed { message: format!("chave recusada para {user}@{}", host.hostname) })
        }
    }
}

const NO_AGENT: &str = if cfg!(windows) {
    "nenhum agente SSH rodando (serviço OpenSSH Authentication Agent ou Pageant)"
} else {
    "nenhum agente SSH rodando (SSH_AUTH_SOCK)"
};

type Agent = AgentClient<Box<dyn AgentStream + Send + Unpin>>;

/// Agente SSH do sistema: no Windows, o serviço do OpenSSH e depois o Pageant.
async fn connect_agent() -> Option<Agent> {
    #[cfg(windows)]
    {
        if let Ok(agent) = AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent").await {
            return Some(agent.dynamic());
        }
        if let Ok(agent) = AgentClient::connect_pageant().await {
            return Some(agent.dynamic());
        }
        None
    }
    #[cfg(unix)]
    {
        AgentClient::connect_env().await.ok().map(|a| a.dynamic())
    }
}

/// Tenta cada chave do agente. None = não há agente; Some(aceito?).
async fn with_agent(handle: &mut client::Handle<Client>, user: &str, rsa_hash: Option<HashAlg>) -> Result<Option<bool>, ConnectError> {
    let Some(mut agent) = connect_agent().await else { return Ok(None) };
    let identities = agent.request_identities().await.map_err(|e| ConnectError::other(format!("agente SSH: {e}")))?;
    for identity in identities {
        let result = match identity {
            AgentIdentity::PublicKey { key, .. } => {
                let hash = key.algorithm().is_rsa().then_some(rsa_hash).flatten();
                handle.authenticate_publickey_with(user, key, hash, &mut agent).await
            }
            AgentIdentity::Certificate { certificate, .. } => {
                let hash = certificate.algorithm().is_rsa().then_some(rsa_hash).flatten();
                handle.authenticate_certificate_with(user, certificate, hash, &mut agent).await
            }
        };
        if result.map_err(|e| ConnectError::other(format!("agente SSH: {e}")))?.success() {
            return Ok(Some(true));
        }
    }
    Ok(Some(false))
}

async fn keyboard_interactive(handle: &mut client::Handle<Client>, user: &str, password: &str) -> Result<bool, ConnectError> {
    let mut reply = handle.authenticate_keyboard_interactive_start(user, None).await.map_err(ConnectError::other)?;
    // Limite de rodadas para não ficar preso num servidor que pede infinitamente.
    for _ in 0..5 {
        match reply {
            KeyboardInteractiveAuthResponse::Success => return Ok(true),
            KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
            KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                let answers = prompts.iter().map(|p| if p.echo { String::new() } else { password.to_string() }).collect();
                reply = handle.authenticate_keyboard_interactive_respond(answers).await.map_err(ConnectError::other)?;
            }
        }
    }
    Ok(false)
}

fn is_bad_passphrase(e: &keys::Error) -> bool {
    let msg = e.to_string().to_lowercase();
    msg.contains("decrypt") || msg.contains("passphrase") || msg.contains("crypto")
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")), home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

fn default_keys() -> Vec<PathBuf> {
    let Some(ssh) = home_dir().map(|h| h.join(".ssh")) else { return vec![] };
    ["id_ed25519", "id_ecdsa", "id_rsa"].iter().map(|n| ssh.join(n)).filter(|p| p.is_file()).collect()
}


/// Contra o container openssh dos outros testes (porta 2222 ou `NEXO_SSH_PORT`):
/// `cargo test -- --ignored live_vault_key`.
#[cfg(test)]
mod live {
    use super::*;

    /// Terminal aberto a partir de outro (logs de container) divide a conexão: ela só fecha
    /// quando o último sai.
    #[tokio::test]
    #[ignore]
    async fn live_shared_connection() {
        let conn = Arc::new(crate::hostinfo::live::connect("NEXO_SSH_PORT", 2222).await);
        let second = conn.share();
        conn.release().await;
        assert!(!second.is_closed());
        assert_eq!(second.run("echo ok").await.unwrap().stdout().trim(), "ok");
        second.release().await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(second.is_closed());
    }

    /// Chave gerada e guardada no cofre (sem arquivo): a pública vai para o `authorized_keys` do container.
    #[tokio::test]
    #[ignore]
    async fn live_vault_key() {
        let seed: [u8; 32] = std::array::from_fn(|i| (i as u8).wrapping_mul(97).wrapping_add(13));
        let key = keys::PrivateKey::from(keys::ssh_key::private::Ed25519Keypair::from_seed(&seed));
        let public = key.public_key().to_openssh().unwrap();
        let script = format!("grep -qF '{public}' /config/.ssh/authorized_keys 2>/dev/null || echo '{public}' >> /config/.ssh/authorized_keys");
        let ok = std::process::Command::new("wsl").args(["docker", "exec", "nexo-ssh", "sh", "-c", &script]).status().unwrap();
        assert!(ok.success(), "não foi possível instalar a chave no container");

        let host = Host {
            id: "k".into(),
            hostname: "127.0.0.1".into(),
            port: std::env::var("NEXO_SSH_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(2222),
            username: "nexo".into(),
            auth: AuthMethod::Key,
            ..Default::default()
        };
        let secret = Secret { private_key: Some(key.to_openssh(keys::ssh_key::LineEnding::LF).unwrap().to_string()), ..Default::default() };
        let route = vec![(host, secret)];
        let mut opts = Options::new();
        let conn = loop {
            match establish(&route, &opts).await {
                Ok(conn) => break conn,
                Err(Failure { host_id, error: ConnectError::UnknownHost { fingerprint, .. } }) => {
                    opts.insert(host_id, ConnectOptions { trust_fingerprint: Some(fingerprint), ..Default::default() });
                }
                Err(e) => panic!("{:?}", e.error),
            }
        };
        conn.close().await;
    }
}
