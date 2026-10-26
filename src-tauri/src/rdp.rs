//! Sessões RDP (IronRDP). A tela é decodificada aqui; o frontend recebe só as regiões que
//! mudaram, em binário, e devolve um ack por mensagem desenhada (controle de fluxo: com a
//! webview atrasada, as regiões se acumulam e saem juntas na próxima mensagem).
//!
//! A conexão pode passar por jump hosts SSH (canal `direct-tcpip` no último deles), o caso
//! comum de xRDP atrás de um bastion.

use std::sync::Arc;
use std::time::Duration;

use ironrdp_connector::connection_activation::ConnectionActivationState;
use ironrdp_connector::sspi::generator::NetworkRequest;
use ironrdp_connector::{ClientConnector, ConnectionResult, ConnectorErrorKind, ConnectorResult, Credentials, DesktopSize};
use ironrdp_core::WriteBuf;
use ironrdp_displaycontrol::client::DisplayControlClient;
use ironrdp_displaycontrol::pdu::MonitorLayoutEntry;
use ironrdp_egfx::client::{GraphicsPipelineClient, GraphicsPipelineHandler};
use ironrdp_egfx::decode::{H264Decoder, OpenH264Decoder};
use ironrdp_graphics::image_processing::PixelFormat;
use ironrdp_graphics::pointer::DecodedPointer;
use ironrdp_input::{Database, MouseButton, MousePosition, Operation, Scancode, WheelRotations};
use ironrdp_pdu::geometry::{InclusiveRectangle, Rectangle as _};
use ironrdp_pdu::gcc::KeyboardType;
use ironrdp_pdu::mcs::{DisconnectProviderUltimatum, DisconnectReason, McsMessage};
use ironrdp_pdu::x224::X224;
use ironrdp_pdu::rdp::capability_sets::{client_codecs_capabilities, MajorPlatformType, RailSupportLevel};
use ironrdp_session::image::DecodedImage;
use ironrdp_session::{ActiveStage, ActiveStageBuilder, ActiveStageOutput, GracefulDisconnectReason};
use ironrdp_tokio::{single_sequence_step_read, split_tokio_framed, FramedWrite as _, TokioFramed};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

use crate::rdp_clipboard::{self, Clipboard};
use crate::session::{Session, SessionManager};
use crate::ssh::{self, ConnectError, ConnectOptions, Connection, Failure, Options, Route};
use crate::vault::{Host, Secret};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Mensagens de tela enviadas e ainda não desenhadas pelo frontend.
const MAX_IN_FLIGHT: usize = 2;
/// Acima disso, as regiões pendentes viram um único retângulo envolvente.
const MAX_DIRTY: usize = 64;

// Mensagens binárias para o frontend (inteiros u16 little-endian).
/// `[1, n, n × (x, y, w, h, RGBA…)]`
const MSG_RECTS: u8 = 1;
/// `[2, w, h]`: tamanho da área de trabalho (início, reconexão, reativação).
const MSG_SIZE: u8 = 2;
/// `[3, w, h, hotspot x, hotspot y, RGBA…]`
const MSG_POINTER: u8 = 3;
const MSG_POINTER_DEFAULT: u8 = 4;
const MSG_POINTER_HIDDEN: u8 = 5;

trait Stream: AsyncRead + AsyncWrite + Unpin + Send + Sync {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + Sync> Stream for T {}
type Framed = TokioFramed<Box<dyn Stream>>;

/// Evento de input do frontend. Teclas vão por scancode: o layout é o do servidor.
#[derive(Debug, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum Input {
    Key { code: u16, down: bool },
    /// Caractere sem scancode conhecido (ex.: colar texto, teclas de IME).
    Unicode { ch: char, down: bool },
    Move { x: u16, y: u16 },
    /// Índice de botão do DOM (`MouseEvent.button`): 0 esquerdo, 1 meio, 2 direito, 3/4 laterais.
    Button { button: u8, down: bool },
    Wheel { delta: i16, vertical: bool },
    /// Estado das teclas de trava (ao ganhar o foco).
    Sync { caps: bool, num: bool, scroll: bool },
    /// Solta tudo que estiver pressionado (ao perder o foco: evita tecla "presa" no servidor).
    ReleaseAll,
    /// O painel ganhou o foco: anuncia ao servidor o texto copiado aqui, se mudou.
    ClipboardSync,
}

enum Command {
    Input(Vec<Input>),
    Resize(u16, u16),
    Ack,
    Close,
}

pub struct RdpSession {
    commands: mpsc::UnboundedSender<Command>,
}

impl RdpSession {
    pub fn input(&self, events: Vec<Input>) {
        let _ = self.commands.send(Command::Input(events));
    }

    /// A sessão terminou (o laço dela não recebe mais comandos).
    pub fn is_closed(&self) -> bool {
        self.commands.is_closed()
    }
}

impl Session for RdpSession {
    fn write(&self, _data: &[u8]) -> std::io::Result<()> {
        // Broadcast de texto não se aplica a uma área de trabalho remota.
        Ok(())
    }

    /// Em RDP, o tamanho é em pixels.
    fn resize(&self, width: u16, height: u16) -> Result<(), String> {
        self.commands.send(Command::Resize(width, height)).map_err(|e| e.to_string())
    }

    fn kill(&self) {
        let _ = self.commands.send(Command::Close);
    }

    fn ack(&self, _bytes: usize) {
        let _ = self.commands.send(Command::Ack);
    }

    fn as_rdp(&self) -> Option<&RdpSession> {
        Some(self)
    }
}

/// Tudo o que é preciso para (re)abrir a conexão: reconectar com outro tamanho de tela
/// reaproveita a conexão SSH com o gateway, se houver.
struct Target {
    host: Host,
    password: String,
    /// Fingerprint do certificado aceito (salvo no cofre ou confirmado agora).
    cert: Option<String>,
    trust: Option<String>,
    gateway: Option<Arc<Connection>>,
    /// Eventos do canal de área de transferência (o backend é recriado a cada conexão).
    clipboard: mpsc::UnboundedSender<rdp_clipboard::Event>,
}

/// Conecta e autentica. Devolve a sessão e, se o usuário acabou de confiar no certificado
/// do servidor, o fingerprint para gravar no cofre.
#[allow(clippy::too_many_arguments)]
pub async fn connect(
    app: AppHandle,
    id: u32,
    route: &Route,
    opts: &Options,
    trusted_cert: Option<String>,
    width: u16,
    height: u16,
    on_frame: Channel<InvokeResponseBody>,
    on_exit: Channel<Option<String>>,
) -> Result<(RdpSession, Option<String>), Failure> {
    let (target_host, secret): &(Host, Secret) = route.last().ok_or_else(|| Failure::other("", "nenhum host para conectar"))?;
    let none = ConnectOptions::default();
    let target_opts = opts.get(&target_host.id).unwrap_or(&none);
    let fail = |error| Failure { host_id: target_host.id.clone(), error };

    let password = target_opts.password.clone().or_else(|| secret.password.clone()).ok_or_else(|| fail(ConnectError::NeedPassword))?;
    let gateway = match &route[..route.len() - 1] {
        [] => None,
        jumps => Some(Arc::new(ssh::establish(jumps, opts).await?)),
    };
    let (clipboard, clipboard_rx) = mpsc::unbounded_channel();
    let mut target = Target {
        host: target_host.clone(),
        password,
        cert: trusted_cert.clone(),
        trust: target_opts.trust_fingerprint.clone(),
        gateway,
        clipboard,
    };

    // Os futures do IronRDP não são `Send` (não podem ir para o runtime do Tauri): cada sessão
    // roda numa thread própria, com um runtime tokio só dela.
    let (commands, command_rx) = mpsc::unbounded_channel();
    let (opened_tx, opened_rx) = tokio::sync::oneshot::channel();
    let size = DesktopSize { width, height };
    std::thread::Builder::new()
        .name(format!("rdp-{id}"))
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(e) => {
                    let _ = opened_tx.send(Err(ConnectError::other(e)));
                    return;
                }
            };
            runtime.block_on(async move {
                match open(&mut target, size).await {
                    Ok((result, framed)) => {
                        let _ = opened_tx.send(Ok(target.cert.clone()));
                        let reason = run(&mut target, result, framed, command_rx, clipboard_rx, &on_frame).await;
                        app.state::<SessionManager>().remove(id);
                        let _ = on_exit.send(reason);
                    }
                    Err(error) => {
                        let _ = opened_tx.send(Err(error));
                    }
                }
                if let Some(gateway) = target.gateway.take() {
                    gateway.close().await;
                }
            });
        })
        .map_err(|e| fail(ConnectError::other(e)))?;

    let cert = opened_rx.await.map_err(|_| fail(ConnectError::other("a sessão RDP terminou inesperadamente")))?.map_err(fail)?;
    let new_cert = cert.filter(|fp| trusted_cert.as_deref() != Some(fp.as_str()));
    Ok((RdpSession { commands }, new_cert))
}

/// Laço da sessão; reconecta quando o servidor não aceita mudar a resolução ao vivo.
/// Devolve o motivo do fim (None = encerrada pelo usuário ou pelo servidor normalmente).
async fn run(
    target: &mut Target,
    mut result: ConnectionResult,
    mut framed: Framed,
    mut commands: mpsc::UnboundedReceiver<Command>,
    mut clip_events: mpsc::UnboundedReceiver<rdp_clipboard::Event>,
    on_frame: &Channel<InvokeResponseBody>,
) -> Option<String> {
    let mut clipboard = Clipboard::new();
    loop {
        let channels = Channels { commands: &mut commands, clip_events: &mut clip_events, clipboard: &mut clipboard };
        match active_session(result, framed, channels, on_frame).await {
            Ok(End::Closed) => return None,
            Ok(End::Disconnected(reason)) => return describe_disconnect(reason),
            Ok(End::Resize(size)) => match open(target, size).await {
                Ok((r, f)) => (result, framed) = (r, f),
                Err(e) => return Some(describe_connect_error(&e)),
            },
            Err(e) => return Some(e),
        }
    }
}

fn describe_disconnect(reason: GracefulDisconnectReason) -> Option<String> {
    match reason {
        GracefulDisconnectReason::UserInitiated | GracefulDisconnectReason::ServerInitiated => None,
        GracefulDisconnectReason::Other(message) => Some(message),
    }
}

fn describe_connect_error(e: &ConnectError) -> String {
    match e {
        ConnectError::AuthFailed { message } | ConnectError::Other { message } => message.clone(),
        ConnectError::CertChanged { .. } => "o certificado do servidor mudou".into(),
        _ => "não foi possível reconectar".into(),
    }
}

/// TCP (direto ou pelo gateway SSH) → TLS → verificação do certificado → autenticação.
async fn open(target: &mut Target, size: DesktopSize) -> Result<(ConnectionResult, Framed), ConnectError> {
    let host = &target.host.clone();
    let stream: Box<dyn Stream> = match &target.gateway {
        None => {
            let socket = tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::TcpStream::connect((host.hostname.as_str(), host.port)))
                .await
                .map_err(|_| ConnectError::other(format!("tempo esgotado conectando em {}:{}", host.hostname, host.port)))?
                .map_err(|e| ConnectError::other(format!("{}:{} — {e}", host.hostname, host.port)))?;
            let _ = socket.set_nodelay(true);
            Box::new(socket)
        }
        Some(gateway) => {
            let channel = tokio::time::timeout(
                CONNECT_TIMEOUT,
                gateway.handle.channel_open_direct_tcpip(host.hostname.clone(), host.port.into(), "127.0.0.1", 0),
            )
            .await
            .map_err(|_| ConnectError::other(format!("tempo esgotado: o gateway SSH não alcançou {}:{}", host.hostname, host.port)))?
            .map_err(|e| ConnectError::other(format!("o gateway SSH não alcançou {}:{} — {e}", host.hostname, host.port)))?;
            // O canal do russh não é `Sync` (exigido pelo IronRDP): uma ponte em memória resolve.
            let (near, mut far) = tokio::io::duplex(256 * 1024);
            tauri::async_runtime::spawn(async move {
                let mut channel = channel.into_stream();
                let _ = tokio::io::copy_bidirectional(&mut channel, &mut far).await;
            });
            Box::new(near)
        }
    };

    let mut config = connector_config(host, &target.password, size);
    // Endereço do cliente só aparece em logs do servidor; atrás de um gateway não há um real.
    let client_addr = "127.0.0.1:0".parse().expect("endereço fixo");
    // Canais dinâmicos com factory: o servidor os reabre a cada reativação (o xRDP faz isso ao mudar a
    // resolução) e cada abertura precisa de um processador novo.
    let mut drdynvc = ironrdp_dvc::DrdynvcClient::new()
        .with_dynamic_channel_factory(ironrdp_displaycontrol::CHANNEL_NAME, || DisplayControlClient::new(|_| Ok(Vec::new())));
    if host.rdp.gfx {
        // Pipeline gráfico (GFX): H.264 e codecs progressivos, bem mais fluido que bitmaps. A sessão
        // compõe os quadros na mesma imagem; sem decodificador H.264, anuncia só o que sabe pintar.
        drdynvc = drdynvc.with_dynamic_channel_factory(ironrdp_egfx::CHANNEL_NAME, || {
            let decoder = OpenH264Decoder::new().ok().map(|d| Box::new(d) as Box<dyn H264Decoder>);
            GraphicsPipelineClient::new(Box::new(GfxHandler), decoder)
        });
        config.support_dyn_vc_gfx_protocol = true;
    }
    let mut connector = ClientConnector::new(config, client_addr).with_static_channel(drdynvc);
    if host.rdp.clipboard {
        let backend = rdp_clipboard::Backend::new(target.clipboard.clone());
        connector.attach_static_channel(ironrdp_cliprdr::CliprdrClient::new(Box::new(backend)));
    }

    let attempt = handshake(stream, connector, host.hostname.clone(), target.cert.clone(), target.trust.clone());
    let (result, framed, fingerprint) = tokio::time::timeout(CONNECT_TIMEOUT * 2, attempt)
        .await
        .map_err(|_| ConnectError::other(format!("{}:{} não completou a conexão RDP", host.hostname, host.port)))??;
    target.cert = Some(fingerprint);
    Ok((result, framed))
}

/// Negociação, TLS, certificado e autenticação. Devolve também o fingerprint do certificado aceito.
async fn handshake(
    stream: Box<dyn Stream>,
    mut connector: ClientConnector,
    hostname: String,
    known: Option<String>,
    trust: Option<String>,
) -> Result<(ConnectionResult, Framed, String), ConnectError> {
    let mut framed = TokioFramed::new(stream);
    let should_upgrade = ironrdp_tokio::connect_begin(&mut framed, &mut connector).await.map_err(connector_error)?;
    let (stream, leftover) = framed.into_inner();
    let (tls, cert) = ironrdp_tls::upgrade(stream, &hostname)
        .await
        .map_err(|e| ConnectError::other(format!("falha no TLS: {e}")))?;
    let fingerprint = check_cert(known, trust, &cert)?;
    let server_public_key = ironrdp_tls::extract_tls_server_public_key(&cert)
        .ok_or_else(|| ConnectError::other("certificado do servidor sem chave pública"))?
        .to_owned();
    let upgraded = ironrdp_tokio::mark_as_upgraded(should_upgrade, &mut connector);
    let mut framed = TokioFramed::new_with_leftover(Box::new(tls) as Box<dyn Stream>, leftover);
    let outcome = ironrdp_tokio::connect_finalize(
        upgraded,
        connector,
        &mut framed,
        &mut NoKerberos,
        ironrdp_connector::ServerName::new(hostname),
        server_public_key,
        None,
    )
    .await
    .map_err(connector_error)?;
    let result = outcome.into_connected().map_err(connector_error)?;
    Ok((result, framed, fingerprint))
}

/// Confiança no primeiro uso, como o known_hosts do SSH: o xRDP usa certificado autoassinado.
fn check_cert(known: Option<String>, trust: Option<String>, cert: &x509_cert::Certificate) -> Result<String, ConnectError> {
    use x509_cert::der::Encode as _;
    let der = cert.to_der().map_err(|e| ConnectError::other(format!("certificado inválido: {e}")))?;
    let fingerprint = Sha256::digest(&der).iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":");
    if known.as_deref() == Some(fingerprint.as_str()) || trust.as_deref() == Some(fingerprint.as_str()) {
        return Ok(fingerprint);
    }
    Err(match known {
        Some(_) => ConnectError::CertChanged { fingerprint },
        None => ConnectError::UnknownHost { fingerprint, key_type: "Certificado TLS (SHA-256)".into() },
    })
}

fn connector_error(e: ironrdp_connector::ConnectorError) -> ConnectError {
    let message = e.report().to_string();
    match e.kind() {
        ConnectorErrorKind::AccessDenied | ConnectorErrorKind::Credssp(_) => ConnectError::AuthFailed { message },
        _ => ConnectError::Other { message },
    }
}

/// Os eventos do GFX não interessam: a sessão já compõe o resultado na imagem.
struct GfxHandler;

impl GraphicsPipelineHandler for GfxHandler {}

/// Kerberos precisa falar com o KDC; o Nexo autentica por NTLM (NLA) ou pelo login do servidor.
struct NoKerberos;

impl ironrdp_tokio::NetworkClient for NoKerberos {
    async fn send(&mut self, _request: &NetworkRequest) -> ConnectorResult<Vec<u8>> {
        Err(ironrdp_connector::general_err!("Kerberos não é suportado"))
    }
}

fn connector_config(host: &Host, password: &str, size: DesktopSize) -> ironrdp_connector::Config {
    let domain = host.rdp.domain.trim();
    ironrdp_connector::Config {
        desktop_size: clamp_size(size),
        monitor_layout: None,
        desktop_scale_factor: 0,
        // Os dois ligados: o servidor escolhe (Windows usa NLA; o xRDP costuma usar só TLS).
        enable_tls: true,
        enable_credssp: true,
        enable_standard_rdp_security: false,
        credentials: Credentials::UsernamePassword { username: host.username.clone(), password: password.to_string() },
        domain: (!domain.is_empty()).then(|| domain.to_string()),
        client_build: 0,
        client_name: client_name(),
        keyboard_type: KeyboardType::IBM_ENHANCED,
        keyboard_subtype: 0,
        keyboard_functional_keys_count: 12,
        keyboard_layout: keyboard_layout(),
        connection_type: ironrdp_pdu::gcc::ConnectionType::Lan,
        ime_file_name: String::new(),
        bitmap: Some(ironrdp_connector::BitmapConfig {
            lossy_compression: true,
            color_depth: 32,
            codecs: client_codecs_capabilities(&[]).expect("codecs padrão"),
        }),
        dig_product_id: String::new(),
        client_dir: String::new(),
        alternate_shell: String::new(),
        work_dir: String::new(),
        remote_application_mode: false,
        rail_support_level: RailSupportLevel::empty(),
        platform: if cfg!(windows) { MajorPlatformType::WINDOWS } else { MajorPlatformType::UNIX },
        hardware_id: None,
        request_data: None,
        // Login automático: com usuário e senha, o servidor (inclusive o xRDP) pula a tela de login.
        autologon: true,
        enable_audio_playback: false,
        enable_audio_capture: false,
        performance_flags: Default::default(),
        license_cache: None,
        timezone_info: Default::default(),
        compression_type: None,
        enable_server_pointer: true,
        pointer_software_rendering: false,
        multitransport_flags: None,
        support_dyn_vc_gfx_protocol: false,
    }
}

fn clamp_size(size: DesktopSize) -> DesktopSize {
    let (width, height) = MonitorLayoutEntry::adjust_display_size(size.width.into(), size.height.into());
    DesktopSize { width: width as u16, height: height as u16 }
}

fn client_name() -> String {
    let name = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "nexo".into());
    name.chars().take(15).collect()
}

/// Layout de teclado do sistema (id do Windows, ex.: 0x0416 = pt-BR ABNT2). O servidor usa
/// esse id para escolher o mapa de teclas (no xRDP, `km-*.ini`/`xrdp_keyboard.ini`).
fn keyboard_layout() -> u32 {
    #[cfg(windows)]
    {
        // SAFETY: GetKeyboardLayout não tem pré-condições; 0 = thread atual.
        let hkl = unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout(0) };
        (hkl as usize & 0xFFFF) as u32
    }
    #[cfg(not(windows))]
    {
        0x0409
    }
}

/// Entradas da sessão que sobrevivem às reconexões.
struct Channels<'a> {
    commands: &'a mut mpsc::UnboundedReceiver<Command>,
    clip_events: &'a mut mpsc::UnboundedReceiver<rdp_clipboard::Event>,
    clipboard: &'a mut Clipboard,
}

enum End {
    Closed,
    Disconnected(GracefulDisconnectReason),
    Resize(DesktopSize),
}

/// Envia a tela ao frontend respeitando o controle de fluxo.
struct Screen<'a> {
    on_frame: &'a Channel<InvokeResponseBody>,
    dirty: Vec<InclusiveRectangle>,
    in_flight: usize,
    /// Tamanho que o frontend conhece (o GFX pode trocar a imagem no meio do processamento).
    size: (u16, u16),
}

impl Screen<'_> {
    fn mark(&mut self, rect: InclusiveRectangle) {
        self.dirty.push(rect);
        if self.dirty.len() > MAX_DIRTY {
            let all = self.dirty.drain(..).reduce(|a, b| a.union(&b)).expect("não vazio");
            self.dirty.push(all);
        }
    }

    fn flush(&mut self, image: &DecodedImage) -> Result<(), String> {
        if self.size != (image.width(), image.height()) {
            self.send_size(image)?;
        }
        if self.dirty.is_empty() || self.in_flight >= MAX_IN_FLIGHT {
            return Ok(());
        }
        let rects: Vec<_> = self.dirty.drain(..).filter_map(|r| clip(&r, image)).collect();
        if rects.is_empty() {
            return Ok(());
        }
        let bytes: usize = rects.iter().map(|r| 8 + usize::from(r.width()) * usize::from(r.height()) * 4).sum();
        let mut msg = Vec::with_capacity(3 + bytes);
        msg.push(MSG_RECTS);
        msg.extend_from_slice(&(rects.len() as u16).to_le_bytes());
        let stride = image.stride();
        let data = image.data();
        for r in &rects {
            for v in [r.left, r.top, r.width(), r.height()] {
                msg.extend_from_slice(&v.to_le_bytes());
            }
            let row = usize::from(r.width()) * 4;
            for y in r.top..=r.bottom {
                let start = usize::from(y) * stride + usize::from(r.left) * 4;
                let line = &data[start..start + row];
                // Canal alfa sempre opaco: o decodificador pode deixá-lo zerado.
                msg.extend(line.as_chunks::<4>().0.iter().flat_map(|&[r, g, b, _]| [r, g, b, 0xFF]));
            }
        }
        self.in_flight += 1;
        self.on_frame.send(InvokeResponseBody::Raw(msg)).map_err(|_| "painel fechado".to_string())
    }

    fn send_size(&mut self, image: &DecodedImage) -> Result<(), String> {
        let mut msg = vec![MSG_SIZE];
        msg.extend_from_slice(&image.width().to_le_bytes());
        msg.extend_from_slice(&image.height().to_le_bytes());
        self.size = (image.width(), image.height());
        self.dirty.clear();
        self.on_frame.send(InvokeResponseBody::Raw(msg)).map_err(|_| "painel fechado".to_string())?;
        // Tela nova: o próximo envio é a imagem toda.
        self.mark(InclusiveRectangle { left: 0, top: 0, right: image.width() - 1, bottom: image.height() - 1 });
        Ok(())
    }

    fn send_raw(&self, msg: Vec<u8>) -> Result<(), String> {
        self.on_frame.send(InvokeResponseBody::Raw(msg)).map_err(|_| "painel fechado".to_string())
    }
}

fn clip(r: &InclusiveRectangle, image: &DecodedImage) -> Option<InclusiveRectangle> {
    let right = r.right.min(image.width().checked_sub(1)?);
    let bottom = r.bottom.min(image.height().checked_sub(1)?);
    (r.left <= right && r.top <= bottom).then_some(InclusiveRectangle { left: r.left, top: r.top, right, bottom })
}

fn pointer_message(pointer: &DecodedPointer) -> Vec<u8> {
    let mut msg = Vec::with_capacity(9 + pointer.bitmap_data.len());
    msg.push(MSG_POINTER);
    for v in [pointer.width, pointer.height, pointer.hotspot_x, pointer.hotspot_y] {
        msg.extend_from_slice(&v.to_le_bytes());
    }
    msg.extend_from_slice(&pointer.bitmap_data);
    msg
}

fn to_operations(events: Vec<Input>, sync: &mut Option<(bool, bool, bool)>) -> Vec<Operation> {
    let mut ops = Vec::with_capacity(events.len());
    for event in events {
        match event {
            Input::Key { code, down } => {
                let scancode = Scancode::from_u16(code);
                ops.push(if down { Operation::KeyPressed(scancode) } else { Operation::KeyReleased(scancode) });
            }
            Input::Unicode { ch, down } => ops.push(if down { Operation::UnicodeKeyPressed(ch) } else { Operation::UnicodeKeyReleased(ch) }),
            Input::Move { x, y } => ops.push(Operation::MouseMove(MousePosition { x, y })),
            Input::Button { button, down } => {
                // DOM: 0 esquerdo, 1 meio, 2 direito, 3 voltar, 4 avançar.
                let Some(button) = [MouseButton::Left, MouseButton::Middle, MouseButton::Right, MouseButton::X1, MouseButton::X2]
                    .get(usize::from(button))
                    .copied()
                else {
                    continue;
                };
                ops.push(if down { Operation::MouseButtonPressed(button) } else { Operation::MouseButtonReleased(button) });
            }
            Input::Wheel { delta, vertical } => {
                ops.push(Operation::WheelRotations(WheelRotations { is_vertical: vertical, rotation_units: delta }))
            }
            Input::Sync { caps, num, scroll } => *sync = Some((caps, num, scroll)),
            Input::ReleaseAll | Input::ClipboardSync => {}
        }
    }
    ops
}

async fn active_session(
    result: ConnectionResult,
    framed: Framed,
    channels: Channels<'_>,
    on_frame: &Channel<InvokeResponseBody>,
) -> Result<End, String> {
    let Channels { commands, clip_events, clipboard } = channels;
    // Expiração dos bloqueios do canal de área de transferência.
    let mut clip_timer = tokio::time::interval(Duration::from_secs(5));
    let (mut reader, mut writer) = split_tokio_framed(framed);
    let mut image = DecodedImage::new(PixelFormat::RgbA32, result.desktop_size.width, result.desktop_size.height);
    let activation_factory = result.activation_factory;
    let mut stage: ActiveStage = ActiveStageBuilder {
        static_channels: result.static_channels,
        user_channel_id: result.user_channel_id,
        io_channel_id: result.io_channel_id,
        message_channel_id: result.message_channel_id,
        share_id: result.share_id,
        compression_type: result.compression_type,
        enable_server_pointer: result.enable_server_pointer,
        pointer_software_rendering: result.pointer_software_rendering,
    }
    .build();
    let mut input = Database::new();
    let mut screen = Screen { on_frame, dirty: Vec::new(), in_flight: 0, size: (0, 0) };
    screen.send_size(&image)?;
    // Tamanho pedido enquanto o canal de resolução ainda não estava pronto.
    let mut pending_size: Option<(u16, u16)> = None;

    loop {
        let outputs = tokio::select! {
            frame = reader.read_pdu() => {
                let (action, payload) = frame.map_err(|e| format!("conexão perdida: {e}"))?;
                let outputs = stage.process(&mut image, action, &payload, None).map_err(|e| e.report().to_string())?;
                if let Some((w, h)) = pending_size {
                    if stage.display_control_ready() == Some(true) {
                        pending_size = None;
                        if let Some(Ok(frame)) = encode_resize(&mut stage, w, h) {
                            writer.write_all(&frame).await.map_err(|e| e.to_string())?;
                        }
                    }
                }
                outputs
            }
            event = clip_events.recv() => {
                let Some(event) = event else { continue };
                let messages = stage.get_svc_processor_mut::<ironrdp_cliprdr::CliprdrClient>().and_then(|c| clipboard.handle(event, c));
                if let Some(messages) = messages {
                    let frame = stage.process_svc_processor_messages(messages).map_err(|e| e.report().to_string())?;
                    writer.write_all(&frame).await.map_err(|e| e.to_string())?;
                }
                continue;
            }
            _ = clip_timer.tick() => {
                let messages = stage.get_svc_processor_mut::<ironrdp_cliprdr::CliprdrClient>().and_then(|c| c.drive_timeouts().ok());
                if let Some(messages) = messages {
                    let frame = stage.process_svc_processor_messages(messages).map_err(|e| e.report().to_string())?;
                    if !frame.is_empty() {
                        writer.write_all(&frame).await.map_err(|e| e.to_string())?;
                    }
                }
                continue;
            }
            command = commands.recv() => match command {
                None | Some(Command::Close) => {
                    let outputs = stage.graceful_shutdown().map_err(|e| e.report().to_string())?;
                    for out in outputs {
                        if let ActiveStageOutput::ResponseFrame(frame) = out {
                            let _ = writer.write_all(&frame).await;
                        }
                    }
                    // O servidor responde negando o shutdown; o IronRDP então gera a desconexão (MCS
                    // Disconnect Provider Ultimatum). Sem resposta em 2 s, a desconexão sai assim mesmo.
                    let answered = tokio::time::timeout(Duration::from_secs(2), async {
                        while let Ok((action, payload)) = reader.read_pdu().await {
                            let Ok(outputs) = stage.process(&mut image, action, &payload, None) else { return false };
                            let mut done = false;
                            for out in outputs {
                                match out {
                                    ActiveStageOutput::ResponseFrame(frame) if !frame.is_empty() => {
                                        let _ = writer.write_all(&frame).await;
                                    }
                                    ActiveStageOutput::Terminate(_) => done = true,
                                    _ => {}
                                }
                            }
                            if done {
                                return true;
                            }
                        }
                        false
                    })
                    .await;
                    if answered != Ok(true) {
                        let ultimatum = McsMessage::DisconnectProviderUltimatum(DisconnectProviderUltimatum::from_reason(DisconnectReason::UserRequested));
                        if let Ok(frame) = ironrdp_core::encode_vec(&X224(ultimatum)) {
                            let _ = writer.write_all(&frame).await;
                        }
                    }
                    // Fecha o TLS direito (close_notify) em vez de só largar o socket.
                    let _ = tokio::io::AsyncWriteExt::shutdown(&mut writer.into_inner_no_leftover()).await;
                    return Ok(End::Closed);
                }
                Some(Command::Ack) => {
                    screen.in_flight = screen.in_flight.saturating_sub(1);
                    screen.flush(&image)?;
                    continue;
                }
                Some(Command::Resize(w, h)) => {
                    let (w, h) = (clamp_size(DesktopSize { width: w, height: h }).width, clamp_size(DesktopSize { width: w, height: h }).height);
                    if (w, h) == (image.width(), image.height()) {
                        continue;
                    }
                    match encode_resize(&mut stage, w, h) {
                        Some(Ok(frame)) => {
                            writer.write_all(&frame).await.map_err(|e| e.to_string())?;
                            continue;
                        }
                        Some(Err(e)) => return Err(e),
                        // Canal de resolução ainda abrindo: tenta quando ficar pronto.
                        None if stage.display_control_ready() == Some(false) => {
                            pending_size = Some((w, h));
                            continue;
                        }
                        // Servidor sem resolução dinâmica (xRDP antigo): reconecta com o tamanho novo.
                        None => return Ok(End::Resize(DesktopSize { width: w, height: h })),
                    }
                }
                Some(Command::Input(events)) => {
                    if events.iter().any(|e| matches!(e, Input::ClipboardSync)) {
                        let messages = stage.get_svc_processor_mut::<ironrdp_cliprdr::CliprdrClient>().and_then(|c| clipboard.local_changed(c));
                        if let Some(messages) = messages {
                            let frame = stage.process_svc_processor_messages(messages).map_err(|e| e.report().to_string())?;
                            writer.write_all(&frame).await.map_err(|e| e.to_string())?;
                        }
                    }
                    let release = events.iter().any(|e| matches!(e, Input::ReleaseAll));
                    let mut sync = None;
                    let ops = to_operations(events, &mut sync);
                    let mut fastpath = input.apply(ops);
                    if release {
                        fastpath.extend(input.release_all());
                    }
                    if let Some((caps, num, scroll)) = sync {
                        fastpath.push(ironrdp_input::synchronize_event(scroll, num, caps, false));
                    }
                    stage.process_fastpath_input(&mut image, &fastpath).map_err(|e| e.report().to_string())?
                }
            },
        };

        for out in outputs {
            match out {
                ActiveStageOutput::ResponseFrame(frame) => {
                    if !frame.is_empty() {
                        writer.write_all(&frame).await.map_err(|e| format!("conexão perdida: {e}"))?;
                    }
                }
                ActiveStageOutput::GraphicsUpdate(_) => {
                    for rect in stage.take_damage_regions() {
                        screen.mark(rect);
                    }
                }
                ActiveStageOutput::PointerDefault => screen.send_raw(vec![MSG_POINTER_DEFAULT])?,
                ActiveStageOutput::PointerHidden => screen.send_raw(vec![MSG_POINTER_HIDDEN])?,
                ActiveStageOutput::PointerBitmap(pointer) => screen.send_raw(pointer_message(&pointer))?,
                ActiveStageOutput::Terminate(reason) => return Ok(End::Disconnected(reason)),
                ActiveStageOutput::DeactivateAll => {
                    // Sequência de desativação/reativação: o servidor mudou a resolução (ou reiniciou a área de trabalho).
                    let mut activation = activation_factory.create();
                    let mut buf = WriteBuf::new();
                    loop {
                        let written = single_sequence_step_read(&mut reader, &mut activation, &mut buf)
                            .await
                            .map_err(|e| e.report().to_string())?;
                        if written.size().is_some() {
                            writer.write_all(buf.filled()).await.map_err(|e| e.to_string())?;
                        }
                        if let ConnectionActivationState::Finalized {
                            desktop_size,
                            share_id,
                            enable_server_pointer,
                            pointer_software_rendering,
                            static_channel_chunk_size,
                            ..
                        } = activation.connection_activation_state()
                        {
                            image = DecodedImage::new(PixelFormat::RgbA32, desktop_size.width, desktop_size.height);
                            stage.reactivate(
                                activation.io_channel_id(),
                                activation.user_channel_id(),
                                share_id,
                                enable_server_pointer,
                                pointer_software_rendering,
                                static_channel_chunk_size,
                            );
                            screen.send_size(&image)?;
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        screen.flush(&image)?;
    }
}

fn encode_resize(stage: &mut ActiveStage, w: u16, h: u16) -> Option<Result<Vec<u8>, String>> {
    stage.encode_resize(w.into(), h.into(), None, None).map(|r| r.map_err(|e| e.report().to_string()))
}

/// Contra um xRDP real: `docker build -t nexo-xrdp docker/xrdp`, depois
/// `docker run -d --name nexo-xrdp -p 13389:3389 nexo-xrdp` (Debian + xrdp + xorgxrdp, usuário nexo/nexo123) e `cargo test -- --ignored live_rdp --test-threads=1` (um de cada vez: o
/// xRDP entrega a sessão do usuário à conexão mais nova e derruba a anterior).
#[cfg(test)]
mod live {
    use super::*;
    use std::sync::Mutex;

    fn target() -> Target {
        let host = Host {
            id: "rdp".into(),
            label: "xrdp".into(),
            hostname: "127.0.0.1".into(),
            port: 13389,
            username: "nexo".into(),
            protocol: crate::vault::Protocol::Rdp,
            ..Default::default()
        };
        let (clipboard, _) = mpsc::unbounded_channel();
        Target { host, password: "nexo123".into(), cert: None, trust: None, gateway: None, clipboard }
    }

    /// Remonta a tela a partir das mensagens: `w h` (u16) + RGBA.
    fn screenshot(messages: &[Vec<u8>]) -> Vec<u8> {
        let u16_at = |m: &[u8], at: usize| usize::from(u16::from_le_bytes([m[at], m[at + 1]]));
        let (mut w, mut h, mut fb) = (0, 0, Vec::new());
        for m in messages {
            match m[0] {
                MSG_SIZE => {
                    (w, h) = (u16_at(m, 1), u16_at(m, 3));
                    fb = vec![0u8; w * h * 4];
                }
                MSG_RECTS => {
                    let mut at = 3;
                    for _ in 0..u16_at(m, 1) {
                        let (x, y, rw, rh) = (u16_at(m, at), u16_at(m, at + 2), u16_at(m, at + 4), u16_at(m, at + 6));
                        at += 8;
                        for row in 0..rh {
                            let dst = ((y + row) * w + x) * 4;
                            fb[dst..dst + rw * 4].copy_from_slice(&m[at..at + rw * 4]);
                            at += rw * 4;
                        }
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::with_capacity(4 + fb.len());
        out.extend_from_slice(&(w as u16).to_le_bytes());
        out.extend_from_slice(&(h as u16).to_le_bytes());
        out.extend_from_slice(&fb);
        out
    }

    #[tokio::test]
    #[ignore]
    async fn live_rdp() {
        let mut target = target();
        let (clip_tx, clip_rx) = mpsc::unbounded_channel();
        target.clipboard = clip_tx;
        // Primeira conexão: certificado desconhecido.
        let fingerprint = match open(&mut target, DesktopSize { width: 1024, height: 768 }).await {
            Err(ConnectError::UnknownHost { fingerprint, .. }) => fingerprint,
            Err(e) => panic!("esperava certificado desconhecido: {}", describe_connect_error(&e)),
            Ok(_) => panic!("conectou sem confirmar o certificado"),
        };
        target.trust = Some(fingerprint);
        let (result, framed) = open(&mut target, DesktopSize { width: 1024, height: 768 }).await.unwrap_or_else(|e| panic!("{}", describe_connect_error(&e)));

        let messages: Arc<Mutex<Vec<Vec<u8>>>> = Default::default();
        let sink = messages.clone();
        let (commands, command_rx) = mpsc::unbounded_channel();
        let acks = commands.clone();
        let on_frame = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(bytes) = body {
                if bytes[0] == MSG_RECTS {
                    let _ = acks.send(Command::Ack);
                }
                sink.lock().unwrap().push(bytes);
            }
            Ok(())
        });

        let driver = tokio::spawn(async move {
            let rects = |messages: &Arc<Mutex<Vec<Vec<u8>>>>| messages.lock().unwrap().iter().filter(|m| m[0] == MSG_RECTS).count();
            let sizes = |messages: &Arc<Mutex<Vec<Vec<u8>>>>| {
                messages.lock().unwrap().iter().filter(|m| m[0] == MSG_SIZE).map(|m| (u16::from_le_bytes([m[1], m[2]]), u16::from_le_bytes([m[3], m[4]]))).collect::<Vec<_>>()
            };
            // Login automático: a área de trabalho (xterm) aparece sem tela de login.
            for _ in 0..100 {
                if rects(&messages) > 3 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            assert!(rects(&messages) > 0, "nenhuma imagem recebida");
            assert_eq!(sizes(&messages).first(), Some(&(1024, 768)));
            // Resolução dinâmica (xRDP 0.10+) ou reconexão: o tamanho novo tem que chegar.
            commands.send(Command::Resize(1280, 720)).unwrap();
            for _ in 0..100 {
                if sizes(&messages).contains(&(1280, 720)) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            // Clica no xterm e digita `echo ok` + Enter (scancodes).
            tokio::time::sleep(Duration::from_millis(500)).await;
            commands.send(Command::Input(vec![Input::Move { x: 300, y: 200 }, Input::Button { button: 0, down: true }, Input::Button { button: 0, down: false }])).unwrap();
            for code in [0x12u16, 0x2e, 0x23, 0x18, 0x39, 0x18, 0x25, 0x1c] {
                commands.send(Command::Input(vec![Input::Key { code, down: true }, Input::Key { code, down: false }])).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let seen = sizes(&messages);
            let shot = screenshot(&messages.lock().unwrap());
            if let Ok(path) = std::env::var("NEXO_RDP_SHOT") {
                std::fs::write(path, &shot).unwrap();
            }
            commands.send(Command::Close).unwrap();
            (seen, shot)
        });

        let end = run(&mut target, result, framed, command_rx, clip_rx, &on_frame).await;
        let (seen, shot) = driver.await.unwrap();
        assert!(seen.contains(&(1280, 720)), "resolução não mudou: {seen:?}");
        // A tela depois da troca de resolução tem que ter conteúdo (o xterm), não só preto.
        let lit = shot[4..].as_chunks::<4>().0.iter().filter(|&&[r, g, b, _]| r > 40 || g > 40 || b > 40).count();
        assert!(lit > 5_000, "tela vazia depois de mudar a resolução ({lit} pixels acesos)");
        assert_eq!(end, None, "sessão terminou com erro");
    }

    /// Pelo gateway SSH (container openssh da mesma rede Docker, com `AllowTcpForwarding yes`
    /// em 127.0.0.1:2222, ou na porta de `NEXO_SSH_PORT`; o xRDP é alcançado pelo nome `nexo-xrdp`,
    /// invisível daqui).
    #[tokio::test]
    #[ignore]
    async fn live_rdp_gateway() {
        let gateway_host = Host {
            id: "gw".into(),
            label: "gateway".into(),
            hostname: "127.0.0.1".into(),
            port: std::env::var("NEXO_SSH_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(2222),
            username: "nexo".into(),
            ..Default::default()
        };
        let route = vec![(gateway_host, Secret { password: Some("nexo123".into()), ..Default::default() })];
        let mut opts = Options::new();
        let conn = loop {
            match ssh::establish(&route, &opts).await {
                Ok(conn) => break conn,
                Err(Failure { host_id, error: ConnectError::UnknownHost { fingerprint, .. } }) => {
                    opts.insert(host_id, ConnectOptions { trust_fingerprint: Some(fingerprint), ..Default::default() });
                }
                Err(e) => panic!("gateway: {:?}", e.error),
            }
        };
        let mut target = target();
        let (clip_tx, clip_rx) = mpsc::unbounded_channel();
        target.clipboard = clip_tx;
        target.host.hostname = "nexo-xrdp".into();
        target.host.port = 3389;
        target.gateway = Some(Arc::new(conn));
        let fingerprint = match open(&mut target, DesktopSize { width: 800, height: 600 }).await {
            Err(ConnectError::UnknownHost { fingerprint, .. }) => fingerprint,
            Err(e) => panic!("{}", describe_connect_error(&e)),
            Ok(_) => panic!("conectou sem confirmar o certificado"),
        };
        target.trust = Some(fingerprint);
        let (result, framed) = open(&mut target, DesktopSize { width: 800, height: 600 }).await.unwrap_or_else(|e| panic!("{}", describe_connect_error(&e)));

        let rects = Arc::new(Mutex::new(0usize));
        let count = rects.clone();
        let (commands, command_rx) = mpsc::unbounded_channel();
        let acks = commands.clone();
        let on_frame = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(bytes) = body {
                if bytes[0] == MSG_RECTS {
                    *count.lock().unwrap() += 1;
                    let _ = acks.send(Command::Ack);
                }
            }
            Ok(())
        });
        let driver = tokio::spawn(async move {
            for _ in 0..100 {
                if *rects.lock().unwrap() > 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            let n = *rects.lock().unwrap();
            commands.send(Command::Close).unwrap();
            n
        });
        let end = run(&mut target, result, framed, command_rx, clip_rx, &on_frame).await;
        assert!(driver.await.unwrap() > 0, "nenhuma imagem pelo gateway");
        assert_eq!(end, None);
        target.gateway.take().unwrap().close().await;
    }

    /// Roda um comando no container do xRDP (os testes rodam no Windows; o Docker, no WSL).
    fn in_container(script: &str) -> String {
        let out = std::process::Command::new("wsl")
            .args(["docker", "exec", "-u", "nexo", "-e", "DISPLAY=:10", "nexo-xrdp", "sh", "-c", script])
            .output()
            .expect("wsl docker exec");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Área de transferência nos dois sentidos (usa o clipboard real desta máquina; o conteúdo é restaurado).
    #[tokio::test]
    #[ignore]
    async fn live_rdp_clipboard() {
        let mut os = arboard::Clipboard::new().unwrap();
        let saved = os.get_text().ok();

        let mut target = target();
        let (clip_tx, clip_rx) = mpsc::unbounded_channel();
        target.clipboard = clip_tx;
        let fingerprint = match open(&mut target, DesktopSize { width: 800, height: 600 }).await {
            Err(ConnectError::UnknownHost { fingerprint, .. }) => fingerprint,
            _ => panic!("esperava certificado desconhecido"),
        };
        target.trust = Some(fingerprint);
        let (result, framed) = open(&mut target, DesktopSize { width: 800, height: 600 }).await.unwrap_or_else(|e| panic!("{}", describe_connect_error(&e)));
        let (commands, command_rx) = mpsc::unbounded_channel();
        let acks = commands.clone();
        let on_frame = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(bytes) = body {
                if bytes[0] == MSG_RECTS {
                    let _ = acks.send(Command::Ack);
                }
            }
            Ok(())
        });

        let driver = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            // Local → servidor.
            arboard::Clipboard::new().unwrap().set_text("nexo-local-123").unwrap();
            commands.send(Command::Input(vec![Input::ClipboardSync])).unwrap();
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let remote = tokio::task::spawn_blocking(|| in_container("timeout 3 xclip -selection clipboard -o")).await.unwrap();
            // Servidor → local.
            tokio::task::spawn_blocking(|| in_container("printf nexo-remote-456 | xclip -selection clipboard -loops 2 >/dev/null 2>&1 &")).await.unwrap();
            let mut local = String::new();
            for _ in 0..30 {
                tokio::time::sleep(Duration::from_millis(200)).await;
                local = arboard::Clipboard::new().unwrap().get_text().unwrap_or_default();
                if local == "nexo-remote-456" {
                    break;
                }
            }
            commands.send(Command::Close).unwrap();
            (remote, local)
        });
        let end = run(&mut target, result, framed, command_rx, clip_rx, &on_frame).await;
        let (remote, local) = driver.await.unwrap();
        if let Some(saved) = saved {
            let _ = os.set_text(saved);
        }
        assert_eq!(end, None);
        assert_eq!(remote, "nexo-local-123", "texto local não chegou ao servidor");
        assert_eq!(local, "nexo-remote-456", "texto do servidor não chegou aqui");
    }
}
