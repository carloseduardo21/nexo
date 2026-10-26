//! Navegador de arquivos SFTP. Usa um canal extra na conexão SSH do terminal,
//! então não há novo login nem nova verificação de host.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, StatusCode};
use serde::Serialize;
use tauri::ipc::Channel;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const CHUNK: usize = 256 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    name: String,
    path: String,
    /// Diretório (ou link para diretório): dá para entrar.
    is_dir: bool,
    is_link: bool,
    size: u64,
    /// Segundos desde 1970.
    modified: Option<u32>,
    /// Ex.: `rwxr-xr-x`.
    perms: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    done: u64,
    total: u64,
    current: String,
}

/// Mensagem amigável para o usuário.
pub fn describe(e: &SftpError) -> String {
    match e {
        SftpError::Status(s) => match s.status_code {
            StatusCode::NoSuchFile => "arquivo ou pasta não encontrado".into(),
            StatusCode::PermissionDenied => "permissão negada".into(),
            StatusCode::Failure if !s.error_message.is_empty() => s.error_message.clone(),
            StatusCode::Failure => "o servidor recusou a operação".into(),
            code if s.error_message.is_empty() => code.to_string(),
            _ => s.error_message.clone(),
        },
        SftpError::Timeout => "o servidor não respondeu a tempo".into(),
        other => other.to_string(),
    }
}

/// O erro indica que o canal SFTP morreu (e não só que a operação falhou)?
pub fn is_fatal(e: &SftpError) -> bool {
    !matches!(e, SftpError::Status(_) | SftpError::Limited(_))
}

const S_IFMT: u32 = 0o170000;
const S_IFDIR: u32 = 0o040000;
const S_IFLNK: u32 = 0o120000;

fn file_type(attrs: &FileAttributes) -> u32 {
    attrs.permissions.unwrap_or(0) & S_IFMT
}

fn perms_string(mode: u32) -> String {
    let bits = ['r', 'w', 'x'];
    (0..9).map(|i| if mode & (0o400 >> i) != 0 { bits[i % 3] } else { '-' }).collect()
}

pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn base_name(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path)
}

pub async fn list(sftp: &SftpSession, path: &str) -> Result<Vec<Entry>, SftpError> {
    let mut entries = Vec::new();
    for item in sftp.read_dir(path).await? {
        let name = item.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let attrs = item.metadata();
        let full = item.path();
        let is_link = file_type(&attrs) == S_IFLNK;
        // Para links, segue o alvo para saber se é pasta. Link quebrado conta como arquivo.
        let is_dir = if is_link {
            sftp.metadata(full.as_str()).await.is_ok_and(|m| file_type(&m) == S_IFDIR)
        } else {
            file_type(&attrs) == S_IFDIR
        };
        entries.push(Entry {
            name,
            path: full,
            is_dir,
            is_link,
            size: attrs.size.unwrap_or(0),
            modified: attrs.mtime,
            perms: perms_string(attrs.permissions.unwrap_or(0)),
        });
    }
    Ok(entries)
}

/// Apaga arquivos e pastas (recursivamente). Links são apagados, nunca seguidos.
pub async fn remove(sftp: &SftpSession, path: &str) -> Result<(), SftpError> {
    // Pós-ordem iterativa: (caminho, filhos já visitados?).
    let mut stack = vec![(path.to_string(), false)];
    while let Some((path, visited)) = stack.pop() {
        if visited {
            sftp.remove_dir(path).await?;
            continue;
        }
        if file_type(&sftp.symlink_metadata(path.as_str()).await?) != S_IFDIR {
            sftp.remove_file(path).await?;
            continue;
        }
        let children = sftp.read_dir(path.as_str()).await?;
        stack.push((path, true));
        for child in children {
            let name = child.file_name();
            if name != "." && name != ".." {
                stack.push((child.path(), false));
            }
        }
    }
    Ok(())
}

// ---------- Transferências ----------

/// Flags de cancelamento das transferências em andamento, por id (gerado no frontend).
#[derive(Default)]
pub struct Transfers {
    cancel: Mutex<HashMap<u32, Arc<AtomicBool>>>,
}

impl Transfers {
    pub fn start(&self, id: u32) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.cancel.lock().unwrap().insert(id, flag.clone());
        flag
    }

    pub fn finish(&self, id: u32) {
        self.cancel.lock().unwrap().remove(&id);
    }

    pub fn cancel(&self, id: u32) {
        if let Some(flag) = self.cancel.lock().unwrap().get(&id) {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

pub const CANCELED: &str = "cancelado";

struct Job {
    cancel: Arc<AtomicBool>,
    progress: Channel<Progress>,
    done: u64,
    total: u64,
    last: Instant,
}

impl Job {
    fn new(cancel: Arc<AtomicBool>, progress: Channel<Progress>, total: u64) -> Self {
        Self { cancel, progress, done: 0, total, last: Instant::now() - PROGRESS_INTERVAL }
    }

    fn advance(&mut self, bytes: u64, current: &str) -> Result<(), String> {
        self.done += bytes;
        if self.last.elapsed() >= PROGRESS_INTERVAL {
            self.last = Instant::now();
            self.report(current);
        }
        if self.cancel.load(Ordering::Relaxed) {
            return Err(CANCELED.into());
        }
        Ok(())
    }

    fn report(&self, current: &str) {
        let _ = self.progress.send(Progress { done: self.done, total: self.total, current: current.to_string() });
    }
}

/// Um arquivo a copiar: origem, destino e tamanho.
struct Item {
    from: String,
    to: String,
    size: u64,
}

/// Baixa arquivos/pastas remotos para `local_dir`. Devolve os caminhos locais criados (nível de cima).
pub async fn download(
    sftp: &SftpSession,
    remote: &[String],
    local_dir: &Path,
    cancel: Arc<AtomicBool>,
    progress: Channel<Progress>,
) -> Result<Vec<PathBuf>, String> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    let mut roots = Vec::new();

    for path in remote {
        let meta = sftp.metadata(path.as_str()).await.map_err(|e| format!("{path}: {}", describe(&e)))?;
        let root = unique_path(&local_dir.join(base_name(path)));
        if file_type(&meta) != S_IFDIR {
            files.push(Item { from: path.clone(), to: root.display().to_string(), size: meta.size.unwrap_or(0) });
            roots.push(root);
            continue;
        }
        // Percorre a árvore remota (links para pastas não são seguidos, para evitar ciclos).
        let mut pending = vec![(path.clone(), root.clone())];
        while let Some((remote_dir, local)) = pending.pop() {
            for entry in sftp.read_dir(remote_dir.as_str()).await.map_err(|e| format!("{remote_dir}: {}", describe(&e)))? {
                let name = entry.file_name();
                if name == "." || name == ".." {
                    continue;
                }
                let attrs = entry.metadata();
                match file_type(&attrs) {
                    S_IFDIR => pending.push((entry.path(), local.join(&name))),
                    S_IFLNK => {}
                    _ => files.push(Item {
                        from: entry.path(),
                        to: local.join(&name).display().to_string(),
                        size: attrs.size.unwrap_or(0),
                    }),
                }
            }
            dirs.push(local);
        }
        roots.push(root);
    }

    for dir in &dirs {
        tokio::fs::create_dir_all(dir).await.map_err(|e| format!("{}: {e}", dir.display()))?;
    }

    let mut job = Job::new(cancel, progress, files.iter().map(|f| f.size).sum());
    let mut buf = vec![0u8; CHUNK];
    for item in &files {
        if let Err(e) = download_file(sftp, item, &mut job, &mut buf).await {
            let _ = tokio::fs::remove_file(&item.to).await;
            if e == CANCELED {
                for root in &roots {
                    let _ = if root.is_dir() { tokio::fs::remove_dir_all(root).await } else { tokio::fs::remove_file(root).await };
                }
            }
            return Err(e);
        }
    }
    job.report("");
    Ok(roots)
}

async fn download_file(sftp: &SftpSession, item: &Item, job: &mut Job, buf: &mut [u8]) -> Result<(), String> {
    let name = base_name(&item.from);
    let mut src = sftp.open(item.from.as_str()).await.map_err(|e| format!("{}: {}", item.from, describe(&e)))?;
    let mut dst = tokio::fs::File::create(&item.to).await.map_err(|e| format!("{}: {e}", item.to))?;
    loop {
        let n = src.read(buf).await.map_err(|e| format!("{}: {e}", item.from))?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n]).await.map_err(|e| format!("{}: {e}", item.to))?;
        job.advance(n as u64, name)?;
    }
    dst.flush().await.map_err(|e| format!("{}: {e}", item.to))?;
    Ok(())
}

/// Envia arquivos/pastas locais para `remote_dir` (sobrescreve arquivos com o mesmo nome).
pub async fn upload(
    sftp: &SftpSession,
    local: &[PathBuf],
    remote_dir: &str,
    cancel: Arc<AtomicBool>,
    progress: Channel<Progress>,
) -> Result<(), String> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for path in local {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or_else(|| format!("{}: caminho inválido", path.display()))?;
        collect_local(path, join(remote_dir, &name), &mut dirs, &mut files).map_err(|e| format!("{}: {e}", path.display()))?;
    }

    for dir in &dirs {
        // Pasta que já existe não é erro.
        if !sftp.try_exists(dir.as_str()).await.unwrap_or(false) {
            sftp.create_dir(dir.as_str()).await.map_err(|e| format!("{dir}: {}", describe(&e)))?;
        }
    }

    let mut job = Job::new(cancel, progress, files.iter().map(|f| f.size).sum());
    let mut buf = vec![0u8; CHUNK];
    for item in &files {
        upload_file(sftp, item, &mut job, &mut buf).await?;
    }
    job.report("");
    Ok(())
}

fn collect_local(path: &Path, remote: String, dirs: &mut Vec<String>, files: &mut Vec<Item>) -> std::io::Result<()> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_dir() {
        files.push(Item { from: path.display().to_string(), to: remote, size: meta.len() });
        return Ok(());
    }
    dirs.push(remote.clone());
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        collect_local(&entry.path(), join(&remote, &name), dirs, files)?;
    }
    Ok(())
}

async fn upload_file(sftp: &SftpSession, item: &Item, job: &mut Job, buf: &mut [u8]) -> Result<(), String> {
    let name = base_name(&item.to).to_string();
    let mut src = tokio::fs::File::open(&item.from).await.map_err(|e| format!("{}: {e}", item.from))?;
    let mut dst = sftp.create(item.to.as_str()).await.map_err(|e| format!("{}: {}", item.to, describe(&e)))?;
    let result = async {
        loop {
            let n = src.read(buf).await.map_err(|e| format!("{}: {e}", item.from))?;
            if n == 0 {
                break;
            }
            dst.write_all(&buf[..n]).await.map_err(|e| format!("{}: {e}", item.to))?;
            job.advance(n as u64, &name)?;
        }
        // `shutdown` espera as confirmações de escrita pendentes: só aqui sabemos que deu certo.
        dst.shutdown().await.map_err(|e| format!("{}: {e}", item.to))
    }
    .await;
    if result.is_err() {
        let _ = sftp.remove_file(item.to.as_str()).await;
    }
    result
}

/// `arquivo.txt` → `arquivo (1).txt` se já existir.
fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (1..)
        .map(|i| path.with_file_name(format!("{stem} ({i}){ext}")))
        .find(|p| !p.exists())
        .expect("sempre existe um nome livre")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perms() {
        assert_eq!(perms_string(0o100755), "rwxr-xr-x");
        assert_eq!(perms_string(0o040700), "rwx------");
        assert_eq!(perms_string(0o644), "rw-r--r--");
    }

    #[test]
    fn paths() {
        assert_eq!(join("/", "etc"), "/etc");
        assert_eq!(join("/home/u", "a.txt"), "/home/u/a.txt");
        assert_eq!(base_name("/var/log/"), "log");
        assert_eq!(base_name("arquivo"), "arquivo");
    }
}

/// Teste contra um servidor real: `docker run -p 2222:2222 -e USER_NAME=nexo -e USER_PASSWORD=nexo123
/// -e PASSWORD_ACCESS=true lscr.io/linuxserver/openssh-server` e `cargo test -- --ignored live`.
#[cfg(test)]
mod live {
    use super::*;
    use russh::client;

    struct AcceptAll;

    impl client::Handler for AcceptAll {
        type Error = russh::Error;
        async fn check_server_key(&mut self, _: &russh::keys::PublicKeyOrCertificate) -> Result<bool, Self::Error> {
            Ok(true)
        }
    }

    #[tokio::test]
    #[ignore]
    async fn live_download_upload() {
        let mut handle = client::connect(Arc::new(client::Config::default()), ("127.0.0.1", 2222), AcceptAll).await.unwrap();
        assert!(handle.authenticate_password("nexo", "nexo123").await.unwrap().success());
        let channel = handle.channel_open_session().await.unwrap();
        channel.request_subsystem(true, "sftp").await.unwrap();
        let sftp = SftpSession::new(channel.into_stream()).await.unwrap();
        let home = sftp.canonicalize(".").await.unwrap();
        println!("home = {home}");
        for e in list(&sftp, &home).await.unwrap() {
            println!("  {} dir={} {}", e.name, e.is_dir, e.size);
        }

        let out = std::env::temp_dir().join("nexo-sftp-test");
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).unwrap();
        let progress = Channel::new(|_| Ok(()));
        let started = Instant::now();
        let roots = download(&sftp, &[join(&home, "grande.bin"), join(&home, "pasta")], &out, Arc::default(), progress.clone())
            .await
            .unwrap();
        println!("download {:?} em {:?}", roots, started.elapsed());
        assert_eq!(std::fs::metadata(out.join("grande.bin")).unwrap().len(), 5_000_000);
        assert_eq!(std::fs::read_to_string(out.join("pasta/sub/b.txt")).unwrap(), "tchau\n");

        let remote = join(&home, "enviado");
        let _ = remove(&sftp, &remote).await;
        sftp.create_dir(remote.as_str()).await.unwrap();
        upload(&sftp, &[out.join("grande.bin"), out.join("pasta")], &remote, Arc::default(), progress).await.unwrap();
        assert_eq!(sftp.metadata(join(&remote, "grande.bin")).await.unwrap().size, Some(5_000_000));
        remove(&sftp, &remote).await.unwrap();
    }
}
