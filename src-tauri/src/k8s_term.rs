//! Terminais de pods: logs acompanhando ao vivo e shell (`exec` com TTY), pela API do cluster.

use std::sync::Arc;
use std::time::Duration;

use futures::{AsyncReadExt as _, SinkExt as _};
use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Status;
use kube::api::{AttachParams, LogParams, TerminalSize};
use kube::{Api, Client};
use serde::Deserialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::sync::mpsc;

use crate::flow::Flow;
use crate::k8s::{checked, describe};
use crate::session::Session;

/// Mesmo comando do shell de container Docker: bash se houver, senão sh.
const SHELL: &str = "command -v bash >/dev/null && exec bash || exec sh";

#[derive(Deserialize, Debug, Clone, Copy)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Mode {
    /// `previous`: do container anterior (o que reiniciou).
    Logs { previous: bool },
    Shell,
}

enum Input {
    Data(Vec<u8>),
    Resize(u16, u16),
    Close,
}

pub struct KubeTerm {
    input: mpsc::UnboundedSender<Input>,
    flow: Arc<Flow>,
}

impl Session for KubeTerm {
    fn write(&self, data: &[u8]) -> std::io::Result<()> {
        self.input
            .send(Input::Data(data.to_vec()))
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        self.input.send(Input::Resize(cols, rows)).map_err(|e| e.to_string())
    }

    fn kill(&self) {
        let _ = self.input.send(Input::Close);
    }

    fn ack(&self, bytes: usize) {
        self.flow.ack(bytes);
    }
}

/// Conecta (os erros voltam aqui, antes de existir sessão) e deixa uma task levando os dados.
#[allow(clippy::too_many_arguments)]
pub async fn open(
    client: Client,
    namespace: &str,
    pod: &str,
    container: &str,
    mode: Mode,
    cols: u16,
    rows: u16,
    on_data: Channel<InvokeResponseBody>,
    on_exit: Channel<Option<u32>>,
) -> Result<KubeTerm, String> {
    let api: Api<Pod> = Api::namespaced(client, checked(namespace)?);
    let pod = checked(pod)?;
    let (input, rx) = mpsc::unbounded_channel();
    let flow = Arc::new(Flow::default());
    match mode {
        Mode::Logs { previous } => {
            let params = LogParams {
                container: Some(container.to_owned()),
                follow: !previous,
                previous,
                tail_lines: Some(1000),
                ..Default::default()
            };
            let stream = api.log_stream(pod, &params).await.map_err(describe)?;
            tokio::spawn(logs(Box::pin(stream), rx, flow.clone(), on_data, on_exit));
        }
        Mode::Shell => {
            let params = AttachParams::interactive_tty().container(container);
            let process = api.exec(pod, ["sh", "-c", SHELL], &params).await.map_err(describe)?;
            tokio::spawn(shell(process, rx, flow.clone(), cols, rows, on_data, on_exit));
        }
    }
    Ok(KubeTerm { input, flow })
}

/// Logs não têm TTY: as quebras de linha viram `\r\n` para o xterm. Ctrl+C encerra.
async fn logs(
    mut stream: std::pin::Pin<Box<impl futures::AsyncBufRead + Send>>,
    mut rx: mpsc::UnboundedReceiver<Input>,
    flow: Arc<Flow>,
    on_data: Channel<InvokeResponseBody>,
    on_exit: Channel<Option<u32>>,
) {
    let mut buf = vec![0u8; 32 * 1024];
    let mut last = 0u8;
    loop {
        tokio::select! {
            _ = flow.wait_resumed(), if flow.is_paused() => {}
            read = stream.read(&mut buf), if !flow.is_paused() => match read {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let data = crlf(&buf[..n], &mut last);
                    flow.sent(data.len());
                    if on_data.send(InvokeResponseBody::Raw(data)).is_err() { break; }
                }
            },
            msg = rx.recv() => match msg {
                Some(Input::Data(d)) if d.contains(&3) => break,
                Some(Input::Close) | None => break,
                _ => {}
            },
        }
    }
    let _ = on_exit.send(None);
}

fn crlf(data: &[u8], last: &mut u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 32);
    for &b in data {
        if b == b'\n' && *last != b'\r' {
            out.push(b'\r');
        }
        out.push(b);
        *last = b;
    }
    out
}

async fn shell(
    mut process: kube::api::AttachedProcess,
    mut rx: mpsc::UnboundedReceiver<Input>,
    flow: Arc<Flow>,
    cols: u16,
    rows: u16,
    on_data: Channel<InvokeResponseBody>,
    on_exit: Channel<Option<u32>>,
) {
    let (Some(mut stdin), Some(mut stdout), Some(mut sizes), Some(status)) =
        (process.stdin(), process.stdout(), process.terminal_size(), process.take_status())
    else {
        let _ = on_exit.send(None);
        return;
    };
    let _ = sizes.send(TerminalSize { width: cols, height: rows }).await;
    let mut buf = vec![0u8; 32 * 1024];
    let mut closed = false;
    loop {
        tokio::select! {
            _ = flow.wait_resumed(), if flow.is_paused() => {}
            read = stdout.read(&mut buf), if !flow.is_paused() => match read {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    flow.sent(n);
                    if on_data.send(InvokeResponseBody::Raw(buf[..n].to_vec())).is_err() { break; }
                }
            },
            msg = rx.recv() => match msg {
                Some(Input::Data(d)) => { if stdin.write_all(&d).await.is_err() { break; } }
                Some(Input::Resize(width, height)) => { let _ = sizes.send(TerminalSize { width, height }).await; }
                Some(Input::Close) | None => { closed = true; break; }
            },
        }
    }
    let code = if closed {
        process.abort();
        None
    } else {
        tokio::time::timeout(Duration::from_secs(2), status).await.ok().flatten().as_ref().and_then(exit_code)
    };
    let _ = on_exit.send(code);
}

/// `Success` = 0; `NonZeroExitCode` traz o código numa das causas.
fn exit_code(status: &Status) -> Option<u32> {
    if status.status.as_deref() == Some("Success") {
        return Some(0);
    }
    status
        .details
        .as_ref()?
        .causes
        .as_ref()?
        .iter()
        .find(|c| c.reason.as_deref() == Some("ExitCode"))?
        .message
        .as_deref()?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_endings() {
        let mut last = 0;
        assert_eq!(crlf(b"a\nb\r\nc\n", &mut last), b"a\r\nb\r\nc\r\n");
        // `\r` no fim de um pedaço e `\n` no começo do próximo.
        let mut last = 0;
        crlf(b"x\r", &mut last);
        assert_eq!(crlf(b"\ny", &mut last), b"\ny");
    }
}
