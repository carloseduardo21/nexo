//! Comandos expostos ao frontend.
//!
//! `session_*` são síncronos de propósito: o Tauri os executa em ordem de chegada,
//! o que garante que as teclas cheguem ao shell na ordem digitada. Eles só enfileiram
//! trabalho (a escrita real acontece na thread/task da sessão), então não seguram a UI.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::SftpSession;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Manager, State};

use crate::docker;
use crate::hostinfo;
use crate::k8s::{self, Kube};
use crate::local;
use crate::rdp;
use crate::session::{Session, SessionManager};
use crate::sftp::{self, Transfers};
use crate::shells::{self, ShellProfile};
use crate::ssh::{self, Failure, Options};
use crate::sshconfig;
use crate::tunnel::{self, Tunnel, Tunnels};
use crate::vault::{Credential, CredentialView, Export, Host, HostView, SecretUpdate, Vault};

#[tauri::command]
pub async fn list_shells() -> Vec<ShellProfile> {
    shells::profiles().to_vec()
}

#[tauri::command]
pub async fn pty_spawn(
    app: AppHandle,
    sessions: State<'_, SessionManager>,
    profile_id: String,
    cols: u16,
    rows: u16,
    on_data: Channel<InvokeResponseBody>,
    on_exit: Channel<Option<u32>>,
) -> Result<u32, String> {
    let profile = shells::find(&profile_id).ok_or_else(|| format!("shell desconhecido: {profile_id}"))?;
    let id = sessions.next_id();
    let pty = local::spawn(app, id, &profile, cols, rows, on_data, on_exit)?;
    sessions.insert(id, pty as Arc<_>);
    Ok(id)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn ssh_connect(
    app: AppHandle,
    sessions: State<'_, SessionManager>,
    vault: State<'_, Vault>,
    host_id: String,
    options: Options,
    cols: u16,
    rows: u16,
    exec: Option<Exec>,
    parent: Option<u32>,
    on_data: Channel<InvokeResponseBody>,
    on_exit: Channel<Option<u32>>,
    on_rtt: Channel<f64>,
) -> Result<u32, Failure> {
    let command = exec.map(|e| e.command()).transpose().map_err(|e| Failure::other(&host_id, e))?;
    // Aberto a partir de outro terminal (ex.: logs de um container): usa a conexão dele, sem
    // novo login. Se ele já fechou, conecta de novo.
    let shared = parent.and_then(|p| sessions.get(p)).and_then(|s| s.as_ssh().and_then(|ssh| ssh.connection()));
    let conn = match shared {
        Some(conn) => conn,
        None => {
            let route = vault.route(&host_id).map_err(|e| Failure::other(&host_id, e))?;
            let conn = Arc::new(ssh::establish(&route, &options).await?);
            remember_secrets(&vault, &options);
            conn
        }
    };
    let id = sessions.next_id();
    let session = ssh::connect(app, id, conn, command.as_deref(), cols, rows, on_data, on_exit, on_rtt)
        .await
        .map_err(|e| Failure::other(&host_id, e))?;
    sessions.insert(id, Arc::new(session));
    Ok(id)
}

/// Terminal que roda um comando em vez do shell.
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Exec {
    DockerLogs {
        container: String,
        #[serde(default)]
        sudo: bool,
    },
    DockerShell {
        container: String,
        #[serde(default)]
        sudo: bool,
    },
}

impl Exec {
    fn command(&self) -> Result<String, String> {
        let (Self::DockerLogs { container, sudo } | Self::DockerShell { container, sudo }) = self;
        if !docker::valid_id(container) {
            return Err("container inválido".into());
        }
        Ok(match self {
            Self::DockerLogs { .. } => docker::logs_command(container, *sudo),
            Self::DockerShell { .. } => docker::shell_command(container, *sudo),
        })
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn rdp_connect(
    app: AppHandle,
    sessions: State<'_, SessionManager>,
    vault: State<'_, Vault>,
    host_id: String,
    options: Options,
    width: u16,
    height: u16,
    on_frame: Channel<InvokeResponseBody>,
    on_exit: Channel<Option<String>>,
) -> Result<u32, Failure> {
    let route = vault.route(&host_id).map_err(|e| Failure::other(&host_id, e))?;
    let host = &route.last().ok_or_else(|| Failure::other(&host_id, "host não encontrado"))?.0;
    let endpoint = format!("{}:{}", host.hostname, host.port);
    let id = sessions.next_id();
    let (session, new_cert) = rdp::connect(app, id, &route, &options, vault.rdp_cert(&endpoint), width, height, on_frame, on_exit).await?;
    sessions.insert(id, Arc::new(session));
    if let Some(fingerprint) = new_cert {
        let _ = vault.trust_rdp_cert(&endpoint, &fingerprint);
    }
    remember_secrets(&vault, &options);
    Ok(id)
}

/// Teclado e mouse de um painel RDP (em lotes: o frontend junta os eventos de cada quadro).
#[tauri::command]
pub fn rdp_input(sessions: State<'_, SessionManager>, id: u32, events: Vec<rdp::Input>) {
    if let Some(session) = sessions.get(id) {
        if let Some(rdp) = session.as_rdp() {
            rdp.input(events);
        }
    }
}

/// Captura do teclado (Win, Alt+Tab, atalhos do Nexo…) para a sessão RDP `id`; None solta.
/// Devolve se a captura está ativa (só no Windows).
#[tauri::command]
pub fn rdp_capture(app: AppHandle, sessions: State<'_, SessionManager>, id: Option<u32>) -> bool {
    let session = id.and_then(|id| sessions.get(id)).filter(|s| s.as_rdp().is_some());
    crate::keyhook::capture(&app, session)
}

/// Senha/passphrase digitada num prompt com "salvar no cofre" marcado: grava depois que deu certo
/// (vale para o destino e para os jump hosts).
fn remember_secrets(vault: &Vault, options: &Options) {
    for (host_id, options) in options {
        if options.remember && (options.password.is_some() || options.passphrase.is_some()) {
            let _ = vault.update_secret(host_id, |s| {
                if options.password.is_some() {
                    s.password = options.password.clone();
                }
                if options.passphrase.is_some() {
                    s.passphrase = options.passphrase.clone();
                }
            });
        }
    }
}

#[tauri::command]
pub fn hosts_list(vault: State<'_, Vault>) -> Vec<HostView> {
    vault.hosts()
}

#[tauri::command]
pub fn host_save(
    vault: State<'_, Vault>,
    host: Host,
    password: Option<String>,
    passphrase: Option<String>,
) -> Result<Host, String> {
    vault.save_host(host, password, passphrase)
}

/// Lê um ssh_config (padrão: ~/.ssh/config) e lista os hosts que dá para importar.
#[tauri::command]
pub fn ssh_config_scan(vault: State<'_, Vault>, path: Option<String>) -> Result<sshconfig::Scan, String> {
    let path = config_path(path)?;
    let existing: Vec<Host> = vault.hosts().into_iter().map(|v| v.host).collect();
    sshconfig::scan(&path, &existing)
}

/// Importa os hosts escolhidos (e os jump hosts de que dependem). Devolve quantos foram criados.
#[tauri::command]
pub fn ssh_config_import(vault: State<'_, Vault>, path: Option<String>, aliases: Vec<String>) -> Result<usize, String> {
    let path = config_path(path)?;
    let existing: Vec<Host> = vault.hosts().into_iter().map(|v| v.host).collect();
    let hosts = sshconfig::to_hosts(&path, &aliases, &existing)?;
    let count = hosts.len();
    vault.add_hosts(hosts)?;
    Ok(count)
}

fn config_path(path: Option<String>) -> Result<PathBuf, String> {
    match path.filter(|p| !p.trim().is_empty()) {
        Some(path) => Ok(PathBuf::from(path)),
        None => sshconfig::default_path().ok_or_else(|| "pasta do usuário não encontrada".into()),
    }
}

#[tauri::command]
pub fn host_delete(vault: State<'_, Vault>, id: String) -> Result<(), String> {
    vault.delete_host(&id)
}

// ---------- Credenciais ----------

#[tauri::command]
pub fn credentials_list(vault: State<'_, Vault>) -> Vec<CredentialView> {
    vault.credentials()
}

#[tauri::command]
pub fn credential_save(vault: State<'_, Vault>, credential: Credential, secrets: SecretUpdate) -> Result<Credential, String> {
    vault.save_credential(credential, secrets)
}

#[tauri::command]
pub fn credential_delete(vault: State<'_, Vault>, id: String) -> Result<(), String> {
    vault.delete_credential(&id)
}

/// Gera um par de chaves ed25519 guardado no cofre, numa credencial nova.
#[tauri::command]
pub fn credential_generate_key(vault: State<'_, Vault>, label: String, username: String) -> Result<CredentialView, String> {
    vault.generate_key_credential(label, username)
}

// ---------- Exportar / importar ----------

/// Grava hosts, credenciais (sem segredos) e túneis num arquivo JSON.
#[tauri::command]
pub fn vault_export(vault: State<'_, Vault>, path: String) -> Result<usize, String> {
    let export = vault.export();
    let json = serde_json::to_vec_pretty(&export).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("{path}: {e}"))?;
    Ok(export.hosts.len())
}

/// Lê um arquivo exportado pelo Nexo e acrescenta o conteúdo. Devolve quantos hosts entraram.
#[tauri::command]
pub fn vault_import(vault: State<'_, Vault>, path: String) -> Result<usize, String> {
    let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    let export: Export = serde_json::from_slice(&bytes).map_err(|e| format!("arquivo inválido: {e}"))?;
    vault.import(export)
}

// ---------- Configurações ----------

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("settings.json"))
}

/// Preferências do app (tema, fonte…): JSON livre, quem conhece o formato é o frontend.
#[tauri::command]
pub fn settings_get(app: AppHandle) -> serde_json::Value {
    settings_path(&app)
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(serde_json::Value::Null)
}

#[tauri::command]
pub fn settings_set(app: AppHandle, settings: serde_json::Value) -> Result<(), String> {
    let path = settings_path(&app)?;
    let json = serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

/// Um único comando para input normal e broadcast: o frontend manda a lista de destinos.
#[tauri::command]
pub fn session_write(sessions: State<'_, SessionManager>, ids: Vec<u32>, data: String) {
    sessions.write_many(&ids, data.as_bytes());
}

/// Digita a senha salva de um host nas sessões (resposta a um prompt de senha, ex.: sudo).
/// Várias sessões quando o broadcast está ligado. A senha vai do cofre direto para as
/// sessões: nunca passa pelo frontend.
#[tauri::command]
pub fn session_send_secret(sessions: State<'_, SessionManager>, vault: State<'_, Vault>, ids: Vec<u32>, host_id: String) -> Result<(), String> {
    let (_, secret) = vault.host(&host_id).ok_or("host não encontrado")?;
    let password = secret.password.ok_or("este host não tem senha salva")?;
    let mut data = password.into_bytes();
    data.push(b'\r');
    sessions.write_many(&ids, &data);
    Ok(())
}

#[tauri::command]
pub fn session_ack(sessions: State<'_, SessionManager>, id: u32, bytes: usize) {
    if let Some(s) = sessions.get(id) {
        s.ack(bytes);
    }
}

#[tauri::command]
pub fn session_resize(sessions: State<'_, SessionManager>, id: u32, cols: u16, rows: u16) -> Result<(), String> {
    match sessions.get(id) {
        Some(s) => s.resize(cols, rows),
        None => Ok(()),
    }
}

#[tauri::command]
pub fn session_kill(sessions: State<'_, SessionManager>, id: u32) {
    if let Some(s) = sessions.get(id) {
        s.kill();
    }
}

// ---------- Painel do host ----------

/// Sessão SSH `id` (para rodar comandos no servidor).
fn ssh_for(sessions: &SessionManager, id: u32) -> Result<Arc<dyn Session>, String> {
    let session = sessions.get(id).ok_or("sessão encerrada")?;
    session.as_ssh().ok_or("não é uma sessão SSH")?;
    Ok(session)
}

#[tauri::command]
pub async fn host_metrics(sessions: State<'_, SessionManager>, id: u32) -> Result<hostinfo::Metrics, String> {
    let session = ssh_for(&sessions, id)?;
    let out = session.as_ssh().unwrap().run(hostinfo::METRICS_SCRIPT).await?;
    match out.status {
        Some(0) => hostinfo::parse(&out.stdout()),
        Some(3) => Err("métricas só funcionam em servidores Linux".into()),
        _ => Err(format!("o servidor não devolveu as métricas: {}", out.stderr())),
    }
}

/// `docker` via sudo: a senha é a digitada agora ou, sem ela, a salva do host (ou da credencial).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sudo {
    host_id: String,
    password: Option<String>,
    /// Salvar a senha digitada no cofre, se o sudo aceitar.
    #[serde(default)]
    remember: bool,
}

/// Roda um script do Docker na sessão `id`, direto ou via sudo.
async fn run_docker(sessions: &SessionManager, vault: &Vault, id: u32, script: &str, sudo: Option<Sudo>) -> Result<String, docker::Error> {
    let session = ssh_for(sessions, id).map_err(docker::Error::other)?;
    let ssh = session.as_ssh().unwrap();
    let typed = sudo.as_ref().and_then(|s| s.password.clone());
    let password = typed.clone().or_else(|| sudo.as_ref().and_then(|s| vault.host(&s.host_id)?.1.password));
    let command = docker::with_sudo(script, sudo.is_some(), password.is_some());
    let input = password.map(|p| format!("{p}\n"));
    let out = ssh.run_with_input(&command, input.as_deref().map(str::as_bytes)).await.map_err(docker::Error::other)?;
    if out.status != Some(0) {
        return Err(docker::describe_error(out.status, &out.stderr()));
    }
    if let (Some(sudo), Some(password)) = (sudo, typed) {
        // Só se o host ainda não tem senha: a salva é a do login SSH, que pode ser outra.
        if sudo.remember && vault.host(&sudo.host_id).is_some_and(|(_, s)| s.password.is_none()) {
            let _ = vault.update_secret(&sudo.host_id, |s| s.password = Some(password));
        }
    }
    Ok(out.stdout())
}

#[tauri::command]
pub async fn docker_list(
    sessions: State<'_, SessionManager>,
    vault: State<'_, Vault>,
    id: u32,
    sudo: Option<Sudo>,
) -> Result<Vec<docker::Container>, docker::Error> {
    let out = run_docker(&sessions, &vault, id, docker::LIST_SCRIPT, sudo).await?;
    Ok(docker::parse_list(&out))
}

#[tauri::command]
pub async fn docker_action(
    sessions: State<'_, SessionManager>,
    vault: State<'_, Vault>,
    id: u32,
    container: String,
    action: docker::Action,
    sudo: Option<Sudo>,
) -> Result<(), docker::Error> {
    if !docker::valid_id(&container) {
        return Err(docker::Error::other("container inválido"));
    }
    run_docker(&sessions, &vault, id, &docker::action_command(&container, action), sudo).await?;
    Ok(())
}

// ---------- SFTP ----------

/// Canal SFTP da sessão SSH `id`.
async fn sftp_for(sessions: &SessionManager, id: u32) -> Result<(Arc<dyn Session>, Arc<SftpSession>), String> {
    let session = sessions.get(id).ok_or("sessão encerrada")?;
    let sftp = session.as_ssh().ok_or("não é uma sessão SSH")?.sftp().await?;
    Ok((session, sftp))
}

/// Converte o erro e, se o canal morreu, descarta-o para o próximo pedido abrir outro.
async fn sftp_err(session: &Arc<dyn Session>, e: SftpError) -> String {
    if sftp::is_fatal(&e) {
        if let Some(ssh) = session.as_ssh() {
            ssh.reset_sftp().await;
        }
    }
    sftp::describe(&e)
}

#[tauri::command]
pub async fn sftp_home(sessions: State<'_, SessionManager>, id: u32) -> Result<String, String> {
    let (session, sftp) = sftp_for(&sessions, id).await?;
    match sftp.canonicalize(".").await {
        Ok(path) => Ok(path),
        Err(e) => Err(sftp_err(&session, e).await),
    }
}

#[tauri::command]
pub async fn sftp_list(sessions: State<'_, SessionManager>, id: u32, path: String) -> Result<Vec<sftp::Entry>, String> {
    let (session, sftp) = sftp_for(&sessions, id).await?;
    match sftp::list(&sftp, &path).await {
        Ok(entries) => Ok(entries),
        Err(e) => Err(sftp_err(&session, e).await),
    }
}

#[tauri::command]
pub async fn sftp_mkdir(sessions: State<'_, SessionManager>, id: u32, path: String) -> Result<(), String> {
    let (session, sftp) = sftp_for(&sessions, id).await?;
    match sftp.create_dir(path).await {
        Ok(()) => Ok(()),
        Err(e) => Err(sftp_err(&session, e).await),
    }
}

#[tauri::command]
pub async fn sftp_rename(sessions: State<'_, SessionManager>, id: u32, from: String, to: String) -> Result<(), String> {
    let (session, sftp) = sftp_for(&sessions, id).await?;
    match sftp.rename(from, to).await {
        Ok(()) => Ok(()),
        Err(e) => Err(sftp_err(&session, e).await),
    }
}

#[tauri::command]
pub async fn sftp_remove(sessions: State<'_, SessionManager>, id: u32, paths: Vec<String>) -> Result<(), String> {
    let (session, sftp) = sftp_for(&sessions, id).await?;
    for path in paths {
        if let Err(e) = sftp::remove(&sftp, &path).await {
            return Err(format!("{path}: {}", sftp_err(&session, e).await));
        }
    }
    Ok(())
}

#[tauri::command]
pub fn download_dir(app: AppHandle) -> Result<String, String> {
    app.path().download_dir().map(|p| p.display().to_string()).map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn sftp_download(
    sessions: State<'_, SessionManager>,
    transfers: State<'_, Transfers>,
    id: u32,
    transfer_id: u32,
    paths: Vec<String>,
    local_dir: String,
    on_progress: Channel<sftp::Progress>,
) -> Result<Vec<String>, String> {
    let (_session, sftp) = sftp_for(&sessions, id).await?;
    let cancel = transfers.start(transfer_id);
    let result = sftp::download(&sftp, &paths, Path::new(&local_dir), cancel, on_progress).await;
    transfers.finish(transfer_id);
    result.map(|roots| roots.iter().map(|p| p.display().to_string()).collect())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn sftp_upload(
    sessions: State<'_, SessionManager>,
    transfers: State<'_, Transfers>,
    id: u32,
    transfer_id: u32,
    paths: Vec<String>,
    remote_dir: String,
    on_progress: Channel<sftp::Progress>,
) -> Result<(), String> {
    let (_session, sftp) = sftp_for(&sessions, id).await?;
    let cancel = transfers.start(transfer_id);
    let local: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    let result = sftp::upload(&sftp, &local, &remote_dir, cancel, on_progress).await;
    transfers.finish(transfer_id);
    result
}

#[tauri::command]
pub fn transfer_cancel(transfers: State<'_, Transfers>, transfer_id: u32) {
    transfers.cancel(transfer_id);
}

// ---------- Túneis ----------

#[tauri::command]
pub fn tunnels_list(vault: State<'_, Vault>) -> Vec<Tunnel> {
    vault.tunnels()
}

#[tauri::command]
pub fn tunnel_save(app: AppHandle, vault: State<'_, Vault>, tunnels: State<'_, Tunnels>, tunnel: Tunnel) -> Result<Tunnel, String> {
    if tunnel.bind_port == 0 && tunnel.kind != tunnel::TunnelKind::Remote {
        return Err("informe a porta local".into());
    }
    if tunnel.kind != tunnel::TunnelKind::Dynamic && (tunnel.dest_host.trim().is_empty() || tunnel.dest_port == 0) {
        return Err("informe o destino (host e porta)".into());
    }
    // Configuração mudou: o túnel em execução para (o frontend reinicia se quiser).
    tunnels.stop(&app, &tunnel.id);
    vault.save_tunnel(tunnel)
}

#[tauri::command]
pub fn tunnel_delete(app: AppHandle, vault: State<'_, Vault>, tunnels: State<'_, Tunnels>, id: String) -> Result<(), String> {
    tunnels.stop(&app, &id);
    vault.delete_tunnel(&id)
}

#[tauri::command]
pub async fn tunnel_start(
    app: AppHandle,
    vault: State<'_, Vault>,
    tunnels: State<'_, Tunnels>,
    id: String,
    options: Options,
) -> Result<(), Failure> {
    let tunnel = vault.tunnel(&id).ok_or_else(|| Failure::other("", "túnel não encontrado"))?;
    let route = vault.route(&tunnel.host_id).map_err(|e| Failure::other(&tunnel.host_id, e))?;
    tunnels.start(app, tunnel, route, options.clone()).await?;
    remember_secrets(&vault, &options);
    Ok(())
}

#[tauri::command]
pub fn tunnel_stop(app: AppHandle, tunnels: State<'_, Tunnels>, id: String) {
    tunnels.stop(&app, &id);
}

#[tauri::command]
pub fn tunnel_statuses(tunnels: State<'_, Tunnels>) -> std::collections::HashMap<String, tunnel::Status> {
    tunnels.statuses()
}

/// `kubeconfig` vazio ou ausente = o padrão (`KUBECONFIG` ou `~/.kube/config`).
fn kubeconfig(path: &Option<String>) -> Option<&str> {
    path.as_deref().filter(|p| !p.trim().is_empty())
}

/// Relê o kubeconfig (e descarta os clientes antigos: contextos e credenciais podem ter mudado).
#[tauri::command]
pub async fn k8s_contexts(kube: State<'_, Kube>, kubeconfig: Option<String>) -> Result<k8s::Contexts, String> {
    kube.forget().await;
    k8s::contexts(self::kubeconfig(&kubeconfig))
}

#[tauri::command]
pub async fn k8s_namespaces(kube: State<'_, Kube>, kubeconfig: Option<String>, context: String) -> Result<Vec<String>, String> {
    let client = kube.client(self::kubeconfig(&kubeconfig), &context).await?;
    k8s::namespaces(&client).await
}

/// `namespace` ausente = todos.
#[tauri::command]
pub async fn k8s_list(
    kube: State<'_, Kube>,
    kubeconfig: Option<String>,
    context: String,
    kind: k8s::Kind,
    namespace: Option<String>,
) -> Result<k8s::Table, String> {
    let client = kube.client(self::kubeconfig(&kubeconfig), &context).await?;
    k8s::list(&client, kind, namespace.as_deref()).await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn k8s_yaml(
    kube: State<'_, Kube>,
    kubeconfig: Option<String>,
    context: String,
    kind: k8s::Kind,
    namespace: Option<String>,
    name: String,
    reveal: bool,
) -> Result<String, String> {
    let client = kube.client(self::kubeconfig(&kubeconfig), &context).await?;
    k8s::yaml(&client, kind, namespace.as_deref(), &name, reveal).await
}
