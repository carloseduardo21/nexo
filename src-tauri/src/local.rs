//! Terminal local via PTY (ConPTY no Windows, openpty no Linux/macOS).

use std::io::{Read, Write};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Manager};

use crate::flow::Flow;
use crate::session::{Session, SessionManager};
use crate::shells::ShellProfile;

pub struct LocalPty {
    /// Fila para a thread de escrita: quem chama nunca bloqueia e a ordem das teclas é preservada.
    input: Sender<Vec<u8>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    flow: Arc<Flow>,
    profile: ShellProfile,
}

impl Session for LocalPty {
    fn write(&self, data: &[u8]) -> std::io::Result<()> {
        self.input
            .send(data.to_vec())
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        self.master
            .lock()
            .unwrap()
            .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())
    }

    fn kill(&self) {
        let _ = self.killer.lock().unwrap().kill();
    }

    fn ack(&self, bytes: usize) {
        self.flow.ack(bytes);
    }

    fn local_profile(&self) -> Option<&ShellProfile> {
        Some(&self.profile)
    }
}

/// `sh -c <script>` no Linux/macOS local ou numa distro do WSL. PowerShell e cmd não têm.
pub fn sh_command(profile: &ShellProfile, script: &str) -> Option<(String, Vec<String>)> {
    match profile.kind.as_str() {
        "unix" => Some(("sh".into(), vec!["-c".into(), script.into()])),
        "wsl" => {
            let distro = profile.id.strip_prefix("wsl:")?;
            Some(("wsl.exe".into(), ["-d", distro, "-e", "sh", "-c", script].map(String::from).to_vec()))
        }
        _ => None,
    }
}

/// Roda um script curto (painel do host) fora do terminal, como o `Connection::run` do SSH.
pub async fn run(profile: &ShellProfile, script: &str, input: Option<&[u8]>) -> Result<crate::ssh::Output, String> {
    use tokio::io::AsyncWriteExt;
    let (program, args) = sh_command(profile, script).ok_or("só em terminais Linux e WSL")?;
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take();
    if let (Some(stdin), Some(input)) = (stdin.as_mut(), input) {
        let _ = stdin.write_all(input).await;
    }
    drop(stdin);
    let out = tokio::time::timeout(std::time::Duration::from_secs(20), child.wait_with_output())
        .await
        .map_err(|_| "o comando demorou demais".to_owned())?
        .map_err(|e| e.to_string())?;
    Ok(crate::ssh::Output {
        status: out.status.code().and_then(|c| u32::try_from(c).ok()),
        stdout: out.stdout,
        stderr: out.stderr,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn spawn(
    app: AppHandle,
    id: u32,
    profile: &ShellProfile,
    command: Option<&str>,
    cols: u16,
    rows: u16,
    on_data: Channel<InvokeResponseBody>,
    on_exit: Channel<Option<u32>>,
) -> Result<Arc<LocalPty>, String> {
    let pair = native_pty_system()
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| e.to_string())?;

    // Com `command` (logs/shell de um container), roda só ele no lugar do shell.
    let (program, args) = match command {
        Some(script) => sh_command(profile, script).ok_or("só em terminais Linux e WSL")?,
        None => (profile.program.clone(), profile.args.clone()),
    };
    let mut cmd = CommandBuilder::new(program);
    cmd.args(args);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        cmd.cwd(home);
    }

    let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);

    let killer = child.clone_killer();
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;

    let (input, input_rx) = mpsc::channel::<Vec<u8>>();
    thread::Builder::new()
        .name(format!("pty-write-{id}"))
        .spawn(move || {
            for chunk in input_rx {
                if writer.write_all(&chunk).and_then(|_| writer.flush()).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| e.to_string())?;

    let flow = Arc::new(Flow::default());
    let reader_flow = flow.clone();

    // Leitura bloqueante numa thread dedicada: cada chunk vai direto pro frontend,
    // sem fila intermediária — o eco de uma tecla chega no menor tempo possível.
    let reader_thread = thread::Builder::new()
        .name(format!("pty-read-{id}"))
        .spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        reader_flow.sent(n);
                        if on_data.send(InvokeResponseBody::Raw(buf[..n].to_vec())).is_err() {
                            break;
                        }
                        reader_flow.wait_blocking();
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;

    // Quando o processo termina, descartamos a sessão (fecha o PTY e libera o reader)
    // e só então avisamos o frontend, garantindo que toda a saída já foi entregue.
    thread::Builder::new()
        .name(format!("pty-wait-{id}"))
        .spawn(move || {
            let code = child.wait().ok().map(|s| s.exit_code());
            app.state::<SessionManager>().remove(id);
            let _ = reader_thread.join();
            let _ = on_exit.send(code);
        })
        .map_err(|e| e.to_string())?;

    Ok(Arc::new(LocalPty {
        input,
        master: Mutex::new(pair.master),
        killer: Mutex::new(killer),
        flow,
        profile: profile.clone(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O script chega inteiro (aspas, `$`, quebras de linha) e o código de saída volta.
    #[tokio::test]
    #[ignore]
    async fn live_wsl_run() {
        let profile = crate::shells::profiles().iter().find(|p| p.kind == "wsl" || p.kind == "unix").expect("sem WSL").clone();
        let script = "a='x y'; echo \"[$a]\" \"$(printf '%s' \"q'q\")\"\nread line; echo \"got:$line\"; exit 7";
        let out = run(&profile, script, Some(b"segredo\n")).await.unwrap();
        assert_eq!(out.stdout(), "[x y] q'q\ngot:segredo\n", "{}", out.stderr());
        assert_eq!(out.status, Some(7));
        let metrics = run(&profile, crate::hostinfo::METRICS_SCRIPT, None).await.unwrap();
        assert_eq!(metrics.status, Some(0), "{}", metrics.stderr());
        crate::hostinfo::parse(&metrics.stdout()).unwrap();
    }
}
