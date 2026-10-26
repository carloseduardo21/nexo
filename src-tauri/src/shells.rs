//! Detecta os shells locais disponíveis (PowerShell, cmd, distros WSL, bash/zsh...).

use serde::Serialize;
use std::sync::OnceLock;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellProfile {
    pub id: String,
    pub name: String,
    /// "powershell" | "cmd" | "wsl" | "unix" — usado pelo frontend para o ícone.
    pub kind: String,
    pub program: String,
    pub args: Vec<String>,
}

fn profile(id: &str, name: &str, kind: &str, program: &str, args: &[&str]) -> ShellProfile {
    ShellProfile {
        id: id.into(),
        name: name.into(),
        kind: kind.into(),
        program: program.into(),
        args: args.iter().map(|s| s.to_string()).collect(),
    }
}

pub fn profiles() -> &'static [ShellProfile] {
    static PROFILES: OnceLock<Vec<ShellProfile>> = OnceLock::new();
    PROFILES.get_or_init(detect)
}

pub fn find(id: &str) -> Option<ShellProfile> {
    profiles().iter().find(|p| p.id == id).cloned()
}

#[cfg(windows)]
fn detect() -> Vec<ShellProfile> {
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut list = Vec::new();

    let pwsh = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .map(|dir| dir.join("pwsh.exe"))
        .chain([PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe")])
        .find(|p| p.is_file());
    if let Some(pwsh) = pwsh {
        list.push(profile("pwsh", "PowerShell 7", "powershell", &pwsh.to_string_lossy(), &["-NoLogo"]));
    }
    list.push(profile("powershell", "Windows PowerShell", "powershell", "powershell.exe", &["-NoLogo"]));
    list.push(profile("cmd", "Prompt de Comando", "cmd", "cmd.exe", &[]));

    // `wsl -l -q` responde em UTF-16LE.
    if let Ok(out) = std::process::Command::new("wsl.exe")
        .args(["-l", "-q"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        let utf16: Vec<u16> = out.stdout.as_chunks::<2>().0.iter().map(|&b| u16::from_le_bytes(b)).collect();
        for distro in String::from_utf16_lossy(&utf16).lines() {
            let distro = distro.trim_matches(|c: char| c.is_whitespace() || c == '\0');
            if distro.is_empty() || distro.starts_with("docker-desktop") {
                continue;
            }
            list.push(profile(
                &format!("wsl:{distro}"),
                &format!("WSL · {distro}"),
                "wsl",
                "wsl.exe",
                &["-d", distro, "--cd", "~"],
            ));
        }
    }
    list
}

#[cfg(not(windows))]
fn detect() -> Vec<ShellProfile> {
    let mut paths: Vec<String> = Vec::new();
    if let Ok(sh) = std::env::var("SHELL") {
        paths.push(sh);
    }
    if let Ok(etc) = std::fs::read_to_string("/etc/shells") {
        for line in etc.lines().map(str::trim) {
            if !line.starts_with('#') && !line.is_empty() && !paths.iter().any(|p| p == line) {
                paths.push(line.to_string());
            }
        }
    }
    let mut seen_names = std::collections::HashSet::new();
    paths
        .into_iter()
        .filter(|p| std::path::Path::new(p).is_file())
        .filter_map(|p| {
            let name = p.rsplit('/').next()?.to_string();
            // Evita duplicatas tipo /bin/bash e /usr/bin/bash.
            if !["bash", "zsh", "fish", "sh", "nu", "pwsh"].contains(&name.as_str()) || !seen_names.insert(name.clone()) {
                return None;
            }
            Some(profile(&name, &name, "unix", &p, &["-l"]))
        })
        .collect()
}
