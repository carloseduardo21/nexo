//! Importação do `~/.ssh/config` do OpenSSH.
//!
//! Entende o que importa para cadastrar hosts: `Host` (com curingas e `!`), `HostName`,
//! `User`, `Port`, `IdentityFile`, `ProxyJump` e `Include`. Como no OpenSSH, vale o
//! primeiro valor encontrado, então `Host *` no fim do arquivo funciona como padrão.
//! Blocos `Match` são ignorados.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::vault::{AuthMethod, Host};

const GROUP: &str = "ssh config";
const MAX_INCLUDE_DEPTH: usize = 8;

/// Um host do arquivo, já com os padrões aplicados.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub alias: String,
    pub hostname: String,
    pub port: u16,
    pub user: String,
    pub identity_file: String,
    pub proxy_jump: String,
    /// Já existe no Nexo um host com o mesmo endereço, usuário e porta (ou o mesmo nome).
    pub exists: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scan {
    pub path: String,
    pub entries: Vec<Entry>,
}

struct Block {
    /// Vazio = bloco `Match` (nunca casa).
    patterns: Vec<String>,
    options: Vec<(String, String)>,
}

pub fn default_path() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".ssh").join("config"))
}

pub fn scan(path: &Path, existing: &[Host]) -> Result<Scan, String> {
    let entries = parse_file(path)?
        .into_iter()
        .map(|mut e| {
            e.exists = existing.iter().any(|h| same(h, &e.hostname, &e.user, e.port) || h.label == e.alias);
            e
        })
        .collect();
    Ok(Scan { path: path.display().to_string(), entries })
}

fn parse_file(path: &Path) -> Result<Vec<Entry>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("não foi possível ler {}: {e}", path.display()))?;
    let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut blocks = vec![Block { patterns: vec!["*".into()], options: vec![] }];
    read_lines(&text, &base, &mut blocks, 0);
    Ok(resolve(&blocks))
}

/// Hosts a cadastrar: os escolhidos e os jump hosts de que eles dependem.
pub fn to_hosts(path: &Path, aliases: &[String], existing: &[Host]) -> Result<Vec<Host>, String> {
    let entries = parse_file(path)?;
    let mut importer = Importer { entries: &entries, existing, ids: HashMap::new(), created: Vec::new(), visiting: HashSet::new() };
    for alias in aliases {
        let entry = entries.iter().find(|e| &e.alias == alias).ok_or_else(|| format!("{alias} não está no arquivo"))?;
        importer.import(entry)?;
    }
    Ok(importer.created)
}

struct Importer<'a> {
    entries: &'a [Entry],
    existing: &'a [Host],
    /// Alias (ou `user@host:porta`) → id do host, no Nexo ou criado agora.
    ids: HashMap<String, String>,
    created: Vec<Host>,
    visiting: HashSet<String>,
}

impl Importer<'_> {
    fn import(&mut self, entry: &Entry) -> Result<String, String> {
        if let Some(id) = self.ids.get(&entry.alias) {
            return Ok(id.clone());
        }
        if !self.visiting.insert(entry.alias.clone()) {
            return Err(format!("ProxyJump circular em {}", entry.alias));
        }
        let mut jump = String::new();
        for hop in entry.proxy_jump.split(',').map(str::trim).filter(|h| !h.is_empty()) {
            let id = self.hop(hop)?;
            // Em "a,b", o b é alcançado pelo a: liga a cadeia quando o b foi criado agora sem jump.
            if !jump.is_empty() {
                if let Some(host) = self.created.iter_mut().find(|h| h.id == id && h.jump_host_id.is_empty()) {
                    host.jump_host_id = jump.clone();
                }
            }
            jump = id;
        }
        let host = Host {
            id: uuid::Uuid::new_v4().to_string(),
            label: entry.alias.clone(),
            hostname: entry.hostname.clone(),
            port: entry.port,
            username: entry.user.clone(),
            auth: AuthMethod::Key,
            key_path: entry.identity_file.clone(),
            group: GROUP.into(),
            jump_host_id: jump,
            ..Default::default()
        };
        self.visiting.remove(&entry.alias);
        self.ids.insert(entry.alias.clone(), host.id.clone());
        self.created.push(host.clone());
        Ok(host.id)
    }

    /// Um salto do ProxyJump: alias do arquivo, host já cadastrado ou `[user@]host[:porta]` novo.
    fn hop(&mut self, spec: &str) -> Result<String, String> {
        if let Some(id) = self.ids.get(spec) {
            return Ok(id.clone());
        }
        let (user, rest) = match spec.split_once('@') {
            Some((user, rest)) => (Some(user.to_string()), rest),
            None => (None, spec),
        };
        let (name, port) = split_port(rest);
        let entry = self.entries.iter().find(|e| e.alias == name).cloned();
        let id = match entry {
            // Alias do arquivo sem sobrescrever usuário/porta: usa (ou importa) aquele host.
            Some(entry) if user.is_none() && port.is_none() => match self.find(&entry.hostname, &entry.user, entry.port) {
                Some(id) => id,
                None => self.import(&entry)?,
            },
            entry => {
                let hostname = entry.as_ref().map_or(name.to_string(), |e| e.hostname.clone());
                let user = user.or_else(|| entry.as_ref().map(|e| e.user.clone())).unwrap_or_else(default_user);
                let port = port.or(entry.as_ref().map(|e| e.port)).unwrap_or(22);
                match self.find(&hostname, &user, port) {
                    Some(id) => id,
                    None => {
                        let host = Host {
                            id: uuid::Uuid::new_v4().to_string(),
                            label: spec.to_string(),
                            hostname,
                            port,
                            username: user,
                            auth: AuthMethod::Key,
                            key_path: entry.map(|e| e.identity_file).unwrap_or_default(),
                            group: GROUP.into(),
                            ..Default::default()
                        };
                        self.created.push(host.clone());
                        host.id
                    }
                }
            }
        };
        self.ids.insert(spec.to_string(), id.clone());
        Ok(id)
    }

    fn find(&self, hostname: &str, user: &str, port: u16) -> Option<String> {
        self.existing.iter().chain(&self.created).find(|h| same(h, hostname, user, port)).map(|h| h.id.clone())
    }
}

fn same(host: &Host, hostname: &str, user: &str, port: u16) -> bool {
    host.hostname.eq_ignore_ascii_case(hostname) && host.username == user && host.port == port
}

fn split_port(s: &str) -> (&str, Option<u16>) {
    // [::1]:22 / host:22
    if let Some(rest) = s.strip_prefix('[') {
        if let Some((host, port)) = rest.split_once("]:") {
            return (host, port.parse().ok());
        }
        return (rest.trim_end_matches(']'), None);
    }
    match s.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => (host, port.parse().ok()),
        _ => (s, None),
    }
}

fn read_lines(text: &str, base: &Path, blocks: &mut Vec<Block>, depth: usize) {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.find(|c: char| c.is_whitespace() || c == '=') {
            Some(i) => (&line[..i], line[i..].trim_start_matches(|c: char| c.is_whitespace() || c == '=').trim()),
            None => (line, ""),
        };
        let key = key.to_ascii_lowercase();
        let args = split_args(value);
        match key.as_str() {
            "host" => blocks.push(Block { patterns: args, options: vec![] }),
            "match" => blocks.push(Block { patterns: vec![], options: vec![] }),
            "include" if depth < MAX_INCLUDE_DEPTH => {
                for pattern in &args {
                    for file in expand_include(pattern, base) {
                        if let Ok(text) = std::fs::read_to_string(&file) {
                            read_lines(&text, base, blocks, depth + 1);
                        }
                    }
                }
            }
            _ => {
                if let (Some(block), Some(first)) = (blocks.last_mut(), args.into_iter().next()) {
                    block.options.push((key, first));
                }
            }
        }
    }
}

/// Divide argumentos respeitando aspas: `IdentityFile "C:\Minhas Chaves\id"`.
fn split_args(value: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in value.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

/// `Include` aceita curingas no nome do arquivo; caminhos relativos partem de ~/.ssh.
fn expand_include(pattern: &str, base: &Path) -> Vec<PathBuf> {
    let path = expand_home(pattern);
    let path = if path.is_absolute() { path } else { base.join(path) };
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !name.contains(['*', '?']) {
        return vec![path];
    }
    let Some(dir) = path.parent() else { return vec![] };
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| glob(&name, &e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    files
}

fn resolve(blocks: &[Block]) -> Vec<Entry> {
    let mut seen = HashSet::new();
    let aliases: Vec<&str> = blocks
        .iter()
        .flat_map(|b| &b.patterns)
        .map(String::as_str)
        .filter(|p| !p.contains(['*', '?', '!']) && seen.insert(*p))
        .collect();

    aliases
        .into_iter()
        .map(|alias| {
            let mut options: HashMap<&str, &str> = HashMap::new();
            for block in blocks.iter().filter(|b| matches(&b.patterns, alias)) {
                for (key, value) in &block.options {
                    options.entry(key.as_str()).or_insert(value.as_str());
                }
            }
            let hostname = options.get("hostname").map_or(alias.to_string(), |h| h.replace("%h", alias));
            let proxy_jump = options.get("proxyjump").filter(|j| !j.eq_ignore_ascii_case("none")).map_or(String::new(), |j| j.to_string());
            Entry {
                alias: alias.to_string(),
                hostname,
                port: options.get("port").and_then(|p| p.parse().ok()).unwrap_or(22),
                user: options.get("user").map_or_else(default_user, |u| u.to_string()),
                identity_file: options.get("identityfile").map_or(String::new(), |f| f.to_string()),
                proxy_jump,
                exists: false,
            }
        })
        .collect()
}

fn matches(patterns: &[String], alias: &str) -> bool {
    let mut hit = false;
    for pattern in patterns {
        match pattern.strip_prefix('!') {
            Some(negated) if glob(negated, alias) => return false,
            Some(_) => {}
            None => hit |= glob(pattern, alias),
        }
    }
    hit
}

/// Curingas do ssh_config: `*` (qualquer sequência) e `?` (um caractere), sem diferenciar maiúsculas.
fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

fn default_user() -> String {
    std::env::var(if cfg!(windows) { "USERNAME" } else { "USER" }).unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Vec<Entry> {
        let mut blocks = vec![Block { patterns: vec!["*".into()], options: vec![] }];
        read_lines(text, Path::new("."), &mut blocks, 0);
        resolve(&blocks)
    }

    const CONFIG: &str = r#"
# comentário
Host bastion
    HostName 203.0.113.10
    User ops
    IdentityFile "~/.ssh/minha chave"

Host web db
    HostName %h.interno
    ProxyJump bastion

Host db
    Port 2200
    User postgres

Host *.exemplo.com !proibido.exemplo.com
    User dev

Match host foo
    User ninguem

Host *
    User padrao
    Port=2222
"#;

    #[test]
    fn parses_hosts_with_defaults() {
        let entries = parse(CONFIG);
        let names: Vec<_> = entries.iter().map(|e| e.alias.as_str()).collect();
        assert_eq!(names, ["bastion", "web", "db"]);

        let bastion = &entries[0];
        assert_eq!((bastion.hostname.as_str(), bastion.user.as_str(), bastion.port), ("203.0.113.10", "ops", 2222));
        assert_eq!(bastion.identity_file, "~/.ssh/minha chave");

        let web = &entries[1];
        assert_eq!((web.hostname.as_str(), web.user.as_str(), web.proxy_jump.as_str()), ("web.interno", "padrao", "bastion"));

        // O primeiro valor vence: "Host web db" não define porta, então vale a do bloco "Host db".
        let db = &entries[2];
        assert_eq!((db.hostname.as_str(), db.user.as_str(), db.port), ("db.interno", "postgres", 2200));
    }

    #[test]
    fn globs() {
        assert!(glob("*.exemplo.com", "a.EXEMPLO.com"));
        assert!(glob("web-??", "web-01"));
        assert!(!glob("web-??", "web-1"));
        assert!(matches(&["*.exemplo.com".into(), "!proibido.exemplo.com".into()], "ok.exemplo.com"));
        assert!(!matches(&["*.exemplo.com".into(), "!proibido.exemplo.com".into()], "proibido.exemplo.com"));
    }

    #[test]
    fn imports_jump_chain() {
        let dir = std::env::temp_dir().join(format!("nexo-sshconfig-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config");
        std::fs::write(&path, "Host alvo\n  HostName 10.0.0.5\n  User app\n  ProxyJump gw1,root@gw2:2022\nHost gw1\n  HostName gw1.exemplo.com\n  User eu\n").unwrap();

        let hosts = to_hosts(&path, &["alvo".into()], &[]).unwrap();
        std::fs::remove_dir_all(&dir).ok();

        let by_label = |l: &str| hosts.iter().find(|h| h.label == l).unwrap();
        let (alvo, gw1, gw2) = (by_label("alvo"), by_label("gw1"), by_label("root@gw2:2022"));
        assert_eq!(alvo.jump_host_id, gw2.id);
        assert_eq!(gw2.jump_host_id, gw1.id);
        assert!(gw1.jump_host_id.is_empty());
        assert_eq!((gw2.hostname.as_str(), gw2.username.as_str(), gw2.port), ("gw2", "root", 2022));
        assert_eq!(hosts.len(), 3);
    }
}
