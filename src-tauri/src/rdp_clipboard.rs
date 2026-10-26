//! Área de transferência de sessões RDP (canal CLIPRDR), só texto.
//!
//! Servidor → local: o servidor anuncia uma cópia, a sessão pede o texto e grava no clipboard do
//! sistema na hora. Local → servidor: ao ganhar o foco, o painel avisa a sessão, que anuncia o texto
//! local (se mudou); o servidor só pede os dados quando alguém cola lá.
//!
//! Os callbacks do backend rodam dentro do processamento do canal e não podem enviar nada; eles
//! viram eventos que o laço da sessão trata (`Clipboard::handle`).

use ironrdp_cliprdr::backend::CliprdrBackend;
use ironrdp_cliprdr::pdu::{
    ClipboardFormat, ClipboardFormatId, ClipboardGeneralCapabilityFlags, FileContentsRequest, FileContentsResponse,
    FormatDataRequest, FormatDataResponse, LockDataId,
};
use ironrdp_cliprdr::CliprdrClient;
use ironrdp_core::IntoOwned as _;
use tokio::sync::mpsc;

pub type Messages = ironrdp_cliprdr::CliprdrSvcMessages<ironrdp_cliprdr::Client>;

/// O que o backend do canal pede ao laço da sessão.
#[derive(Debug)]
pub enum Event {
    /// Canal pronto (ou o servidor pediu nossa lista de formatos): anunciar o texto local.
    Announce,
    /// O servidor copiou texto: buscar.
    RemoteCopied(ClipboardFormatId),
    /// Texto que veio do servidor.
    RemoteText(String),
    /// O servidor quer colar o nosso texto.
    DataRequested(ClipboardFormatId),
}

#[derive(Debug)]
pub struct Backend {
    events: mpsc::UnboundedSender<Event>,
}

impl Backend {
    pub fn new(events: mpsc::UnboundedSender<Event>) -> Self {
        Self { events }
    }

    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }
}

impl ironrdp_core::AsAny for Backend {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn core::any::Any {
        self
    }
}

impl CliprdrBackend for Backend {
    fn temporary_directory(&self) -> &str {
        ".cliprdr"
    }

    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        ClipboardGeneralCapabilityFlags::USE_LONG_FORMAT_NAMES
    }

    fn on_ready(&mut self) {
        self.emit(Event::Announce);
    }

    fn on_request_format_list(&mut self) {
        self.emit(Event::Announce);
    }

    fn on_process_negotiated_capabilities(&mut self, _capabilities: ClipboardGeneralCapabilityFlags) {}

    fn on_remote_copy(&mut self, formats: &[ClipboardFormat]) {
        let has = |id| formats.iter().any(|f| f.id() == id);
        if has(ClipboardFormatId::CF_UNICODETEXT) {
            self.emit(Event::RemoteCopied(ClipboardFormatId::CF_UNICODETEXT));
        } else if has(ClipboardFormatId::CF_TEXT) {
            self.emit(Event::RemoteCopied(ClipboardFormatId::CF_TEXT));
        }
    }

    fn on_format_data_request(&mut self, request: FormatDataRequest) {
        self.emit(Event::DataRequested(request.format));
    }

    fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
        if response.is_error() {
            return;
        }
        // A resposta não diz o formato: tenta UTF-16 (o pedido normal) e cai para ANSI.
        let text = response.to_unicode_string().or_else(|_| response.to_string());
        if let Ok(text) = text {
            self.emit(Event::RemoteText(text));
        }
    }

    fn on_file_contents_request(&mut self, _request: FileContentsRequest) {}

    fn on_file_contents_response(&mut self, _response: FileContentsResponse<'_>) {}

    fn on_lock(&mut self, _data_id: LockDataId) {}

    fn on_unlock(&mut self, _data_id: LockDataId) {}
}

/// Lado da sessão: fala com o clipboard do sistema e gera as mensagens do canal.
pub struct Clipboard {
    os: Option<arboard::Clipboard>,
    /// Último texto trocado com o servidor (nos dois sentidos): evita anunciar de volta o que veio de lá.
    last: Option<String>,
}

impl Clipboard {
    pub fn new() -> Self {
        Self { os: arboard::Clipboard::new().ok(), last: None }
    }

    fn local_text(&mut self) -> Option<String> {
        self.os.as_mut()?.get_text().ok().filter(|t| !t.is_empty())
    }

    /// Painel ganhou o foco: se o texto local mudou, anuncia ao servidor.
    pub fn local_changed(&mut self, cliprdr: &mut CliprdrClient) -> Option<Messages> {
        let text = self.local_text()?;
        if self.last.as_deref() == Some(text.as_str()) {
            return None;
        }
        self.last = Some(text);
        cliprdr.initiate_copy(&[ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)]).ok()
    }

    pub fn handle(&mut self, event: Event, cliprdr: &mut CliprdrClient) -> Option<Messages> {
        match event {
            Event::Announce => {
                // Anuncia mesmo sem texto novo: a lista de formatos é obrigatória ao iniciar o canal.
                let formats: &[ClipboardFormat] = if self.local_text().is_some() {
                    &[ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)]
                } else {
                    &[]
                };
                self.last = self.local_text();
                cliprdr.initiate_copy(formats).ok()
            }
            Event::RemoteCopied(format) => cliprdr.initiate_paste(format).ok(),
            Event::RemoteText(text) => {
                // O Windows usa CRLF; servidores Linux mandam LF.
                let text = text.trim_end_matches('\0').to_string();
                if let Some(os) = self.os.as_mut() {
                    let _ = os.set_text(text.clone());
                }
                self.last = Some(text);
                None
            }
            Event::DataRequested(format) => {
                let text = self.local_text().unwrap_or_default();
                let response = if format == ClipboardFormatId::CF_TEXT {
                    FormatDataResponse::new_string(&text)
                } else {
                    FormatDataResponse::new_unicode_string(&text)
                };
                cliprdr.submit_format_data(response.into_owned()).ok()
            }
        }
    }
}
