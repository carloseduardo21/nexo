//! Containers Docker do servidor de uma sessão SSH: lista (com uso de CPU/memória) e ações.
//! Roda o CLI `docker` do servidor num canal extra da conexão, como as métricas (`hostinfo.rs`).

use serde::{Deserialize, Serialize};

/// Lista e estatísticas numa chamada só, separadas por `@stats`.
/// Saída 4: sem Docker no servidor. As estatísticas são opcionais (podem falhar sozinhas).
pub const LIST_SCRIPT: &str = "command -v docker >/dev/null 2>&1 || { echo 'docker não encontrado' >&2; exit 4; }
docker ps -a --no-trunc --format '{{json .}}' || exit 5
echo @stats
docker stats --no-stream --no-trunc --format '{{json .}}' 2>/dev/null
exit 0
";

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    /// `running`, `exited`, `paused`, `restarting`, `created`, `dead`.
    pub state: String,
    /// Ex.: `Up 6 hours`, `Exited (0) 2 days ago`.
    pub status: String,
    pub ports: String,
    /// Projeto do Docker Compose, para agrupar.
    pub project: String,
    /// Uso de CPU (% de um núcleo; passa de 100 com vários) e memória — só dos que estão rodando.
    pub cpu: Option<f64>,
    pub mem: Option<f64>,
    pub mem_usage: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PsLine {
    #[serde(rename = "ID")]
    id: String,
    names: String,
    image: String,
    state: String,
    status: String,
    #[serde(default)]
    ports: String,
    #[serde(default)]
    labels: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct StatsLine {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "CPUPerc")]
    cpu_perc: String,
    mem_perc: String,
    mem_usage: String,
}

pub fn parse_list(output: &str) -> Vec<Container> {
    let (ps, stats) = output.split_once("\n@stats").unwrap_or((output, ""));
    let stats: Vec<StatsLine> = stats.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let mut list: Vec<Container> = ps
        .lines()
        .filter_map(|l| serde_json::from_str::<PsLine>(l).ok())
        .map(|c| {
            let s = stats.iter().find(|s| s.id == c.id);
            Container {
                project: label(&c.labels, "com.docker.compose.project").unwrap_or_default().to_string(),
                cpu: s.and_then(|s| percent(&s.cpu_perc)),
                mem: s.and_then(|s| percent(&s.mem_perc)),
                mem_usage: s.map(|s| s.mem_usage.clone()).unwrap_or_default(),
                id: c.id,
                name: c.names,
                image: c.image,
                state: c.state,
                status: c.status,
                ports: c.ports,
            }
        })
        .collect();
    list.sort_by(|a, b| a.project.cmp(&b.project).then_with(|| a.name.cmp(&b.name)));
    list
}

/// Valor de um label em `chave=valor,chave=valor`. Outros labels podem ter vírgulas no valor;
/// os do Compose não.
fn label<'a>(labels: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("{key}=");
    let start = labels.match_indices(&needle).find(|(i, _)| *i == 0 || labels.as_bytes()[i - 1] == b',')?.0 + needle.len();
    labels[start..].split(',').next()
}

fn percent(s: &str) -> Option<f64> {
    s.trim().trim_end_matches('%').parse().ok()
}

/// Ações sobre um container. `remove` só funciona com ele parado (sem `-f`, de propósito).
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
    Remove,
}

impl Action {
    fn verb(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Pause => "pause",
            Self::Unpause => "unpause",
            Self::Remove => "rm",
        }
    }
}

/// ID de container como o `docker ps --no-trunc` devolve (hex): nada a escapar no shell.
pub fn valid_id(id: &str) -> bool {
    (12..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn action_command(id: &str, action: Action) -> String {
    format!("docker {} {id}", action.verb())
}

/// Comando para acompanhar os logs num terminal. Com `sudo`, o prompt de senha aparece no
/// próprio terminal (e o Nexo sugere a senha salva, como em qualquer prompt).
pub fn logs_command(id: &str, sudo: bool) -> String {
    format!("{}docker logs -f --tail 500 {id}", if sudo { "sudo " } else { "" })
}

/// Shell interativo no container: bash se houver, senão sh.
pub fn shell_command(id: &str, sudo: bool) -> String {
    format!("{}docker exec -it {id} sh -c 'command -v bash >/dev/null && exec bash || exec sh'", if sudo { "sudo " } else { "" })
}

/// Como rodar o comando: direto, ou via `sudo` com a senha na entrada (`-S`) — ou sem senha
/// (`-n`: falha na hora se o sudo pedir uma, em vez de travar esperando).
pub fn with_sudo(script: &str, sudo: bool, has_password: bool) -> String {
    match (sudo, has_password) {
        (false, _) => script.to_string(),
        (true, true) => format!("sudo -S -p '' sh -c {}", shell_quote(script)),
        (true, false) => format!("sudo -n sh -c {}", shell_quote(script)),
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Erros que o painel trata: pedir a senha do sudo, oferecer o sudo.
#[derive(Serialize, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Error {
    /// Usuário fora do grupo docker: dá para tentar com sudo.
    Permission { message: String },
    /// O sudo pediu senha e não havia uma (ou ela estava errada).
    NeedSudoPassword { wrong: bool },
    Other { message: String },
}

impl Error {
    pub fn other(message: impl Into<String>) -> Self {
        Self::Other { message: message.into() }
    }
}

/// Classifica o erro do `docker` (ou do `sudo` antes dele).
pub fn describe_error(status: Option<u32>, stderr: &str) -> Error {
    let lower = stderr.to_lowercase();
    if lower.contains("a password is required") || lower.contains("a terminal is required") {
        return Error::NeedSudoPassword { wrong: false };
    }
    if lower.contains("incorrect password") || lower.contains("sorry, try again") {
        return Error::NeedSudoPassword { wrong: true };
    }
    if lower.contains("not in the sudoers") || lower.contains("is not allowed to") {
        return Error::other("o usuário não tem permissão para usar sudo neste servidor");
    }
    if lower.contains("sudo: command not found") || lower.contains("sudo: not found") {
        return Error::other("o sudo não está instalado neste servidor");
    }
    if status == Some(4) {
        return Error::other("o Docker não está instalado neste servidor");
    }
    if lower.contains("permission denied") && lower.contains("docker") {
        return Error::Permission { message: "sem permissão para usar o Docker (o usuário não está no grupo docker)".into() };
    }
    // "Cannot connect to the Docker daemon" (até o 28) / "failed to connect to the docker API" (29+).
    if lower.contains("connect to the docker daemon") || lower.contains("failed to connect to the docker api") {
        return Error::other("o serviço do Docker não está rodando no servidor");
    }
    let first = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("o comando docker falhou");
    Error::other(first.trim_start_matches("Error response from daemon: "))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Saída real (Docker 29), encurtada nos labels.
    const SAMPLE: &str = r#"{"Command":"\"/init\"","CreatedAt":"2026-10-26 10:40:19 -0300 -03","HealthStatus":"none","ID":"5ba971043517db10fb0416596d620377b7d4a6a09a42442f239e788a58c82640","Image":"lscr.io/linuxserver/openssh-server","Labels":"build_version=Linuxserver.io version:- 10.3,org.opencontainers.image.description=sandboxed, limited, etc.,maintainer=aptalca","LocalVolumes":"1","Mounts":"5844c5dfd389","Names":"nexo-ssh","Networks":"nexo-net","Platform":{"architecture":"amd64","os":"linux"},"Ports":"0.0.0.0:2223->2222/tcp, [::]:2223->2222/tcp","RunningFor":"6 hours ago","Size":"774kB (virtual 41.6MB)","State":"running","Status":"Up 6 hours"}
{"Command":"\"docker-entrypoint.sh postgres\"","ID":"9f3a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a","Image":"postgres:16","Labels":"com.docker.compose.config-hash=abc,com.docker.compose.project=loja,com.docker.compose.service=db","Names":"loja-db-1","Ports":"5432/tcp","State":"running","Status":"Up 2 days (healthy)"}
{"Command":"\"/init\"","ID":"6d7c1ab8f00002fa7a757a6b997ba7c4bfee96a9ccb72020e66f0ff6472527f7","Image":"lscr.io/linuxserver/openssh-server:latest","Labels":"","Names":"nexo-sftp","Ports":"","State":"exited","Status":"Exited (0) 2 days ago"}

@stats
{"BlockIO":"13.5MB / 528kB","CPUPerc":"0.00%","Container":"5ba971043517db10fb0416596d620377b7d4a6a09a42442f239e788a58c82640","ID":"5ba971043517db10fb0416596d620377b7d4a6a09a42442f239e788a58c82640","MemPerc":"0.14%","MemUsage":"10.52MiB / 7.6GiB","Name":"nexo-ssh","NetIO":"141kB / 155kB","PIDs":"13"}
{"BlockIO":"483MB / 11.3MB","CPUPerc":"131.73%","Container":"9f3a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a","ID":"9f3a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a","MemPerc":"2.68%","MemUsage":"208.7MiB / 7.6GiB","Name":"loja-db-1","NetIO":"6.32MB / 3.26MB","PIDs":"91"}
"#;

    #[test]
    fn parses_containers_and_stats() {
        let list = parse_list(SAMPLE);
        let names: Vec<_> = list.iter().map(|c| c.name.as_str()).collect();
        // Sem projeto primeiro, depois por projeto; por nome dentro de cada grupo.
        assert_eq!(names, ["nexo-sftp", "nexo-ssh", "loja-db-1"]);
        let db = &list[2];
        assert_eq!(db.project, "loja");
        assert_eq!(db.cpu, Some(131.73));
        assert_eq!(db.mem_usage, "208.7MiB / 7.6GiB");
        let ssh = &list[1];
        assert_eq!(ssh.state, "running");
        assert_eq!(ssh.mem, Some(0.14));
        assert_eq!(ssh.ports, "0.0.0.0:2223->2222/tcp, [::]:2223->2222/tcp");
        assert_eq!(ssh.project, "");
        let stopped = &list[0];
        assert_eq!((stopped.cpu, stopped.status.as_str()), (None, "Exited (0) 2 days ago"));
    }

    #[test]
    fn compose_label_needs_exact_key() {
        assert_eq!(label("a=1,com.docker.compose.project=x,b=2", "com.docker.compose.project"), Some("x"));
        assert_eq!(label("my.com.docker.compose.project=y", "com.docker.compose.project"), None);
    }

    #[test]
    fn ids_are_validated() {
        assert!(valid_id("5ba971043517"));
        assert!(valid_id("5ba971043517db10fb0416596d620377b7d4a6a09a42442f239e788a58c82640"));
        assert!(!valid_id("nexo-ssh"));
        assert!(!valid_id("5ba971043517; rm -rf /"));
    }

    fn message(e: Error) -> String {
        match e {
            Error::Permission { message } | Error::Other { message } => message,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn friendly_errors() {
        let denied = "permission denied while trying to connect to the Docker daemon socket at unix:///var/run/docker.sock";
        assert!(matches!(describe_error(Some(1), denied), Error::Permission { .. }));
        assert!(message(describe_error(Some(4), "")).contains("não está instalado"));
        let down = "failed to connect to the docker API at unix:///var/run/docker.sock; check if the path is correct and if the daemon is running";
        assert!(message(describe_error(Some(5), down)).contains("não está rodando"));
        assert_eq!(message(describe_error(Some(1), "Error response from daemon: conflict: container is running")), "conflict: container is running");
    }

    #[test]
    fn sudo_errors() {
        assert_eq!(describe_error(Some(1), "sudo: a password is required"), Error::NeedSudoPassword { wrong: false });
        assert_eq!(describe_error(Some(1), "Sorry, try again.\nsudo: 1 incorrect password attempt"), Error::NeedSudoPassword { wrong: true });
        assert!(message(describe_error(Some(1), "carlos is not in the sudoers file.")).contains("permissão para usar sudo"));
    }

    #[test]
    fn sudo_wraps_the_whole_script() {
        assert_eq!(with_sudo("docker ps", false, true), "docker ps");
        assert_eq!(with_sudo("echo 'a'", true, true), r"sudo -S -p '' sh -c 'echo '\''a'\'''");
        assert!(with_sudo("docker ps", true, false).starts_with("sudo -n sh -c "));
    }
}

/// Roda o script de listagem no Docker do WSL (só leitura) e confere o parser com a saída real:
/// `cargo test -- --ignored live_docker_list`.
#[cfg(test)]
mod live {
    use super::*;

    #[test]
    #[ignore]
    fn live_docker_list() {
        let out = std::process::Command::new("wsl").args(["bash", "-c", LIST_SCRIPT]).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let list = parse_list(&String::from_utf8_lossy(&out.stdout));
        for c in &list {
            println!("{:<24} {:<10} {:>8} {:<24} {}", c.name, c.state, c.cpu.map(|v| format!("{v:.1}%")).unwrap_or_default(), c.mem_usage, c.project);
        }
        assert!(!list.is_empty(), "nenhum container no Docker do WSL");
        assert!(list.iter().all(|c| valid_id(&c.id)));
        assert!(list.iter().filter(|c| c.state == "running").all(|c| c.cpu.is_some() && !c.mem_usage.is_empty()));
    }

    /// O script passa inteiro pelo `sh -c '…'` do sudo (aspas simples dentro dele escapadas).
    #[test]
    #[ignore]
    fn live_docker_list_quoted() {
        let quoted = format!("sh -c {}", shell_quote(LIST_SCRIPT));
        let out = std::process::Command::new("wsl").args(["bash", "-c", &quoted]).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert!(!parse_list(&String::from_utf8_lossy(&out.stdout)).is_empty());
    }

    /// Senha do sudo pela entrada, contra o container SSH dos outros testes (porta 2222 ou
    /// `NEXO_SSH_PORT`). Lá o `nexo` não está no sudoers: com a senha certa, o sudo aceita a
    /// senha e recusa o comando — o que prova que ela chegou.
    #[tokio::test]
    #[ignore]
    async fn live_sudo_password() {
        let conn = crate::hostinfo::live::connect("NEXO_SSH_PORT", 2222).await;
        let run = |password: Option<&'static str>| {
            let conn = &conn;
            async move {
                let command = with_sudo("id -u", true, password.is_some());
                let input = password.map(|p| format!("{p}\n"));
                let out = conn.run_with_input(&command, input.as_deref().map(str::as_bytes)).await.unwrap();
                describe_error(out.status, &out.stderr())
            }
        };
        assert_eq!(run(None).await, Error::NeedSudoPassword { wrong: false });
        assert_eq!(run(Some("errada")).await, Error::NeedSudoPassword { wrong: true });
        let Error::Other { message } = run(Some("nexo123")).await else { panic!("esperava recusa do sudoers") };
        assert!(message.contains("permissão para usar sudo"), "{message}");
        conn.close().await;
    }
}
