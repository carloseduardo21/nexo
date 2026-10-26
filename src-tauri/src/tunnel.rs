//! Túneis SSH: local (`-L`), remoto (`-R`) e SOCKS5 dinâmico (`-D`).
//!
//! Cada túnel ativo tem a própria conexão SSH, independente de terminais abertos, e
//! reconecta sozinho (com espera crescente) se ela cair. O estado vai para o frontend
//! pelo evento `tunnel-status`.

use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use crate::ssh::{self, ConnectError, Connection, Failure, Options, Route};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TunnelKind {
    Local,
    Remote,
    Dynamic,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tunnel {
    pub id: String,
    #[serde(default)]
    pub label: String,
    pub host_id: String,
    pub kind: TunnelKind,
    /// Local/dinâmico: onde escutar nesta máquina. Remoto: onde escutar no servidor.
    pub bind_host: String,
    pub bind_port: u16,
    /// Local: destino visto a partir do servidor. Remoto: destino visto desta máquina.
    #[serde(default)]
    pub dest_host: String,
    #[serde(default)]
    pub dest_port: u16,
    /// Inicia junto com o Nexo.
    #[serde(default)]
    pub auto_start: bool,
}

#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Status {
    Connecting,
    Running { connections: usize },
    Reconnecting { attempt: u32, message: String },
    Error { message: String },
}

/// `status: None` = parado.
#[derive(Clone, Serialize)]
struct StatusEvent {
    id: String,
    status: Option<Status>,
}

struct Active {
    /// Distingue execuções do mesmo túnel (parar e iniciar de novo rapidamente).
    generation: u64,
    stop: watch::Sender<bool>,
    status: Status,
}

type ActiveMap = Arc<Mutex<HashMap<String, Active>>>;

#[derive(Default)]
pub struct Tunnels {
    active: ActiveMap,
    generation: AtomicU64,
}

impl Tunnels {
    pub fn statuses(&self) -> HashMap<String, Status> {
        self.active.lock().unwrap().iter().map(|(id, a)| (id.clone(), a.status.clone())).collect()
    }

    /// Conecta e começa a escutar. Erros de conexão voltam tipados (o frontend pede senha,
    /// confirma fingerprint e tenta de novo); depois disso o túnel se mantém sozinho.
    pub async fn start(&self, app: AppHandle, tunnel: Tunnel, route: Route, opts: Options) -> Result<(), Failure> {
        let (stop_tx, stop_rx) = watch::channel(false);
        let generation = self.generation.fetch_add(1, Ordering::Relaxed);
        {
            let mut active = self.active.lock().unwrap();
            if active.get(&tunnel.id).is_some_and(|a| !matches!(a.status, Status::Error { .. })) {
                return Ok(()); // já rodando
            }
            active.insert(tunnel.id.clone(), Active { generation, stop: stop_tx, status: Status::Connecting });
        }
        let reporter = Reporter { app, id: tunnel.id.clone(), generation, active: self.active.clone() };
        reporter.emit(Some(Status::Connecting));

        let setup = async {
            // Escuta antes de conectar: porta ocupada aparece na hora, sem esperar o SSH.
            let listener = match tunnel.kind {
                TunnelKind::Local | TunnelKind::Dynamic => Some(bind(&tunnel).await.map_err(|error| Failure { host_id: tunnel.host_id.clone(), error })?),
                TunnelKind::Remote => None,
            };
            let conn = ssh::establish(&route, &opts).await?;
            Ok::<_, Failure>((listener, conn))
        };
        match setup.await {
            Ok((listener, conn)) => {
                tauri::async_runtime::spawn(run(reporter, tunnel, route, opts, listener, conn, stop_rx));
                Ok(())
            }
            Err(e) => {
                reporter.remove();
                Err(e)
            }
        }
    }

    pub fn stop(&self, app: &AppHandle, id: &str) {
        let removed = self.active.lock().unwrap().remove(id);
        if let Some(active) = removed {
            let _ = active.stop.send(true);
            let _ = app.emit("tunnel-status", StatusEvent { id: id.to_string(), status: None });
        }
    }
}

/// Atualiza o estado de uma execução específica do túnel e avisa o frontend.
#[derive(Clone)]
struct Reporter {
    app: AppHandle,
    id: String,
    generation: u64,
    active: ActiveMap,
}

impl Reporter {
    fn set(&self, status: Status) {
        {
            let mut active = self.active.lock().unwrap();
            match active.get_mut(&self.id) {
                Some(a) if a.generation == self.generation => a.status = status.clone(),
                _ => return, // parado (ou substituído) enquanto isso
            }
        }
        self.emit(Some(status));
    }

    /// Só atualiza o contador se o túnel ainda está no ar (não sobrescreve "reconectando").
    fn connections(&self, count: usize) {
        let running = self
            .active
            .lock()
            .unwrap()
            .get(&self.id)
            .is_some_and(|a| a.generation == self.generation && matches!(a.status, Status::Running { .. }));
        if running {
            self.set(Status::Running { connections: count });
        }
    }

    fn remove(&self) {
        let mut active = self.active.lock().unwrap();
        if active.get(&self.id).is_some_and(|a| a.generation == self.generation) {
            active.remove(&self.id);
            drop(active);
            self.emit(None);
        }
    }

    fn emit(&self, status: Option<Status>) {
        let _ = self.app.emit("tunnel-status", StatusEvent { id: self.id.clone(), status });
    }
}

async fn bind(tunnel: &Tunnel) -> Result<TcpListener, ConnectError> {
    TcpListener::bind((tunnel.bind_host.as_str(), tunnel.bind_port)).await.map_err(|e| ConnectError::Other {
        message: format!("não foi possível escutar em {}:{} — {e}", tunnel.bind_host, tunnel.bind_port),
    })
}

/// Erros que não se resolvem tentando de novo (senha errada, chave do host mudou...).
fn is_permanent(e: &ConnectError) -> bool {
    !matches!(e, ConnectError::Other { .. })
}

fn describe(e: &ConnectError) -> String {
    match e {
        ConnectError::Other { message } | ConnectError::AuthFailed { message } => message.clone(),
        ConnectError::NeedPassword => "o servidor pediu senha".into(),
        ConnectError::NeedPassphrase { .. } => "a chave precisa de passphrase".into(),
        ConnectError::UnknownHost { .. } => "host não confiado".into(),
        ConnectError::HostKeyChanged { .. } => "a chave do servidor mudou".into(),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run(
    rep: Reporter,
    tunnel: Tunnel,
    route: Route,
    opts: Options,
    listener: Option<TcpListener>,
    mut conn: Connection,
    mut stop: watch::Receiver<bool>,
) {
    loop {
        let current = Arc::new(conn);
        if tunnel.kind == TunnelKind::Remote {
            if let Err(message) = forward_remote(&current, &tunnel).await {
                rep.set(Status::Error { message });
                current.close().await;
                return;
            }
        }
        rep.set(Status::Running { connections: 0 });

        if serve(&rep, &tunnel, listener.as_ref(), &current, &mut stop).await {
            current.close().await;
            rep.remove();
            return;
        }

        // A conexão caiu: tenta de novo com espera crescente (2s, 4s… até 30s).
        let mut message = "conexão perdida".to_string();
        let mut attempt = 0;
        conn = loop {
            attempt += 1;
            rep.set(Status::Reconnecting { attempt, message: message.clone() });
            let delay = Duration::from_secs((1u64 << attempt.min(5)).min(30));
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = stop.changed() => {
                    rep.remove();
                    return;
                }
            }
            match ssh::establish(&route, &opts).await {
                Ok(conn) => break conn,
                Err(e) if is_permanent(&e.error) => {
                    rep.set(Status::Error { message: describe(&e.error) });
                    return;
                }
                Err(e) => message = describe(&e.error),
            }
        };
    }
}

async fn forward_remote(conn: &Connection, tunnel: &Tunnel) -> Result<(), String> {
    let port = conn
        .handle
        .tcpip_forward(tunnel.bind_host.clone(), tunnel.bind_port.into())
        .await
        .map_err(|e| format!("o servidor recusou escutar em {}:{} — {e}", tunnel.bind_host, tunnel.bind_port))?;
    // Com porta fixa, o servidor não devolve a porta: vale a pedida.
    let port = if port == 0 { tunnel.bind_port.into() } else { port };
    conn.forwards.lock().unwrap().insert(port, (tunnel.dest_host.clone(), tunnel.dest_port));
    Ok(())
}

/// Atende conexões até pedirem para parar (true) ou a conexão SSH cair (false).
async fn serve(rep: &Reporter, tunnel: &Tunnel, listener: Option<&TcpListener>, conn: &Arc<Connection>, stop: &mut watch::Receiver<bool>) -> bool {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let open = Arc::new(AtomicUsize::new(0));
    loop {
        tokio::select! {
            _ = stop.changed() => return true,
            _ = tick.tick() => if conn.is_closed() { return false },
            accepted = accept(listener) => {
                let Ok((socket, peer)) = accepted else { continue };
                let _ = socket.set_nodelay(true);
                let (conn, rep, open, tunnel) = (conn.clone(), rep.clone(), open.clone(), tunnel.clone());
                rep.connections(open.fetch_add(1, Ordering::Relaxed) + 1);
                tauri::async_runtime::spawn(async move {
                    let result = match tunnel.kind {
                        TunnelKind::Dynamic => socks5(&conn, socket, peer).await,
                        _ => forward_local(&conn, socket, peer, &tunnel.dest_host, tunnel.dest_port).await,
                    };
                    if let Err(e) = result {
                        eprintln!("[nexo] túnel {}: {e}", tunnel.bind_port);
                    }
                    rep.connections(open.fetch_sub(1, Ordering::Relaxed) - 1);
                });
            }
        }
    }
}

async fn accept(listener: Option<&TcpListener>) -> std::io::Result<(TcpStream, SocketAddr)> {
    match listener {
        Some(listener) => listener.accept().await,
        None => std::future::pending().await,
    }
}

async fn forward_local(conn: &Connection, mut socket: TcpStream, peer: SocketAddr, host: &str, port: u16) -> std::io::Result<()> {
    let channel = conn
        .handle
        .channel_open_direct_tcpip(host, port.into(), peer.ip().to_string(), peer.port().into())
        .await
        .map_err(|e| std::io::Error::other(format!("{host}:{port} — {e}")))?;
    let mut stream = channel.into_stream();
    tokio::io::copy_bidirectional(&mut socket, &mut stream).await.map(|_| ())
}

/// Servidor SOCKS5 mínimo (sem autenticação, só CONNECT): cada conexão vira um canal direct-tcpip.
async fn socks5(conn: &Connection, mut socket: TcpStream, peer: SocketAddr) -> std::io::Result<()> {
    let invalid = |msg: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_string());

    let mut head = [0u8; 2];
    socket.read_exact(&mut head).await?;
    if head[0] != 5 {
        return Err(invalid("cliente não é SOCKS5"));
    }
    let mut methods = vec![0u8; head[1].into()];
    socket.read_exact(&mut methods).await?;
    if !methods.contains(&0) {
        socket.write_all(&[5, 0xff]).await?;
        return Err(invalid("cliente SOCKS exige autenticação"));
    }
    socket.write_all(&[5, 0]).await?;

    let mut request = [0u8; 4];
    socket.read_exact(&mut request).await?;
    if request[1] != 1 {
        return socks_reply(&mut socket, 7).await; // só CONNECT
    }
    let host = match request[3] {
        1 => {
            let mut ip = [0u8; 4];
            socket.read_exact(&mut ip).await?;
            Ipv4Addr::from(ip).to_string()
        }
        3 => {
            let mut name = vec![0u8; socket.read_u8().await?.into()];
            socket.read_exact(&mut name).await?;
            String::from_utf8_lossy(&name).into_owned()
        }
        4 => {
            let mut ip = [0u8; 16];
            socket.read_exact(&mut ip).await?;
            Ipv6Addr::from(ip).to_string()
        }
        _ => return socks_reply(&mut socket, 8).await,
    };
    let port = socket.read_u16().await?;

    match conn.handle.channel_open_direct_tcpip(host, port.into(), peer.ip().to_string(), peer.port().into()).await {
        Ok(channel) => {
            socks_reply(&mut socket, 0).await?;
            let mut stream = channel.into_stream();
            tokio::io::copy_bidirectional(&mut socket, &mut stream).await.map(|_| ())
        }
        Err(_) => socks_reply(&mut socket, 5).await, // conexão recusada
    }
}

async fn socks_reply(socket: &mut TcpStream, code: u8) -> std::io::Result<()> {
    socket.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0]).await
}

/// Contra um servidor real (o mesmo container do teste de SFTP, com `AllowTcpForwarding yes`):
/// `cargo test -- --ignored live`.
#[cfg(test)]
mod live {
    use super::*;
    use crate::ssh::ConnectOptions;
    use crate::vault::{AuthMethod, Host, Secret};

    fn host(id: &str) -> (Host, Secret) {
        let host = Host {
            id: id.into(),
            label: "teste".into(),
            hostname: "127.0.0.1".into(),
            port: 2222,
            username: "nexo".into(),
            auth: AuthMethod::Password,
            key_path: String::new(),
            group: String::new(),
            jump_host_id: String::new(),
        };
        (host, Secret { password: Some("nexo123".into()), passphrase: None })
    }

    async fn establish(route: Route) -> Connection {
        // Confia na chave do container (fica gravada no ~/.ssh/known_hosts como [127.0.0.1]:2222).
        let mut opts = Options::new();
        loop {
            match ssh::establish(&route, &opts).await {
                Ok(conn) => return conn,
                Err(Failure { host_id, error: ConnectError::UnknownHost { fingerprint, .. } }) => {
                    opts.insert(host_id, ConnectOptions { trust_fingerprint: Some(fingerprint), ..Default::default() });
                }
                Err(e) => panic!("{}", describe(&e.error)),
            }
        }
    }

    async fn connect() -> Connection {
        establish(vec![host("t")]).await
    }

    /// Jump host: o container salta para ele mesmo (o sshd dele escuta em 127.0.0.1:2222 lá dentro).
    #[tokio::test]
    #[ignore]
    async fn live_jump() {
        let conn = establish(vec![host("jump"), host("alvo")]).await;
        let channel = conn.handle.channel_open_session().await.unwrap();
        channel.exec(true, "echo via-jump").await.unwrap();
        let mut stream = channel.into_stream();
        let mut out = String::new();
        stream.read_to_string(&mut out).await.unwrap();
        assert_eq!(out.trim(), "via-jump");
        assert!(!conn.is_closed());
        conn.close().await;
        println!("jump OK");
    }

    async fn banner(socket: &mut TcpStream) -> String {
        let mut buf = [0u8; 7];
        socket.read_exact(&mut buf).await.unwrap();
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// Escuta numa porta livre e entrega cada conexão aceita para `handler`.
    async fn listen(conn: Arc<Connection>, socks: bool) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (socket, peer) = listener.accept().await.unwrap();
            let result = if socks {
                socks5(&conn, socket, peer).await
            } else {
                // Destino visto pelo servidor: o próprio sshd do container.
                forward_local(&conn, socket, peer, "127.0.0.1", 2222).await
            };
            let _ = result;
        });
        addr
    }

    #[tokio::test]
    #[ignore]
    async fn live_tunnels() {
        let conn = Arc::new(connect().await);

        // -L
        let addr = listen(conn.clone(), false).await;
        let mut socket = TcpStream::connect(addr).await.unwrap();
        assert_eq!(banner(&mut socket).await, "SSH-2.0");
        println!("local ok");

        // -D (SOCKS5 com nome de domínio)
        let addr = listen(conn.clone(), true).await;
        let mut socket = TcpStream::connect(addr).await.unwrap();
        socket.write_all(&[5, 1, 0]).await.unwrap();
        let mut hello = [0u8; 2];
        socket.read_exact(&mut hello).await.unwrap();
        assert_eq!(hello, [5, 0]);
        let mut req = vec![5, 1, 0, 3, 9];
        req.extend_from_slice(b"localhost");
        req.extend_from_slice(&2222u16.to_be_bytes());
        socket.write_all(&req).await.unwrap();
        let mut reply = [0u8; 10];
        socket.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply[1], 0, "SOCKS recusou");
        assert_eq!(banner(&mut socket).await, "SSH-2.0");
        println!("socks ok");

        // -R: servidor escuta em 127.0.0.1:9000 → servidor local de teste.
        let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local_port = local.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = local.accept().await.unwrap();
            s.write_all(b"ola-do-windows").await.unwrap();
        });
        let tunnel = Tunnel {
            id: "r".into(),
            label: String::new(),
            host_id: "t".into(),
            kind: TunnelKind::Remote,
            bind_host: "127.0.0.1".into(),
            bind_port: 9000,
            dest_host: "127.0.0.1".into(),
            dest_port: local_port,
            auto_start: false,
        };
        forward_remote(&conn, &tunnel).await.unwrap();
        // Abre uma conexão para a porta 9000 *do servidor* pelo próprio SSH: ela volta pelo -R.
        let channel = conn.handle.channel_open_direct_tcpip("127.0.0.1", 9000, "127.0.0.1", 1).await.unwrap();
        let mut stream = channel.into_stream();
        let mut got = String::new();
        stream.read_to_string(&mut got).await.unwrap();
        assert_eq!(got, "ola-do-windows");
        println!("remote ok");
    }
}
