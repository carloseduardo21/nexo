//! Métricas do servidor de uma sessão SSH (CPU, memória, disco, rede).
//! Um script curto lê `/proc` e `df` num canal extra da conexão do terminal: nada é instalado
//! no servidor. Contadores (CPU, rede) vêm brutos; o frontend calcula as taxas entre amostras.

use serde::Serialize;

/// Seções separadas por `@nome`. `LC_ALL=C`: números e cabeçalhos sem tradução.
pub const METRICS_SCRIPT: &str = "export LC_ALL=C
[ -r /proc/stat ] || { echo 'sem /proc' >&2; exit 3; }
echo @stat; head -n1 /proc/stat
echo @mem; cat /proc/meminfo
echo @load; cat /proc/loadavg
echo @uptime; cat /proc/uptime
echo @cpus; nproc 2>/dev/null || grep -c ^processor /proc/cpuinfo
echo @host; hostname; uname -sr; (. /etc/os-release 2>/dev/null && echo \"$PRETTY_NAME\")
echo @net; cat /proc/net/dev
echo @disk; df -P -k 2>/dev/null
";

#[derive(Serialize, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub hostname: String,
    pub kernel: String,
    pub os: String,
    pub cpus: u32,
    /// Jiffies desde o boot: total e ocioso (idle + iowait).
    pub cpu_total: u64,
    pub cpu_idle: u64,
    pub load: [f64; 3],
    pub uptime: u64,
    /// Bytes.
    pub mem_total: u64,
    pub mem_available: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    /// Bytes recebidos/enviados desde o boot, somando as interfaces físicas.
    pub net_rx: u64,
    pub net_tx: u64,
    pub disks: Vec<Disk>,
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Disk {
    pub mount: String,
    pub device: String,
    /// Bytes.
    pub total: u64,
    pub used: u64,
}

pub fn parse(output: &str) -> Result<Metrics, String> {
    let mut m = Metrics::default();
    let mut section = "";
    let mut host_lines = 0;
    for line in output.lines() {
        if let Some(name) = line.strip_prefix('@') {
            section = name.trim();
            continue;
        }
        match section {
            "stat" => {
                // cpu  user nice system idle iowait irq softirq steal …
                let values: Vec<u64> = line.split_whitespace().skip(1).filter_map(|v| v.parse().ok()).collect();
                // guest/guest_nice (posições 8 e 9) já estão contados em user/nice.
                m.cpu_total = values.iter().take(8).sum();
                m.cpu_idle = values.get(3).copied().unwrap_or(0) + values.get(4).copied().unwrap_or(0);
            }
            "mem" => {
                let mut parts = line.split_whitespace();
                let (Some(key), Some(value)) = (parts.next(), parts.next().and_then(|v| v.parse::<u64>().ok())) else { continue };
                let bytes = value * 1024;
                match key {
                    "MemTotal:" => m.mem_total = bytes,
                    "MemAvailable:" => m.mem_available = bytes,
                    "SwapTotal:" => m.swap_total = bytes,
                    "SwapFree:" => m.swap_free = bytes,
                    _ => {}
                }
            }
            "load" => {
                for (slot, v) in m.load.iter_mut().zip(line.split_whitespace()) {
                    *slot = v.parse().unwrap_or(0.0);
                }
            }
            "uptime" => m.uptime = line.split_whitespace().next().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) as u64,
            "cpus" => m.cpus = line.trim().parse().unwrap_or(0),
            "host" => {
                match host_lines {
                    0 => m.hostname = line.trim().to_string(),
                    1 => m.kernel = line.trim().to_string(),
                    _ => m.os = line.trim().to_string(),
                }
                host_lines += 1;
            }
            "net" => {
                // `  eth0: rx_bytes rx_packets … (8 campos de rx) tx_bytes …`
                let Some((iface, data)) = line.split_once(':') else { continue };
                if !is_physical(iface.trim()) {
                    continue;
                }
                let fields: Vec<u64> = data.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                if fields.len() >= 9 {
                    m.net_rx += fields[0];
                    m.net_tx += fields[8];
                }
            }
            "disk" => {
                // Filesystem 1024-blocks Used Available Capacity Mounted-on
                let fields: Vec<&str> = line.split_whitespace().collect();
                if fields.len() < 6 || fields[0] == "Filesystem" {
                    continue;
                }
                let (Ok(total), Ok(used)) = (fields[1].parse::<u64>(), fields[2].parse::<u64>()) else { continue };
                let device = fields[0].to_string();
                let mount = fields[5..].join(" ");
                if is_real_disk(&device, &mount, total) && !m.disks.iter().any(|d| d.device == device) {
                    m.disks.push(Disk { mount, device, total: total * 1024, used: used * 1024 });
                }
            }
            _ => {}
        }
    }
    if m.mem_total == 0 || m.cpu_total == 0 {
        return Err("resposta inesperada do servidor".into());
    }
    m.disks.sort_by(|a, b| (a.mount != "/").cmp(&(b.mount != "/")).then_with(|| a.mount.cmp(&b.mount)));
    Ok(m)
}

/// Interfaces de verdade: sem loopback nem as virtuais de containers/VMs.
fn is_physical(iface: &str) -> bool {
    const VIRTUAL: [&str; 9] = ["lo", "veth", "docker", "br-", "virbr", "cni", "flannel", "cali", "vnet"];
    !VIRTUAL.iter().any(|p| iface.starts_with(p))
}

/// Discos que interessam: com dispositivo de bloco (ou rede) e fora de pseudo-sistemas.
fn is_real_disk(device: &str, mount: &str, total: u64) -> bool {
    const SKIP_MOUNTS: [&str; 6] = ["/dev", "/proc", "/sys", "/run", "/snap", "/var/lib/docker"];
    total > 0
        && (device.starts_with('/') || device.contains(":/") || device.starts_with("//"))
        && !device.starts_with("/dev/loop")
        && !SKIP_MOUNTS.iter().any(|p| mount == *p || mount.starts_with(&format!("{p}/")))
        && mount != "/boot/efi"
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "@stat
cpu  4705 150 1120 16250 520 0 30 10 0 0
@mem
MemTotal:        2014236 kB
MemFree:          120000 kB
MemAvailable:    1012118 kB
SwapTotal:        524284 kB
SwapFree:         524000 kB
@load
0.52 0.40 0.31 1/123 4567
@uptime
93784.12 180000.00
@cpus
4
@host
web-01
Linux 6.1.0-26-amd64
Debian GNU/Linux 12 (bookworm)
@net
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo:  999999     100    0    0    0     0          0         0   999999     100    0    0    0     0       0          0
  eth0: 1000000    2000    0    0    0     0          0         0   500000    1500    0    0    0     0       0          0
docker0:  777777    10    0    0    0     0          0         0   777777    10    0    0    0     0       0          0
vethab12:  1234    10    0    0    0     0          0         0   1234    10    0    0    0     0       0          0
@disk
Filesystem     1024-blocks    Used Available Capacity Mounted on
udev               1000000       0   1000000       0% /dev
tmpfs               201424     900    200524       1% /run
/dev/sda1         30832548 9123456  20123456      32% /
/dev/sda1         30832548 9123456  20123456      32% /var/lib/docker/overlay2
/dev/loop0           65536   65536         0     100% /snap/core/123
/dev/sdb1        103081248 2000000  95823456       3% /mnt/data storage
/dev/sda15          126678    6182    120496       5% /boot/efi
nas:/export       1000000  500000    500000      50% /mnt/nas
";

    #[test]
    fn parses_metrics() {
        let m = parse(SAMPLE).unwrap();
        assert_eq!(m.hostname, "web-01");
        assert_eq!(m.kernel, "Linux 6.1.0-26-amd64");
        assert_eq!(m.os, "Debian GNU/Linux 12 (bookworm)");
        assert_eq!(m.cpus, 4);
        assert_eq!(m.cpu_total, 4705 + 150 + 1120 + 16250 + 520 + 30 + 10);
        assert_eq!(m.cpu_idle, 16250 + 520);
        assert_eq!(m.load, [0.52, 0.40, 0.31]);
        assert_eq!(m.uptime, 93784);
        assert_eq!(m.mem_total, 2014236 * 1024);
        assert_eq!(m.mem_available, 1012118 * 1024);
        assert_eq!(m.swap_total - m.swap_free, 284 * 1024);
        // Só eth0: lo, docker0 e veth ficam de fora.
        assert_eq!((m.net_rx, m.net_tx), (1000000, 500000));
        let mounts: Vec<_> = m.disks.iter().map(|d| d.mount.as_str()).collect();
        assert_eq!(mounts, ["/", "/mnt/data storage", "/mnt/nas"]);
        assert_eq!(m.disks[0].used, 9123456 * 1024);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("command not found").is_err());
    }
}

/// Contra o container openssh dos outros testes (porta 2222 ou `NEXO_SSH_PORT`):
/// `cargo test -- --ignored live_metrics`.
#[cfg(test)]
pub(crate) mod live {
    use super::*;
    use crate::ssh::{self, ConnectError, ConnectOptions, Connection, Failure, Options};
    use crate::vault::{AuthMethod, Host, Secret};

    /// Conecta com usuário/senha, confiando na chave do servidor na primeira vez.
    pub async fn connect(port_var: &str, default_port: u16) -> Connection {
        let host = Host {
            id: "t".into(),
            hostname: "127.0.0.1".into(),
            port: std::env::var(port_var).ok().and_then(|p| p.parse().ok()).unwrap_or(default_port),
            username: "nexo".into(),
            auth: AuthMethod::Password,
            ..Default::default()
        };
        let route = vec![(host, Secret { password: Some("nexo123".into()), ..Default::default() })];
        let mut opts = Options::new();
        loop {
            match ssh::establish(&route, &opts).await {
                Ok(conn) => return conn,
                Err(Failure { host_id, error: ConnectError::UnknownHost { fingerprint, .. } }) => {
                    opts.insert(host_id, ConnectOptions { trust_fingerprint: Some(fingerprint), ..Default::default() });
                }
                Err(e) => panic!("{:?}", e.error),
            }
        }
    }

    #[tokio::test]
    #[ignore]
    async fn live_metrics() {
        let conn = connect("NEXO_SSH_PORT", 2222).await;
        let out = conn.run(METRICS_SCRIPT).await.unwrap();
        assert_eq!(out.status, Some(0), "{}", out.stderr());
        let m = parse(&out.stdout()).unwrap();
        println!("{m:#?}");
        assert!(m.cpus > 0 && m.mem_available > 0 && m.mem_available <= m.mem_total);
        assert!(!m.hostname.is_empty());
        assert!(m.disks.iter().any(|d| d.mount == "/" || d.total > 0), "nenhum disco: {:?}", m.disks);
        conn.close().await;
    }
}
