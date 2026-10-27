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
    pub forwards: crate::k8s_forward::Forwards,
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
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
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

/// Um tipo conhecido ou um custom resource (de um CRD do cluster).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Resource {
    Builtin(Kind),
    Custom(Custom),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Custom {
    pub group: String,
    pub version: String,
    pub plural: String,
    pub namespaced: bool,
}

impl From<Kind> for Resource {
    fn from(kind: Kind) -> Self {
        Resource::Builtin(kind)
    }
}

impl Resource {
    fn path(&self, namespace: Option<&str>) -> Result<String, String> {
        match self {
            Resource::Builtin(kind) => kind.path(namespace),
            Resource::Custom(c) => {
                let base = format!("/apis/{}/{}", checked(&c.group)?, checked(&c.version)?);
                Ok(match namespace.filter(|_| c.namespaced) {
                    Some(ns) => format!("{base}/namespaces/{}/{}", checked(ns)?, checked(&c.plural)?),
                    None => format!("{base}/{}", checked(&c.plural)?),
                })
            }
        }
    }

    fn is(&self, kind: Kind) -> bool {
        *self == Resource::Builtin(kind)
    }
}

/// Caminho de um objeto.
pub(crate) fn object_path(resource: &Resource, namespace: Option<&str>, name: &str) -> Result<String, String> {
    Ok(format!("{}/{}", resource.path(namespace)?, checked(name)?))
}

/// Nomes e namespaces viram parte da URL: só os caracteres que o Kubernetes aceita neles.
pub(crate) fn checked(name: &str) -> Result<&str, String> {
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

pub async fn list(client: &Client, resource: &Resource, namespace: Option<&str>) -> Result<Table, String> {
    let path = format!("{}?includeObject=Metadata&limit={LIST_LIMIT}", resource.path(namespace)?);
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
pub async fn yaml(client: &Client, resource: &Resource, namespace: Option<&str>, name: &str, reveal: bool) -> Result<String, String> {
    let mut object = get_json(client, object_path(resource, namespace, name)?).await?;
    to_yaml(&mut object, resource.is(Kind::Secret) && !reveal)
}

/// Substitui o objeto pelo YAML editado (PUT). O `resourceVersion` que veio no YAML faz o
/// servidor recusar se alguém mudou o objeto nesse meio-tempo. Devolve o YAML novo.
pub async fn apply(client: &Client, resource: &Resource, namespace: Option<&str>, name: &str, text: &str) -> Result<String, String> {
    let object = edited(resource, namespace, name, text)?;
    let request = http::Request::put(object_path(resource, namespace, name)?)
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::ACCEPT, "application/json")
        .body(object.to_string().into_bytes())
        .map_err(|e| e.to_string())?;
    let mut saved: Value = client.request(request).await.map_err(|e| match e {
        kube::Error::Api(status) if status.code == 409 => {
            "o objeto mudou desde que foi aberto: recarregue o YAML e edite de novo".to_owned()
        }
        e => describe(e),
    })?;
    to_yaml(&mut saved, false)
}

/// Lê o YAML editado e confere que ainda é o mesmo objeto (o nome e o namespace não mudam).
fn edited(resource: &Resource, namespace: Option<&str>, name: &str, text: &str) -> Result<Value, String> {
    let mut object: Value = serde_saphyr::from_str(text).map_err(|e| format!("YAML inválido: {e}"))?;
    if !object.is_object() {
        return Err("o YAML precisa ser um objeto".into());
    }
    if object["metadata"]["name"].as_str() != Some(name) {
        return Err(format!("o nome não pode mudar (era {name})"));
    }
    if let Some(ns) = namespace {
        match object["metadata"]["namespace"].as_str() {
            Some(other) if other != ns => return Err(format!("o namespace não pode mudar (era {ns})")),
            Some(_) => {}
            None => object["metadata"]["namespace"] = ns.into(),
        }
    }
    if resource.is(Kind::Secret) && text.contains(HIDDEN) {
        return Err("revele os valores do Secret antes de editar".into());
    }
    Ok(object)
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

pub(crate) async fn get_json(client: &Client, path: String) -> Result<Value, String> {
    client.request(get(path, "application/json")?).await.map_err(describe)
}

async fn send(client: &Client, request: http::Request<Vec<u8>>) -> Result<(), String> {
    client.request::<Value>(request).await.map(drop).map_err(describe)
}

fn merge_patch(path: String, body: &Value) -> Result<http::Request<Vec<u8>>, String> {
    http::Request::patch(path)
        .header(http::header::CONTENT_TYPE, "application/merge-patch+json")
        .header(http::header::ACCEPT, "application/json")
        .body(body.to_string().into_bytes())
        .map_err(|e| e.to_string())
}

/// Ações sobre um objeto. Excluir não vale para o que derruba o cluster inteiro de uma vez
/// (namespaces, nodes, volumes, storage classes): isso fica com o `kubectl`.
#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Action {
    Delete,
    Scale { replicas: u32 },
    /// Como o `kubectl rollout restart`: muda uma anotação do template e os pods são recriados.
    Restart,
}

pub async fn act(client: &Client, resource: &Resource, namespace: Option<&str>, name: &str, action: Action) -> Result<(), String> {
    let path = object_path(resource, namespace, name)?;
    let kind = match resource {
        Resource::Builtin(kind) => Some(*kind),
        Resource::Custom(_) => None,
    };
    match action {
        Action::Delete => {
            if matches!(kind, Some(Kind::Namespace | Kind::Node | Kind::PersistentVolume | Kind::StorageClass)) {
                return Err("excluir este tipo só pelo kubectl".into());
            }
            let request = http::Request::delete(path)
                .header(http::header::ACCEPT, "application/json")
                .body(Vec::new())
                .map_err(|e| e.to_string())?;
            send(client, request).await
        }
        Action::Scale { replicas } => {
            if !matches!(kind, Some(Kind::Deployment | Kind::StatefulSet | Kind::ReplicaSet)) {
                return Err("este tipo não escala".into());
            }
            send(client, merge_patch(format!("{path}/scale"), &serde_json::json!({ "spec": { "replicas": replicas } }))?).await
        }
        Action::Restart => {
            if !matches!(kind, Some(Kind::Deployment | Kind::StatefulSet | Kind::DaemonSet)) {
                return Err("este tipo não reinicia".into());
            }
            let now = k8s_openapi::jiff::Timestamp::now().to_string();
            let body = serde_json::json!({ "spec": { "template": { "metadata": { "annotations": {
                "kubectl.kubernetes.io/restartedAt": now } } } } });
            send(client, merge_patch(path, &body)?).await
        }
    }
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContainerInfo {
    pub name: String,
    pub init: bool,
}

/// Containers de um pod (os de init por último) e o padrão (anotação
/// `kubectl.kubernetes.io/default-container` ou o primeiro).
pub async fn containers(client: &Client, namespace: &str, pod: &str) -> Result<(Vec<ContainerInfo>, String), String> {
    let object = get_json(client, object_path(&Kind::Pod.into(), Some(namespace), pod)?).await?;
    Ok(pod_containers(&object))
}

fn pod_containers(pod: &Value) -> (Vec<ContainerInfo>, String) {
    let names = |field: &str| -> Vec<String> {
        pod["spec"][field].as_array().into_iter().flatten().filter_map(|c| c["name"].as_str().map(str::to_owned)).collect()
    };
    let mut list: Vec<ContainerInfo> = names("containers").into_iter().map(|name| ContainerInfo { name, init: false }).collect();
    list.extend(names("initContainers").into_iter().map(|name| ContainerInfo { name, init: true }));
    let annotated = pod["metadata"]["annotations"]["kubectl.kubernetes.io/default-container"].as_str();
    let default = annotated
        .filter(|a| list.iter().any(|c| c.name == *a))
        .map(str::to_owned)
        .or_else(|| list.first().map(|c| c.name.clone()))
        .unwrap_or_default();
    (list, default)
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Port {
    pub port: u16,
    pub name: String,
    pub protocol: String,
}

/// Tipos em que dá para abrir um port-forward.
pub fn forwardable(kind: Kind) -> bool {
    matches!(kind, Kind::Pod | Kind::Service | Kind::Deployment | Kind::StatefulSet | Kind::DaemonSet | Kind::ReplicaSet)
}

/// Portas conhecidas do objeto: as dos containers ou, num Service, as do Service.
pub async fn ports(client: &Client, kind: Kind, namespace: &str, name: &str) -> Result<Vec<Port>, String> {
    if !forwardable(kind) {
        return Err("port-forward só em pods, services e workloads".into());
    }
    let object = get_json(client, object_path(&kind.into(), Some(namespace), name)?).await?;
    Ok(object_ports(kind, &object))
}

fn object_ports(kind: Kind, object: &Value) -> Vec<Port> {
    let port = |p: &Value, field: &str| {
        Some(Port {
            port: u16::try_from(p[field].as_u64()?).ok()?,
            name: p["name"].as_str().unwrap_or_default().to_owned(),
            protocol: p["protocol"].as_str().unwrap_or("TCP").to_owned(),
        })
    };
    let mut list: Vec<Port> = if kind == Kind::Service {
        object["spec"]["ports"].as_array().into_iter().flatten().filter_map(|p| port(p, "port")).collect()
    } else {
        let spec = if kind == Kind::Pod { &object["spec"] } else { &object["spec"]["template"]["spec"] };
        spec["containers"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|c| c["ports"].as_array().into_iter().flatten())
            .filter_map(|p| port(p, "containerPort"))
            .collect()
    };
    list.dedup_by_key(|p| p.port);
    list
}

/// Para onde vai um port-forward: o pod e a porta nele. Services e workloads escolhem um pod
/// pronto pelo seletor (como o `kubectl port-forward svc/x`).
pub async fn resolve(client: &Client, kind: Kind, namespace: &str, name: &str, port: u16) -> Result<(String, u16), String> {
    if kind == Kind::Pod {
        return Ok((checked(name)?.to_owned(), port));
    }
    if !forwardable(kind) {
        return Err("port-forward só em pods, services e workloads".into());
    }
    let object = get_json(client, object_path(&kind.into(), Some(namespace), name)?).await?;
    let (selector, target) = if kind == Kind::Service {
        let entry = object["spec"]["ports"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|p| p["port"].as_u64() == Some(port.into()))
            .ok_or_else(|| format!("o service {name} não tem a porta {port}"))?;
        (&object["spec"]["selector"], entry["targetPort"].clone())
    } else {
        (&object["spec"]["selector"]["matchLabels"], Value::from(port))
    };
    let selector = label_selector(selector).ok_or_else(|| format!("{name} não tem seletor de pods"))?;
    let pods = get_json(
        client,
        format!("/api/v1/namespaces/{}/pods?labelSelector={}", checked(namespace)?, encode(&selector)),
    )
    .await?;
    let pod = pods["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["metadata"]["deletionTimestamp"].is_null() && p["status"]["phase"] == "Running")
        // Os prontos primeiro.
        .max_by_key(|p| {
            p["status"]["conditions"].as_array().into_iter().flatten().any(|c| c["type"] == "Ready" && c["status"] == "True")
        })
        .ok_or_else(|| format!("nenhum pod rodando para {name}"))?;
    let pod_name = pod["metadata"]["name"].as_str().unwrap_or_default().to_owned();
    let pod_port = match target {
        Value::Number(n) => n.as_u64().and_then(|n| u16::try_from(n).ok()).unwrap_or(port),
        // Porta nomeada: procura nos containers do pod.
        Value::String(named) => pod["spec"]["containers"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|c| c["ports"].as_array().into_iter().flatten())
            .find(|p| p["name"] == named.as_str())
            .and_then(|p| u16::try_from(p["containerPort"].as_u64()?).ok())
            .ok_or_else(|| format!("porta {named:?} não encontrada no pod {pod_name}"))?,
        _ => port,
    };
    Ok((pod_name, pod_port))
}

/// `{"app": "web", "tier": "api"}` → `app=web,tier=api`.
fn label_selector(labels: &Value) -> Option<String> {
    let map = labels.as_object().filter(|m| !m.is_empty())?;
    Some(map.iter().map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or_default())).collect::<Vec<_>>().join(","))
}

/// Percent-encoding para um valor de query string.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Uso atual de um pod (soma dos containers) ou node, pelo metrics-server.
#[derive(Serialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Milicores.
    pub cpu: f64,
    /// Bytes.
    pub memory: f64,
}

/// Uso por `namespace/nome` (pods) ou `nome` (nodes). Erro se o cluster não tem metrics-server.
pub async fn metrics(client: &Client, nodes: bool, namespace: Option<&str>) -> Result<HashMap<String, Usage>, String> {
    let path = match (nodes, namespace) {
        (true, _) => "/apis/metrics.k8s.io/v1beta1/nodes".to_owned(),
        (false, Some(ns)) => format!("/apis/metrics.k8s.io/v1beta1/namespaces/{}/pods", checked(ns)?),
        (false, None) => "/apis/metrics.k8s.io/v1beta1/pods".to_owned(),
    };
    Ok(parse_metrics(&get_json(client, path).await?))
}

fn parse_metrics(list: &Value) -> HashMap<String, Usage> {
    let usage_of = |u: &Value| Usage {
        cpu: u["cpu"].as_str().and_then(cpu_millis).unwrap_or(0.0),
        memory: u["memory"].as_str().and_then(bytes).unwrap_or(0.0),
    };
    list["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            let meta = &item["metadata"];
            let name = meta["name"].as_str().unwrap_or_default();
            let key = match meta["namespace"].as_str() {
                Some(ns) => format!("{ns}/{name}"),
                None => name.to_owned(),
            };
            let usage = match item["containers"].as_array() {
                Some(containers) => containers.iter().map(|c| usage_of(&c["usage"])).fold(Usage { cpu: 0.0, memory: 0.0 }, |a, b| Usage {
                    cpu: a.cpu + b.cpu,
                    memory: a.memory + b.memory,
                }),
                None => usage_of(&item["usage"]),
            };
            (key, usage)
        })
        .collect()
}

/// "250m" → 250; "1" → 1000; "123456789n" → 123,456789.
fn cpu_millis(q: &str) -> Option<f64> {
    let (number, factor) = match q.as_bytes().last()? {
        b'n' => (&q[..q.len() - 1], 1e-6),
        b'u' => (&q[..q.len() - 1], 1e-3),
        b'm' => (&q[..q.len() - 1], 1.0),
        _ => (q, 1000.0),
    };
    Some(number.parse::<f64>().ok()? * factor)
}

/// "128Mi", "1Gi", "500k", "1e3", "12345" → bytes.
fn bytes(q: &str) -> Option<f64> {
    const SUFFIXES: [(&str, f64); 12] = [
        ("Ki", 1024.0),
        ("Mi", 1048576.0),
        ("Gi", 1073741824.0),
        ("Ti", 1099511627776.0),
        ("Pi", 1125899906842624.0),
        ("Ei", 1152921504606846976.0),
        ("k", 1e3),
        ("M", 1e6),
        ("G", 1e9),
        ("T", 1e12),
        ("P", 1e15),
        ("E", 1e18),
    ];
    for (suffix, factor) in SUFFIXES {
        if let Some(number) = q.strip_suffix(suffix) {
            return Some(number.parse::<f64>().ok()? * factor);
        }
    }
    q.parse().ok()
}

/// Um custom resource do cluster (de um CRD), na versão que o servidor guarda.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CustomInfo {
    pub group: String,
    pub version: String,
    pub plural: String,
    pub kind: String,
    pub namespaced: bool,
}

pub async fn custom_resources(client: &Client) -> Result<Vec<CustomInfo>, String> {
    let list = get_json(client, "/apis/apiextensions.k8s.io/v1/customresourcedefinitions".into()).await?;
    Ok(parse_crds(&list))
}

fn parse_crds(list: &Value) -> Vec<CustomInfo> {
    let mut crds: Vec<CustomInfo> = list["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|crd| {
            let spec = &crd["spec"];
            let versions = spec["versions"].as_array()?;
            let version = versions
                .iter()
                .find(|v| v["storage"] == true && v["served"] == true)
                .or_else(|| versions.iter().find(|v| v["served"] == true))?;
            Some(CustomInfo {
                group: spec["group"].as_str()?.to_owned(),
                version: version["name"].as_str()?.to_owned(),
                plural: spec["names"]["plural"].as_str()?.to_owned(),
                kind: spec["names"]["kind"].as_str()?.to_owned(),
                namespaced: spec["scope"] == "Namespaced",
            })
        })
        .collect();
    crds.sort_by(|a, b| (&a.group, &a.kind).cmp(&(&b.group, &b.kind)));
    crds
}

/// Mensagem legível para os erros mais comuns.
pub(crate) fn describe(error: kube::Error) -> String {
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

    /// Os nomes que o frontend manda (`KubeKind` em backend.ts).
    #[test]
    fn kind_names() {
        let names = ["pod", "deployment", "statefulSet", "daemonSet", "replicaSet", "job", "cronJob", "service", "ingress",
            "networkPolicy", "configMap", "secret", "serviceAccount", "horizontalPodAutoscaler", "persistentVolumeClaim",
            "persistentVolume", "storageClass", "node", "namespace", "event"];
        for name in names {
            let kind: Kind = serde_json::from_value(name.into()).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(kind.path(Some("x")).is_ok());
        }
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
    fn pods_and_ports() {
        let pod = serde_json::json!({"metadata": {"annotations": {"kubectl.kubernetes.io/default-container": "app"}},
          "spec": {"initContainers": [{"name": "migrate"}],
            "containers": [{"name": "sidecar"}, {"name": "app", "ports": [{"containerPort": 8080, "name": "http"}, {"containerPort": 9090, "protocol": "TCP"}]}]}});
        let (list, default) = pod_containers(&pod);
        assert_eq!(default, "app");
        assert_eq!(list.iter().map(|c| (c.name.as_str(), c.init)).collect::<Vec<_>>(), [("sidecar", false), ("app", false), ("migrate", true)]);
        let ports = object_ports(Kind::Pod, &pod);
        assert_eq!(ports[0], Port { port: 8080, name: "http".into(), protocol: "TCP".into() });
        assert_eq!(ports.len(), 2);
        let svc = serde_json::json!({"spec": {"ports": [{"port": 80, "targetPort": "http", "name": "web"}]}});
        assert_eq!(object_ports(Kind::Service, &svc)[0].port, 80);
        assert_eq!(label_selector(&serde_json::json!({"app": "web", "tier": "api"})).unwrap(), "app=web,tier=api");
        assert!(label_selector(&serde_json::json!({})).is_none());
        assert_eq!(encode("app=web,tier=a b"), "app%3Dweb%2Ctier%3Da%20b");
    }

    #[test]
    fn custom_paths() {
        let custom = Resource::Custom(Custom { group: "cert-manager.io".into(), version: "v1".into(), plural: "certificates".into(), namespaced: true });
        assert_eq!(custom.path(Some("web")).unwrap(), "/apis/cert-manager.io/v1/namespaces/web/certificates");
        assert_eq!(custom.path(None).unwrap(), "/apis/cert-manager.io/v1/certificates");
        let evil = Resource::Custom(Custom { group: "a/../b".into(), version: "v1".into(), plural: "x".into(), namespaced: false });
        assert!(evil.path(None).is_err());
        // O frontend manda o tipo como texto (conhecido) ou objeto (custom).
        assert_eq!(serde_json::from_str::<Resource>(r#""pod""#).unwrap(), Resource::Builtin(Kind::Pod));
        assert_eq!(serde_json::from_str::<Resource>(r#"{"group":"cert-manager.io","version":"v1","plural":"certificates","namespaced":true}"#).unwrap(), custom);
    }

    #[test]
    fn quantities() {
        assert_eq!(cpu_millis("250m"), Some(250.0));
        assert_eq!(cpu_millis("2"), Some(2000.0));
        assert!((cpu_millis("123456789n").unwrap() - 123.456789).abs() < 1e-9);
        assert_eq!(bytes("128Mi"), Some(134217728.0));
        assert_eq!(bytes("1k"), Some(1000.0));
        assert_eq!(bytes("4096"), Some(4096.0));
        let list = serde_json::json!({"items": [
            {"metadata": {"name": "web", "namespace": "loja"}, "containers": [{"usage": {"cpu": "100m", "memory": "64Mi"}}, {"usage": {"cpu": "50m", "memory": "16Mi"}}]},
            {"metadata": {"name": "node-a"}, "usage": {"cpu": "1500m", "memory": "2Gi"}}]});
        let usage = parse_metrics(&list);
        assert_eq!(usage["loja/web"], Usage { cpu: 150.0, memory: 80.0 * 1048576.0 });
        assert_eq!(usage["node-a"].cpu, 1500.0);
    }

    #[test]
    fn crds() {
        let list = serde_json::json!({"items": [{"spec": {"group": "cert-manager.io", "scope": "Namespaced",
            "names": {"plural": "certificates", "kind": "Certificate"},
            "versions": [{"name": "v1alpha2", "served": false, "storage": false}, {"name": "v1", "served": true, "storage": true}]}}]});
        assert_eq!(parse_crds(&list), [CustomInfo { group: "cert-manager.io".into(), version: "v1".into(), plural: "certificates".into(), kind: "Certificate".into(), namespaced: true }]);
    }

    #[test]
    fn edit_checks() {
        let cm: Resource = Kind::ConfigMap.into();
        let text = "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: app\ndata:\n  a: b\n";
        let object = edited(&cm, Some("web"), "app", text).unwrap();
        assert_eq!(object["metadata"]["namespace"], "web");
        assert!(edited(&cm, Some("web"), "outro", text).unwrap_err().contains("nome"));
        assert!(edited(&cm, Some("prod"), "app", &text.replace("name: app", "name: app\n  namespace: web")).is_err());
        assert!(edited(&cm, None, "app", "a: [").unwrap_err().starts_with("YAML inválido"));
        let secret: Resource = Kind::Secret.into();
        let hidden = format!("apiVersion: v1\nkind: Secret\nmetadata:\n  name: app\ndata:\n  a: {HIDDEN}\n");
        assert!(edited(&secret, None, "app", &hidden).unwrap_err().contains("revele"));
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
        let pods = list(&client, &Kind::Pod.into(), Some("kube-system")).await.unwrap();
        println!("{:?}", pods.columns.iter().map(|c| &c.name).collect::<Vec<_>>());
        assert!(pods.columns.iter().any(|c| c.name == "Status"));
        assert!(!pods.rows.is_empty());
        assert!(pods.rows.iter().all(|r| r.namespace == "kube-system" && !r.name.is_empty()));
        let nodes = list(&client, &Kind::Node.into(), None).await.unwrap();
        assert!(!nodes.rows.is_empty());
        let all = list(&client, &Kind::Deployment.into(), None).await.unwrap();
        println!("{} deployments em todos os namespaces", all.rows.len());
        let pod = &pods.rows[0];
        let text = yaml(&client, &Kind::Pod.into(), Some("kube-system"), &pod.name, false).await.unwrap();
        assert!(text.contains(&format!("name: {}", pod.name)));
        assert!(!text.contains("managedFields"));
        let err = yaml(&client, &Kind::Pod.into(), Some("kube-system"), "nao-existe", false).await.unwrap_err();
        assert!(err.starts_with("não encontrado"), "{err}");
    }
}

/// Contra um nginx de teste: `kubectl create ns nexo-test; kubectl -n nexo-test create deployment web
/// --image=nginx:alpine --port=80; kubectl -n nexo-test expose deployment web --port=8080 --target-port=80`.
#[cfg(test)]
mod live_actions {
    use super::*;
    use crate::session::Session;
    use std::sync::{Arc, Mutex as StdMutex};
    use tauri::ipc::{Channel, InvokeResponseBody};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const NS: &str = "nexo-test";

    async fn client() -> Client {
        let current = contexts(None).unwrap().current.unwrap();
        Kube::default().client(None, &current).await.unwrap()
    }

    /// Um pod do nginx pronto (espera o rollout de um teste anterior).
    async fn web_pod(client: &Client) -> String {
        for _ in 0..120 {
            let table = list(client, &Kind::Pod.into(), Some(NS)).await.unwrap();
            let ready = table.rows.iter().filter(|r| r.name.starts_with("web-")).collect::<Vec<_>>();
            if let [only] = ready[..] {
                if only.cells[1] == "1/1" && only.cells[2] == "Running" {
                    return only.name.clone();
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        panic!("sem pod web pronto");
    }

    fn capture() -> (Channel<InvokeResponseBody>, Arc<StdMutex<Vec<u8>>>) {
        let out = Arc::new(StdMutex::new(Vec::new()));
        let sink = out.clone();
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(data) = body {
                sink.lock().unwrap().extend(data);
            }
            Ok(())
        });
        (channel, out)
    }

    /// `Some(código)` quando o terminal termina.
    type Exit = Arc<StdMutex<Option<Option<u32>>>>;

    fn exit_capture() -> (Channel<Option<u32>>, Exit) {
        let code = Arc::new(StdMutex::new(None));
        let sink = code.clone();
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                *sink.lock().unwrap() = Some(serde_json::from_str(&json).unwrap());
            }
            Ok(())
        });
        (channel, code)
    }

    async fn wait_for(what: impl Fn() -> bool) {
        for _ in 0..100 {
            if what() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        panic!("tempo esgotado");
    }

    #[tokio::test]
    #[ignore]
    async fn live_terminal() {
        let client = client().await;
        let pod = web_pod(&client).await;
        let (containers, default) = containers(&client, NS, &pod).await.unwrap();
        assert_eq!(default, "nginx");
        assert_eq!(containers.len(), 1);

        let (data, out) = capture();
        let (exit, code) = exit_capture();
        let mode = crate::k8s_term::Mode::Shell;
        let term = crate::k8s_term::open(client.clone(), NS, &pod, &default, mode, 100, 30, data, exit).await.unwrap();
        // O tamanho vai por outro canal que o comando: dá um tempo para ele chegar antes.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        term.write(b"echo nexo-$((40+2)); stty size; exit 3\n").unwrap();
        wait_for(|| code.lock().unwrap().is_some()).await;
        let text = String::from_utf8_lossy(&out.lock().unwrap()).into_owned();
        assert!(text.contains("nexo-42"), "{text}");
        assert!(text.contains("30 100"), "{text}");
        assert_eq!(*code.lock().unwrap(), Some(Some(3)));

        let (data, out) = capture();
        let (exit, code) = exit_capture();
        let mode = crate::k8s_term::Mode::Logs { previous: false };
        let term = crate::k8s_term::open(client.clone(), NS, &pod, &default, mode, 100, 30, data, exit).await.unwrap();
        wait_for(|| !out.lock().unwrap().is_empty()).await;
        let text = String::from_utf8_lossy(&out.lock().unwrap()).into_owned();
        assert!(text.contains("\r\n"), "{text}");
        term.write(b"\x03").unwrap();
        wait_for(|| code.lock().unwrap().is_some()).await;

        let err = crate::k8s_term::open(client, NS, &pod, "nao-existe", mode, 100, 30, capture().0, exit_capture().0).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    #[ignore]
    async fn live_forward() {
        let client = client().await;
        web_pod(&client).await;
        assert_eq!(ports(&client, Kind::Service, NS, "web").await.unwrap()[0].port, 8080);
        assert_eq!(ports(&client, Kind::Deployment, NS, "web").await.unwrap()[0].port, 80);
        let (pod, port) = resolve(&client, Kind::Service, NS, "web", 8080).await.unwrap();
        assert!(pod.starts_with("web-") && port == 80, "{pod} {port}");
        assert!(resolve(&client, Kind::Service, NS, "web", 9999).await.is_err());

        let kube = Kube::default();
        let info = kube.forwards.start(client.clone(), "ctx", NS, Kind::Service, "web", 8080, 0).await.unwrap();
        for _ in 0..2 {
            let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", info.local_port)).await.unwrap();
            socket.write_all(b"GET / HTTP/1.0\r\nHost: x\r\n\r\n").await.unwrap();
            let mut reply = String::new();
            socket.read_to_string(&mut reply).await.unwrap();
            assert!(reply.contains("nginx"), "{reply}");
        }
        // Porta sem nada escutando no pod: o erro aparece na lista.
        let bad = kube.forwards.start(client.clone(), "ctx", NS, Kind::Pod, &pod, 81, 0).await.unwrap();
        let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", bad.local_port)).await.unwrap();
        let mut sink = Vec::new();
        let _ = socket.read_to_end(&mut sink).await;
        wait_for(|| kube.forwards.list().iter().any(|f| f.id == bad.id && f.error == "nada escutando na porta 81 do pod")).await;
        println!("{:?}", kube.forwards.list());
        kube.forwards.stop(info.id);
        kube.forwards.stop(bad.id);
        assert!(kube.forwards.list().is_empty());
        wait_for(|| std::net::TcpStream::connect(("127.0.0.1", info.local_port)).is_err()).await;
    }

    #[tokio::test]
    #[ignore]
    async fn live_edit() {
        let client = client().await;
        let deploy: Resource = Kind::Deployment.into();
        let original = yaml(&client, &deploy, Some(NS), "web", false).await.unwrap();
        let changed = original.replacen("  replicas: 1\n", "  replicas: 1\n  minReadySeconds: 3\n", 1);
        assert_ne!(changed, original);
        let saved = apply(&client, &deploy, Some(NS), "web", &changed).await.unwrap();
        assert!(saved.contains("minReadySeconds: 3"), "{saved}");
        // O YAML antigo tem um resourceVersion velho: o servidor recusa.
        let err = apply(&client, &deploy, Some(NS), "web", &changed).await.unwrap_err();
        assert!(err.contains("mudou desde que foi aberto"), "{err}");
        let back = saved.replace("  minReadySeconds: 3\n", "");
        apply(&client, &deploy, Some(NS), "web", &back).await.unwrap();
        // kind não vem com metrics-server; os CRDs podem ser zero.
        match metrics(&client, false, Some(NS)).await {
            Ok(usage) => println!("métricas: {usage:?}"),
            Err(e) => println!("sem metrics-server: {e}"),
        }
        println!("{} CRDs", custom_resources(&client).await.unwrap().len());
    }

    /// Precisa do CRD de teste `widgets.nexo.test` (com a coluna Color) e de um Widget `azul`.
    #[tokio::test]
    #[ignore]
    async fn live_custom() {
        let client = client().await;
        let info = custom_resources(&client).await.unwrap().into_iter().find(|c| c.group == "nexo.test").expect("sem o CRD de teste");
        assert_eq!((info.kind.as_str(), info.plural.as_str(), info.version.as_str(), info.namespaced), ("Widget", "widgets", "v1", true));
        let widget = Resource::Custom(Custom { group: info.group, version: info.version, plural: info.plural, namespaced: true });
        let table = list(&client, &widget, Some(NS)).await.unwrap();
        assert!(table.columns.iter().any(|c| c.name == "Color"));
        assert_eq!(table.rows[0].name, "azul");
        let all = list(&client, &widget, None).await.unwrap();
        assert_eq!(all.rows[0].namespace, NS);
        let text = yaml(&client, &widget, Some(NS), "azul", false).await.unwrap();
        let saved = apply(&client, &widget, Some(NS), "azul", &text.replace("color: blue", "color: navy")).await.unwrap();
        assert!(saved.contains("color: navy"));
        apply(&client, &widget, Some(NS), "azul", &saved.replace("color: navy", "color: blue")).await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn live_actions() {
        let client = client().await;
        act(&client, &Kind::Deployment.into(), Some(NS), "web", Action::Scale { replicas: 2 }).await.unwrap();
        let text = yaml(&client, &Kind::Deployment.into(), Some(NS), "web", false).await.unwrap();
        assert!(text.contains("replicas: 2"), "{text}");
        act(&client, &Kind::Deployment.into(), Some(NS), "web", Action::Restart).await.unwrap();
        let text = yaml(&client, &Kind::Deployment.into(), Some(NS), "web", false).await.unwrap();
        assert!(text.contains("kubectl.kubernetes.io/restartedAt"), "{text}");
        act(&client, &Kind::Deployment.into(), Some(NS), "web", Action::Scale { replicas: 1 }).await.unwrap();
        assert!(act(&client, &Kind::Pod.into(), Some(NS), "web", Action::Scale { replicas: 1 }).await.is_err());
        assert!(act(&client, &Kind::Namespace.into(), None, NS, Action::Delete).await.is_err());
        let pod = web_pod(&client).await;
        act(&client, &Kind::Pod.into(), Some(NS), &pod, Action::Delete).await.unwrap();
        let err = act(&client, &Kind::Pod.into(), Some(NS), "nao-existe", Action::Delete).await.unwrap_err();
        assert!(err.starts_with("não encontrado"), "{err}");
    }
}
