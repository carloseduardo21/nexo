//! Kubernetes: contextos do kubeconfig, listagem de recursos e YAML.
//!
//! As listas usam o formato `Table` do servidor (o mesmo do `kubectl get`): o cluster já
//! devolve as colunas de cada tipo, inclusive as calculadas (Ready, Status, Restarts…).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use kube::config::{KubeConfigOptions, Kubeconfig};
use kube::{Client, Config};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

/// Mais que isso numa lista é truncado (a interface avisa).
const LIST_LIMIT: u32 = 2000;

/// Clientes já autenticados, por kubeconfig + contexto.
#[derive(Default)]
pub struct Kube {
    clients: Mutex<HashMap<String, Client>>,
}

impl Kube {
    pub async fn forget(&self) {
        self.clients.lock().await.clear();
    }

    pub async fn client(&self, source: Option<&str>, context: &str) -> Result<Client, String> {
        let key = format!("{}\0{context}", source.unwrap_or(""));
        if let Some(client) = self.clients.lock().await.get(&key) {
            return Ok(client.clone());
        }
        let (kubeconfig, _) = read(source)?;
        let options = KubeConfigOptions { context: Some(context.to_owned()), ..Default::default() };
        let mut config = Config::from_custom_kubeconfig(kubeconfig, &options)
            .await
            .map_err(|e| format!("contexto {context}: {e}"))?;
        // A interface consulta a cada poucos segundos: melhor falhar rápido que travar.
        config.connect_timeout = Some(Duration::from_secs(10));
        config.read_timeout = Some(Duration::from_secs(20));
        let client = Client::try_from(config).map_err(|e| format!("contexto {context}: {e}"))?;
        self.clients.lock().await.insert(key, client.clone());
        Ok(client)
    }
}

/// Lê o kubeconfig: o arquivo escolhido ou, sem ele, `KUBECONFIG` / `~/.kube/config`.
/// Devolve também de onde veio (para mostrar na interface).
fn read(source: Option<&str>) -> Result<(Kubeconfig, String), String> {
    if let Some(path) = source {
        let config = Kubeconfig::read_from(path).map_err(|e| format!("kubeconfig {path}: {e}"))?;
        return Ok((config, path.to_owned()));
    }
    if let Some(value) = std::env::var_os("KUBECONFIG").filter(|v| !v.is_empty()) {
        let shown = value.to_string_lossy().into_owned();
        let config = Kubeconfig::read().map_err(|e| format!("KUBECONFIG ({shown}): {e}"))?;
        return Ok((config, shown));
    }
    let path = default_path().ok_or("pasta do usuário não encontrada")?;
    let shown = path.display().to_string();
    if !path.exists() {
        return Err(format!("nenhum kubeconfig em {shown}"));
    }
    let config = Kubeconfig::read_from(&path).map_err(|e| format!("kubeconfig {shown}: {e}"))?;
    Ok((config, shown))
}

fn default_path() -> Option<PathBuf> {
    std::env::home_dir().map(|h| h.join(".kube").join("config"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Contexts {
    /// Arquivo (ou lista do `KUBECONFIG`) de onde vieram.
    pub source: String,
    pub current: Option<String>,
    pub contexts: Vec<ContextInfo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextInfo {
    pub name: String,
    pub cluster: String,
    pub user: String,
    /// Namespace padrão do contexto ('' = default).
    pub namespace: String,
    pub server: String,
}

pub fn contexts(source: Option<&str>) -> Result<Contexts, String> {
    let (config, source) = read(source)?;
    let server = |cluster: &str| {
        config
            .clusters
            .iter()
            .find(|c| c.name == cluster)
            .and_then(|c| c.cluster.as_ref()?.server.clone())
            .unwrap_or_default()
    };
    let contexts = config
        .contexts
        .iter()
        .filter_map(|named| {
            let context = named.context.as_ref()?;
            Some(ContextInfo {
                name: named.name.clone(),
                cluster: context.cluster.clone(),
                user: context.user.clone().unwrap_or_default(),
                namespace: context.namespace.clone().unwrap_or_default(),
                server: server(&context.cluster),
            })
        })
        .collect();
    Ok(Contexts { source, current: config.current_context.clone(), contexts })
}

/// Tipos que a interface sabe listar. Cada um vira um caminho fixo da API: o frontend
/// nunca manda URL.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Pod,
    Deployment,
    StatefulSet,
    DaemonSet,
    ReplicaSet,
    Job,
    CronJob,
    Service,
    Ingress,
    NetworkPolicy,
    ConfigMap,
    Secret,
    ServiceAccount,
    HorizontalPodAutoscaler,
    PersistentVolumeClaim,
    PersistentVolume,
    StorageClass,
    Node,
    Namespace,
    Event,
}

struct ApiPath {
    group: &'static str,
    version: &'static str,
    plural: &'static str,
    namespaced: bool,
}

impl Kind {
    fn api(self) -> ApiPath {
        let (group, version, plural, namespaced) = match self {
            Kind::Pod => ("", "v1", "pods", true),
            Kind::Deployment => ("apps", "v1", "deployments", true),
            Kind::StatefulSet => ("apps", "v1", "statefulsets", true),
            Kind::DaemonSet => ("apps", "v1", "daemonsets", true),
            Kind::ReplicaSet => ("apps", "v1", "replicasets", true),
            Kind::Job => ("batch", "v1", "jobs", true),
            Kind::CronJob => ("batch", "v1", "cronjobs", true),
            Kind::Service => ("", "v1", "services", true),
            Kind::Ingress => ("networking.k8s.io", "v1", "ingresses", true),
            Kind::NetworkPolicy => ("networking.k8s.io", "v1", "networkpolicies", true),
            Kind::ConfigMap => ("", "v1", "configmaps", true),
            Kind::Secret => ("", "v1", "secrets", true),
            Kind::ServiceAccount => ("", "v1", "serviceaccounts", true),
            Kind::HorizontalPodAutoscaler => ("autoscaling", "v2", "horizontalpodautoscalers", true),
            Kind::PersistentVolumeClaim => ("", "v1", "persistentvolumeclaims", true),
            Kind::PersistentVolume => ("", "v1", "persistentvolumes", false),
            Kind::StorageClass => ("storage.k8s.io", "v1", "storageclasses", false),
            Kind::Node => ("", "v1", "nodes", false),
            Kind::Namespace => ("", "v1", "namespaces", false),
            Kind::Event => ("", "v1", "events", true),
        };
        ApiPath { group, version, plural, namespaced }
    }

    /// Caminho da coleção (`namespace` None = todos, ou tipo sem namespace).
    fn path(self, namespace: Option<&str>) -> Result<String, String> {
        let api = self.api();
        let base = if api.group.is_empty() {
            format!("/api/{}", api.version)
        } else {
            format!("/apis/{}/{}", api.group, api.version)
        };
        Ok(match namespace.filter(|_| api.namespaced) {
            Some(ns) => format!("{base}/namespaces/{}/{}", checked(ns)?, api.plural),
            None => format!("{base}/{}", api.plural),
        })
    }
}

/// Nomes e namespaces viram parte da URL: só os caracteres que o Kubernetes aceita neles.
fn checked(name: &str) -> Result<&str, String> {
    let ok = !name.is_empty()
        && name.len() <= 253
        && name != "."
        && name != ".."
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | ':'));
    if ok {
        Ok(name)
    } else {
        Err(format!("nome inválido: {name:?}"))
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub name: String,
    /// string, integer, number, boolean, date
    #[serde(rename = "type")]
    pub kind: String,
    /// 0 = coluna principal; maiores só no `-o wide`.
    #[serde(default)]
    pub priority: i32,
    #[serde(default, skip_serializing)]
    pub description: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Table {
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
    /// A lista passou de `LIST_LIMIT` e veio cortada.
    pub truncated: bool,
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub name: String,
    pub namespace: String,
    pub uid: String,
    pub cells: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTable {
    #[serde(default)]
    column_definitions: Vec<Column>,
    #[serde(default)]
    rows: Vec<RawRow>,
    #[serde(default)]
    metadata: RawListMeta,
}

#[derive(Deserialize, Default)]
struct RawListMeta {
    #[serde(default, rename = "continue")]
    next: String,
}

#[derive(Deserialize)]
struct RawRow {
    #[serde(default)]
    cells: Vec<Value>,
    #[serde(default)]
    object: Option<RawObject>,
}

#[derive(Deserialize)]
struct RawObject {
    #[serde(default)]
    metadata: RawMeta,
}

#[derive(Deserialize, Default)]
struct RawMeta {
    #[serde(default)]
    name: String,
    #[serde(default)]
    namespace: String,
    #[serde(default)]
    uid: String,
}

fn parse_table(text: &str) -> Result<Table, String> {
    let raw: RawTable = serde_json::from_str(text).map_err(|e| format!("resposta inesperada do cluster: {e}"))?;
    let rows = raw
        .rows
        .into_iter()
        .map(|row| {
            let meta = row.object.map(|o| o.metadata).unwrap_or_default();
            Row { name: meta.name, namespace: meta.namespace, uid: meta.uid, cells: row.cells }
        })
        .collect();
    Ok(Table { columns: raw.column_definitions, rows, truncated: !raw.metadata.next.is_empty() })
}

fn get(path: String, accept: &str) -> Result<http::Request<Vec<u8>>, String> {
    http::Request::get(path)
        .header(http::header::ACCEPT, accept)
        .body(Vec::new())
        .map_err(|e| e.to_string())
}

pub async fn list(client: &Client, kind: Kind, namespace: Option<&str>) -> Result<Table, String> {
    let path = format!("{}?includeObject=Metadata&limit={LIST_LIMIT}", kind.path(namespace)?);
    // O `application/json` no fim é o plano B de servidores sem Table (muito antigos).
    let text = client
        .request_text(get(path, "application/json;as=Table;v=v1;g=meta.k8s.io,application/json")?)
        .await
        .map_err(describe)?;
    parse_table(&text)
}

pub async fn namespaces(client: &Client) -> Result<Vec<String>, String> {
    let list: Value = client
        .request(get("/api/v1/namespaces".into(), "application/json")?)
        .await
        .map_err(describe)?;
    let mut names: Vec<String> = list["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["metadata"]["name"].as_str().map(str::to_owned))
        .collect();
    names.sort();
    Ok(names)
}

/// O objeto em YAML, como o `kubectl get -o yaml` (sem `managedFields`). Em Secrets, os
/// valores ficam escondidos a não ser que `reveal`.
pub async fn yaml(client: &Client, kind: Kind, namespace: Option<&str>, name: &str, reveal: bool) -> Result<String, String> {
    let path = format!("{}/{}", kind.path(namespace)?, checked(name)?);
    let mut object: Value = client.request(get(path, "application/json")?).await.map_err(describe)?;
    to_yaml(&mut object, kind == Kind::Secret && !reveal)
}

pub const HIDDEN: &str = "••••••••";

fn to_yaml(object: &mut Value, hide: bool) -> Result<String, String> {
    if let Some(meta) = object.get_mut("metadata").and_then(Value::as_object_mut) {
        meta.remove("managedFields");
        // O `kubectl apply` guarda o objeto inteiro aqui (com os valores do Secret em claro).
        if hide {
            if let Some(annotations) = meta.get_mut("annotations").and_then(Value::as_object_mut) {
                if let Some(v) = annotations.get_mut("kubectl.kubernetes.io/last-applied-configuration") {
                    *v = HIDDEN.into();
                }
            }
        }
    }
    if hide {
        for field in ["data", "stringData"] {
            if let Some(data) = object.get_mut(field).and_then(Value::as_object_mut) {
                for value in data.values_mut() {
                    *value = HIDDEN.into();
                }
            }
        }
    }
    serde_saphyr::to_string(object).map_err(|e| e.to_string())
}

/// Mensagem legível para os erros mais comuns.
fn describe(error: kube::Error) -> String {
    match error {
        kube::Error::Api(status) => match status.code {
            401 => format!("credenciais recusadas pelo cluster ({})", status.message),
            403 => format!("sem permissão: {}", status.message),
            404 => format!("não encontrado: {}", status.message),
            _ if !status.message.is_empty() => status.message,
            _ => format!("erro {} do cluster", status.code),
        },
        kube::Error::Auth(e) => format!("falha na autenticação: {e}"),
        e @ (kube::Error::HyperError(_) | kube::Error::Service(_)) => format!("não foi possível falar com o cluster: {}", chain(&e)),
        e => e.to_string(),
    }
}

/// O erro com as causas (o de cima costuma ser genérico: "client error (Connect)").
fn chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(e) = source {
        let part = e.to_string();
        if !text.contains(&part) {
            text = format!("{text}: {part}");
        }
        source = e.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(Kind::Pod.path(Some("default")).unwrap(), "/api/v1/namespaces/default/pods");
        assert_eq!(Kind::Pod.path(None).unwrap(), "/api/v1/pods");
        assert_eq!(Kind::Deployment.path(Some("web")).unwrap(), "/apis/apps/v1/namespaces/web/deployments");
        // Tipo sem namespace ignora o escolhido.
        assert_eq!(Kind::Node.path(Some("web")).unwrap(), "/api/v1/nodes");
        assert!(Kind::Pod.path(Some("../secrets")).is_err());
        assert!(Kind::Pod.path(Some("a/b")).is_err());
        assert!(Kind::Pod.path(Some("a?b")).is_err());
        assert!(checked("system:node-proxier").is_ok());
        assert!(checked("..").is_err());
    }

    #[test]
    fn table() {
        let text = r#"{"kind":"Table","apiVersion":"meta.k8s.io/v1","metadata":{"resourceVersion":"1"},
          "columnDefinitions":[
            {"name":"Name","type":"string","format":"name","description":"Name must be unique","priority":0},
            {"name":"Ready","type":"string","format":"","description":"","priority":0},
            {"name":"Restarts","type":"string","format":"","description":"","priority":0},
            {"name":"IP","type":"string","format":"","description":"","priority":1}],
          "rows":[{"cells":["web-1","1/1","2 (5m ago)","10.0.0.4"],
            "object":{"kind":"PartialObjectMetadata","apiVersion":"meta.k8s.io/v1",
              "metadata":{"name":"web-1","namespace":"loja","uid":"u1","creationTimestamp":"2026-10-01T10:00:00Z"}}}]}"#;
        let table = parse_table(text).unwrap();
        assert_eq!(table.columns.len(), 4);
        assert_eq!(table.columns[3].priority, 1);
        assert!(!table.truncated);
        assert_eq!(
            table.rows[0],
            Row { name: "web-1".into(), namespace: "loja".into(), uid: "u1".into(), cells: serde_json::from_str(r#"["web-1","1/1","2 (5m ago)","10.0.0.4"]"#).unwrap() }
        );
        let cut = parse_table(r#"{"columnDefinitions":[],"rows":[],"metadata":{"continue":"abc"}}"#).unwrap();
        assert!(cut.truncated);
    }

    #[test]
    fn yaml_hides_secrets() {
        let mut secret = serde_json::json!({
            "apiVersion": "v1", "kind": "Secret",
            "metadata": {"name": "db", "managedFields": [{"manager": "kubectl"}],
              "annotations": {"kubectl.kubernetes.io/last-applied-configuration": "{\"data\":{\"password\":\"czNjcjN0\"}}"}},
            "data": {"password": "czNjcjN0"}, "type": "Opaque"
        });
        let text = to_yaml(&mut secret.clone(), true).unwrap();
        assert!(!text.contains("czNjcjN0"), "{text}");
        assert!(!text.contains("managedFields"));
        assert!(text.contains("password: ••••••••"), "{text}");
        assert!(to_yaml(&mut secret, false).unwrap().contains("czNjcjN0"));
    }

    #[test]
    fn yaml_multiline() {
        let mut config = serde_json::json!({"apiVersion": "v1", "kind": "ConfigMap", "metadata": {"name": "app"},
          "data": {"nginx.conf": "server {\n  listen 80;\n}\n"}});
        let text = to_yaml(&mut config, false).unwrap();
        println!("{text}");
        assert!(text.starts_with("apiVersion: v1\n"), "{text}");
    }
}

/// Contra o cluster do contexto atual do kubeconfig (ex.: `kind create cluster`):
/// `cargo test k8s::live -- --ignored --nocapture`.
#[cfg(test)]
mod live {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn live_list() {
        let contexts = contexts(None).unwrap();
        println!("{} contextos em {}", contexts.contexts.len(), contexts.source);
        let current = contexts.current.expect("sem contexto atual");
        let kube = Kube::default();
        let client = kube.client(None, &current).await.unwrap();
        let names = namespaces(&client).await.unwrap();
        assert!(names.contains(&"kube-system".to_owned()), "{names:?}");
        let pods = list(&client, Kind::Pod, Some("kube-system")).await.unwrap();
        println!("{:?}", pods.columns.iter().map(|c| &c.name).collect::<Vec<_>>());
        assert!(pods.columns.iter().any(|c| c.name == "Status"));
        assert!(!pods.rows.is_empty());
        assert!(pods.rows.iter().all(|r| r.namespace == "kube-system" && !r.name.is_empty()));
        let nodes = list(&client, Kind::Node, None).await.unwrap();
        assert!(!nodes.rows.is_empty());
        let all = list(&client, Kind::Deployment, None).await.unwrap();
        println!("{} deployments em todos os namespaces", all.rows.len());
        let pod = &pods.rows[0];
        let text = yaml(&client, Kind::Pod, Some("kube-system"), &pod.name, false).await.unwrap();
        assert!(text.contains(&format!("name: {}", pod.name)));
        assert!(!text.contains("managedFields"));
        let err = yaml(&client, Kind::Pod, Some("kube-system"), "nao-existe", false).await.unwrap_err();
        assert!(err.starts_with("não encontrado"), "{err}");
    }
}
