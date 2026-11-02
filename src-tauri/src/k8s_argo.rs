//! Argo CD: Applications e AppProjects lidos direto da API do Kubernetes (os CRDs do
//! argoproj.io), sem o argocd-server nem token: vale o RBAC do kubeconfig, como no `argocd --core`.
//!
//! A árvore de um app junta o que o Argo gerencia (`status.resources`) com o que o cluster criou
//! a partir disso (ReplicaSets, Jobs e Pods), ligados pelos `ownerReferences`. Sincronizar,
//! voltar uma versão e encerrar escrevem no próprio Application, que é como o controller recebe
//! pedidos (`operation`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use futures::future::join_all;
use kube::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::k8s::{checked, get_json, merge_patch, send, Custom};

const API: &str = "/apis/argoproj.io/v1alpha1";
const REFRESH: &str = "argocd.argoproj.io/refresh";
/// O destino "este cluster" (onde o Argo CD roda).
const IN_CLUSTER: &str = "https://kubernetes.default.svc";

// ---------- Resumo de um app ----------

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub repo_url: String,
    pub path: String,
    pub chart: String,
    pub target_revision: String,
    /// Multi-source: o nome usado em `$ref` (só valores, sem manifests).
    #[serde(rename = "ref")]
    pub reference: String,
}

impl Source {
    fn of(v: &Value) -> Source {
        let s = |k: &str| v[k].as_str().unwrap_or_default().to_owned();
        Source { repo_url: s("repoURL"), path: s("path"), chart: s("chart"), target_revision: s("targetRevision"), reference: s("ref") }
    }
}

/// As fontes do spec (ou de um item do histórico): `sources` ou o `source` único.
fn sources(v: &Value) -> Vec<Source> {
    match v["sources"].as_array() {
        Some(list) if !list.is_empty() => list.iter().map(Source::of).collect(),
        _ if v["source"].is_object() => vec![Source::of(&v["source"])],
        _ => Vec::new(),
    }
}

#[derive(Serialize, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    /// Running, Terminating, Succeeded, Failed, Error.
    pub phase: String,
    pub message: String,
    pub started_at: String,
    pub finished_at: String,
    pub revision: String,
    pub initiated_by: String,
    pub automated: bool,
    pub dry_run: bool,
}

#[derive(Serialize, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    /// SyncError, ComparisonError, OrphanedResourceWarning…
    #[serde(rename = "type")]
    pub kind: String,
    pub message: String,
    pub time: String,
}

#[derive(Serialize, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub total: u32,
    pub out_of_sync: u32,
    pub degraded: u32,
    pub progressing: u32,
    pub missing: u32,
}

#[derive(Serialize, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct App {
    pub name: String,
    pub namespace: String,
    pub project: String,
    pub sources: Vec<Source>,
    pub dest_server: String,
    pub dest_name: String,
    pub dest_namespace: String,
    /// O destino é o cluster deste contexto: dá para ver os pods e o resto criado pelo cluster.
    pub in_cluster: bool,
    /// Synced, OutOfSync, Unknown.
    pub sync: String,
    pub revisions: Vec<String>,
    /// Healthy, Progressing, Degraded, Suspended, Missing, Unknown.
    pub health: String,
    pub health_message: String,
    pub auto_sync: bool,
    pub prune: bool,
    pub self_heal: bool,
    pub operation: Option<Operation>,
    /// Há um pedido de operação que o controller ainda não terminou.
    pub busy: bool,
    pub reconciled_at: String,
    pub created: String,
    pub labels: BTreeMap<String, String>,
    pub conditions: Vec<Condition>,
    pub counts: Counts,
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

/// Argo CD 3 tem `automated.enabled` (false = desligado sem apagar as opções).
fn automated(spec: &Value) -> Option<&Value> {
    let auto = &spec["syncPolicy"]["automated"];
    (auto.is_object() && auto["enabled"] != false).then_some(auto)
}

fn busy(app: &Value) -> bool {
    app["operation"].is_object() || matches!(app["status"]["operationState"]["phase"].as_str(), Some("Running" | "Terminating"))
}

pub(crate) fn summary(app: &Value) -> App {
    let (meta, spec, status) = (&app["metadata"], &app["spec"], &app["status"]);
    let dest = &spec["destination"];
    let dest_server = text(&dest["server"]);
    let dest_name = text(&dest["name"]);
    let auto = automated(spec);
    let operation = status["operationState"].is_object().then(|| {
        let op = &status["operationState"];
        let by = &op["operation"]["initiatedBy"];
        let revision = match op["syncResult"]["revision"].as_str() {
            Some(r) => r.to_owned(),
            None => op["syncResult"]["revisions"][0].as_str().or(op["operation"]["sync"]["revision"].as_str()).unwrap_or_default().to_owned(),
        };
        Operation {
            phase: text(&op["phase"]),
            message: text(&op["message"]),
            started_at: text(&op["startedAt"]),
            finished_at: text(&op["finishedAt"]),
            revision,
            initiated_by: text(&by["username"]),
            automated: by["automated"] == true,
            dry_run: op["operation"]["sync"]["dryRun"] == true,
        }
    });
    let mut counts = Counts::default();
    for r in status["resources"].as_array().into_iter().flatten() {
        counts.total += 1;
        if r["status"] == "OutOfSync" {
            counts.out_of_sync += 1;
        }
        match r["health"]["status"].as_str() {
            Some("Degraded") => counts.degraded += 1,
            Some("Progressing") => counts.progressing += 1,
            Some("Missing") => counts.missing += 1,
            _ => {}
        }
    }
    let revisions = match status["sync"]["revisions"].as_array() {
        Some(list) if !list.is_empty() => list.iter().map(text).collect(),
        _ => status["sync"]["revision"].as_str().map(|r| vec![r.to_owned()]).unwrap_or_default(),
    };
    App {
        name: text(&meta["name"]),
        namespace: text(&meta["namespace"]),
        project: spec["project"].as_str().filter(|p| !p.is_empty()).unwrap_or("default").to_owned(),
        sources: sources(spec),
        in_cluster: dest_server == IN_CLUSTER || (dest_server.is_empty() && dest_name == "in-cluster"),
        dest_server,
        dest_name,
        dest_namespace: text(&dest["namespace"]),
        sync: status["sync"]["status"].as_str().unwrap_or("Unknown").to_owned(),
        revisions,
        health: status["health"]["status"].as_str().unwrap_or("Unknown").to_owned(),
        health_message: text(&status["health"]["message"]),
        auto_sync: auto.is_some(),
        prune: auto.is_some_and(|a| a["prune"] == true),
        self_heal: auto.is_some_and(|a| a["selfHeal"] == true),
        operation,
        busy: busy(app),
        reconciled_at: text(&status["reconciledAt"]),
        created: text(&meta["creationTimestamp"]),
        labels: meta["labels"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
            .collect(),
        conditions: status["conditions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| Condition { kind: text(&c["type"]), message: text(&c["message"]), time: text(&c["lastTransitionTime"]) })
            .collect(),
        counts,
    }
}

// ---------- Lista ----------

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub name: String,
    pub namespace: String,
    pub description: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Apps {
    pub apps: Vec<App>,
    pub projects: Vec<Project>,
    /// A interface web do Argo CD (`url` do argocd-cm), se configurada.
    pub url: String,
}

/// Todos os Applications que o usuário pode ver (sem permissão no cluster todo, os do
/// namespace `argocd`), com os projetos e o endereço da interface web.
pub async fn apps(client: &Client) -> Result<Apps, String> {
    let list = match get_json(client, format!("{API}/applications")).await {
        Ok(list) => list,
        Err(e) if e.starts_with("sem permissão") => get_json(client, format!("{API}/namespaces/argocd/applications")).await.map_err(|_| e)?,
        Err(e) => return Err(e),
    };
    let mut apps: Vec<App> = list["items"].as_array().into_iter().flatten().map(summary).collect();
    apps.sort_by(|a, b| (&a.name, &a.namespace).cmp(&(&b.name, &b.namespace)));

    let projects = get_json(client, format!("{API}/appprojects")).await.map(|l| projects_of(&l)).unwrap_or_default();
    // O argocd-cm fica no namespace do Argo CD: o dos apps ou o padrão.
    let mut namespaces: Vec<&str> = projects.iter().map(|p| p.namespace.as_str()).chain(apps.iter().map(|a| a.namespace.as_str())).collect();
    namespaces.push("argocd");
    let mut url = String::new();
    for ns in namespaces.into_iter().collect::<std::collections::BTreeSet<_>>() {
        if let Ok(cm) = get_json(client, format!("/api/v1/namespaces/{}/configmaps/argocd-cm", checked(ns)?)).await {
            url = text(&cm["data"]["url"]).trim_end_matches('/').to_owned();
            break;
        }
    }
    Ok(Apps { apps, projects, url })
}

fn projects_of(list: &Value) -> Vec<Project> {
    let mut projects: Vec<Project> = list["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| Project {
            name: text(&p["metadata"]["name"]),
            namespace: text(&p["metadata"]["namespace"]),
            description: text(&p["spec"]["description"]),
        })
        .collect();
    projects.sort_by(|a, b| a.name.cmp(&b.name));
    projects
}

// ---------- Um app: árvore e histórico ----------

#[derive(Serialize, Debug, Default, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    /// `grupo/Kind/namespace/nome`.
    pub id: String,
    /// O dono na árvore ('' = direto no app).
    pub parent: String,
    pub group: String,
    pub version: String,
    pub kind: String,
    pub namespace: String,
    pub name: String,
    /// Está no git (gerenciado pelo Argo); os outros o cluster criou.
    pub managed: bool,
    /// Synced, OutOfSync ('' nos que o cluster criou).
    pub sync: String,
    pub health: String,
    pub health_message: String,
    /// Resultado da última operação para este recurso, quando não foi só "synced".
    pub result: String,
    pub hook: bool,
    /// Saiu do git e espera ser removido (sync com prune).
    pub requires_pruning: bool,
    /// Linhas curtas para o cartão ("2/2 prontos", "rev 4"…).
    pub info: Vec<String>,
    pub images: Vec<String>,
    pub created: String,
    /// ReplicaSet de uma versão antiga (sem réplicas).
    pub old: bool,
    /// Para abrir o YAML (None = tipo que este cluster não serve).
    pub resource: Option<Custom>,
}

fn node_id(group: &str, kind: &str, namespace: &str, name: &str) -> String {
    format!("{group}/{kind}/{namespace}/{name}")
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub id: i64,
    pub revisions: Vec<String>,
    pub sources: Vec<Source>,
    pub deployed_at: String,
    pub started_at: String,
    pub initiated_by: String,
    pub automated: bool,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    pub app: App,
    pub nodes: Vec<Node>,
    /// O mais recente primeiro.
    pub history: Vec<History>,
}

fn app_path(namespace: &str, name: &str) -> Result<String, String> {
    Ok(format!("{API}/namespaces/{}/applications/{}", checked(namespace)?, checked(name)?))
}

pub async fn app(client: &Client, namespace: &str, name: &str) -> Result<Detail, String> {
    let object = get_json(client, app_path(namespace, name)?).await?;
    let summary = summary(&object);
    let mut nodes = managed(&object);
    if summary.in_cluster {
        let live = live_objects(client, &nodes).await;
        nodes = link(nodes, &live);
    }
    resolve(client, &mut nodes).await;
    Ok(Detail { app: summary, nodes, history: history(&object) })
}

/// Ordem parecida com a das ondas do Argo: o que os outros usam vem antes.
fn kind_rank(kind: &str) -> u8 {
    match kind {
        "Namespace" => 0,
        "CustomResourceDefinition" => 1,
        "ServiceAccount" | "ClusterRole" | "Role" | "ClusterRoleBinding" | "RoleBinding" => 2,
        "ConfigMap" | "Secret" | "SealedSecret" | "ExternalSecret" => 3,
        "PersistentVolumeClaim" | "StorageClass" | "PersistentVolume" => 4,
        "Service" => 5,
        "Deployment" | "StatefulSet" | "DaemonSet" | "Rollout" => 6,
        "Job" | "CronJob" => 7,
        "Ingress" | "HTTPRoute" | "Gateway" => 8,
        "HorizontalPodAutoscaler" | "PodDisruptionBudget" => 9,
        _ => 10,
    }
}

/// Os recursos que o Argo gerencia, com o resultado da última operação de cada um.
fn managed(app: &Value) -> Vec<Node> {
    let status = &app["status"];
    let mut results: HashMap<String, String> = HashMap::new();
    for r in status["operationState"]["syncResult"]["resources"].as_array().into_iter().flatten() {
        let state = text(&r["status"]);
        let message = text(&r["message"]);
        // "Synced" sem mensagem de erro não precisa aparecer.
        let shown = match (state.as_str(), message.as_str()) {
            ("Synced", m) if !m.contains("error") && !m.contains("rror:") => String::new(),
            ("Synced" | "", m) => m.to_owned(),
            (s, "") => s.to_owned(),
            (s, m) => format!("{s}: {m}"),
        };
        if !shown.is_empty() {
            let id = node_id(r["group"].as_str().unwrap_or_default(), r["kind"].as_str().unwrap_or_default(), r["namespace"].as_str().unwrap_or_default(), r["name"].as_str().unwrap_or_default());
            results.insert(id, shown);
        }
    }
    let mut nodes: Vec<Node> = status["resources"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            let (group, kind, namespace, name) = (text(&r["group"]), text(&r["kind"]), text(&r["namespace"]), text(&r["name"]));
            let id = node_id(&group, &kind, &namespace, &name);
            Node {
                result: results.remove(&id).unwrap_or_default(),
                id,
                version: text(&r["version"]),
                sync: r["status"].as_str().unwrap_or("Unknown").to_owned(),
                health: text(&r["health"]["status"]),
                health_message: text(&r["health"]["message"]),
                hook: r["hook"] == true,
                requires_pruning: r["requiresPruning"] == true,
                managed: true,
                group,
                kind,
                namespace,
                name,
                ..Node::default()
            }
        })
        .collect();
    nodes.sort_by(|a, b| (kind_rank(&a.kind), &a.kind, &a.namespace, &a.name).cmp(&(kind_rank(&b.kind), &b.kind, &b.namespace, &b.name)));
    nodes
}

fn history(app: &Value) -> Vec<History> {
    let mut list: Vec<History> = app["status"]["history"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|h| History {
            id: h["id"].as_i64().unwrap_or_default(),
            revisions: match h["revisions"].as_array() {
                Some(r) if !r.is_empty() => r.iter().map(text).collect(),
                _ => vec![text(&h["revision"])],
            },
            sources: sources(h),
            deployed_at: text(&h["deployedAt"]),
            started_at: text(&h["deployStartedAt"]),
            initiated_by: text(&h["initiatedBy"]["username"]),
            automated: h["initiatedBy"]["automated"] == true,
        })
        .collect();
    list.reverse();
    list
}

/// Um objeto lido do cluster (as listas não trazem o `kind` em cada item).
struct Live {
    kind: &'static str,
    object: Value,
}

/// Lê os ReplicaSets, Jobs e Pods dos namespaces onde o app tem algo que cria pods.
async fn live_objects(client: &Client, nodes: &[Node]) -> Vec<Live> {
    const OWNERS: [&str; 7] = ["Deployment", "StatefulSet", "DaemonSet", "ReplicaSet", "Job", "CronJob", "Rollout"];
    // Por namespace, os tipos gerenciados que criam pods (para ler também o próprio workload).
    let mut wanted: BTreeMap<&str, HashSet<&str>> = BTreeMap::new();
    for n in nodes.iter().filter(|n| OWNERS.contains(&n.kind.as_str()) && !n.namespace.is_empty()) {
        wanted.entry(n.namespace.as_str()).or_default().insert(n.kind.as_str());
    }
    let mut requests = Vec::new();
    for (ns, kinds) in wanted {
        let Ok(ns) = checked(ns) else { continue };
        requests.push(("ReplicaSet", format!("/apis/apps/v1/namespaces/{ns}/replicasets")));
        requests.push(("Pod", format!("/api/v1/namespaces/{ns}/pods")));
        if kinds.contains("CronJob") {
            requests.push(("Job", format!("/apis/batch/v1/namespaces/{ns}/jobs")));
        }
        // O Argo CD 3 não grava mais a saúde de cada recurso no Application: a dos workloads
        // é calculada aqui, do objeto vivo.
        for (kind, plural) in [("Deployment", "deployments"), ("StatefulSet", "statefulsets"), ("DaemonSet", "daemonsets")] {
            if kinds.contains(kind) {
                requests.push((kind, format!("/apis/apps/v1/namespaces/{ns}/{plural}")));
            }
        }
    }
    let lists = join_all(requests.into_iter().map(|(kind, path)| async move { (kind, get_json(client, path).await) })).await;
    let mut out = Vec::new();
    for (kind, list) in lists {
        // Sem permissão para listar pods de um namespace: a árvore só fica mais rasa.
        let Ok(mut list) = list else { continue };
        if let Some(items) = list["items"].as_array_mut() {
            out.extend(items.drain(..).map(|object| Live { kind, object }));
        }
    }
    out
}

/// Pendura os objetos vivos nos donos (pelo `ownerReferences` de controller), em camadas:
/// ReplicaSets e Jobs primeiro, depois os Pods. Recursos gerenciados que também aparecem vivos
/// (um Job no git, por exemplo) ganham as informações do objeto.
fn link(mut nodes: Vec<Node>, live: &[Live]) -> Vec<Node> {
    let mut index: HashMap<(String, String, String), usize> =
        nodes.iter().enumerate().map(|(i, n)| ((n.namespace.clone(), n.kind.clone(), n.name.clone()), i)).collect();
    // Workloads gerenciados que foram lidos e não existem no cluster: ainda não foram criados.
    const WORKLOADS: [&str; 3] = ["Deployment", "StatefulSet", "DaemonSet"];
    let read: HashSet<(&str, &str)> = live
        .iter()
        .filter(|l| WORKLOADS.contains(&l.kind))
        .map(|l| (l.kind, l.object["metadata"]["namespace"].as_str().unwrap_or_default()))
        .collect();
    for n in nodes.iter_mut().filter(|n| n.health.is_empty() && WORKLOADS.contains(&n.kind.as_str())) {
        let listed = WORKLOADS.iter().find(|k| **k == n.kind).is_some_and(|k| read.contains(&(*k, n.namespace.as_str())));
        let exists = live.iter().any(|l| l.kind == n.kind && text(&l.object["metadata"]["name"]) == n.name && text(&l.object["metadata"]["namespace"]) == n.namespace);
        if listed && !exists {
            n.health = "Missing".into();
        }
    }
    for layer in [WORKLOADS.as_slice(), ["ReplicaSet", "Job"].as_slice(), ["Pod"].as_slice()] {
        for item in live.iter().filter(|l| layer.contains(&l.kind)) {
            let meta = &item.object["metadata"];
            let (namespace, name) = (text(&meta["namespace"]), text(&meta["name"]));
            let key = (namespace.clone(), item.kind.to_owned(), name.clone());
            if let Some(&at) = index.get(&key) {
                let found = describe_live(item.kind, &item.object);
                let n = &mut nodes[at];
                n.info = found.info;
                n.images = found.images;
                n.created = found.created;
                // A saúde que o Argo gravou vale mais; sem ela (Argo CD 3), a calculada.
                if n.health.is_empty() {
                    n.health = found.health;
                    n.health_message = found.health_message;
                }
                continue;
            }
            // Workloads só complementam os gerenciados (os de outros apps não entram).
            if WORKLOADS.contains(&item.kind) {
                continue;
            }
            let owners = meta["ownerReferences"].as_array();
            let owner = owners.and_then(|o| o.iter().find(|r| r["controller"] == true).or(o.first()));
            let Some(owner) = owner else { continue };
            let Some(&parent) = index.get(&(namespace.clone(), text(&owner["kind"]), text(&owner["name"]))) else { continue };
            let mut node = describe_live(item.kind, &item.object);
            node.parent = nodes[parent].id.clone();
            index.insert(key, nodes.len());
            nodes.push(node);
        }
    }
    nodes
}

/// Um ReplicaSet, Job ou Pod vivo como nó da árvore, com a saúde calculada como o Argo faz.
fn describe_live(kind: &'static str, object: &Value) -> Node {
    let (meta, spec, status) = (&object["metadata"], &object["spec"], &object["status"]);
    let (group, version) = match kind {
        "ReplicaSet" | "Deployment" | "StatefulSet" | "DaemonSet" => ("apps", "v1"),
        "Job" => ("batch", "v1"),
        _ => ("", "v1"),
    };
    let (namespace, name) = (text(&meta["namespace"]), text(&meta["name"]));
    let mut node = Node {
        id: node_id(group, kind, &namespace, &name),
        group: group.into(),
        version: version.into(),
        kind: kind.into(),
        namespace,
        name,
        created: text(&meta["creationTimestamp"]),
        ..Node::default()
    };
    let num = |v: &Value| v.as_u64().unwrap_or(0);
    match kind {
        "ReplicaSet" => {
            let (want, ready) = (num(&spec["replicas"]), num(&status["readyReplicas"]));
            if let Some(rev) = meta["annotations"]["deployment.kubernetes.io/revision"].as_str() {
                node.info.push(format!("rev {rev}"));
            }
            node.old = want == 0 && num(&status["replicas"]) == 0;
            if !node.old {
                node.info.push(format!("{ready}/{want} prontos"));
                node.health = if ready >= want { "Healthy" } else { "Progressing" }.into();
            }
        }
        "Deployment" | "StatefulSet" | "DaemonSet" => {
            let (health, message, ready, want) = workload_health(kind, object);
            node.health = health.into();
            node.health_message = message;
            node.info.push(format!("{ready}/{want} prontos"));
            node.images = spec["template"]["spec"]["containers"].as_array().into_iter().flatten().filter_map(|c| c["image"].as_str().map(str::to_owned)).collect();
        }
        "Job" => {
            let conditions = status["conditions"].as_array().into_iter().flatten();
            let failed = conditions.clone().find(|c| c["type"] == "Failed" && c["status"] == "True");
            let complete = conditions.clone().any(|c| c["type"] == "Complete" && c["status"] == "True");
            node.health = if let Some(f) = failed {
                node.health_message = text(&f["message"]);
                "Degraded"
            } else if complete {
                "Healthy"
            } else if spec["suspend"] == true {
                "Suspended"
            } else {
                "Progressing"
            }
            .into();
            node.info.push(match (num(&status["succeeded"]), num(&status["failed"]), num(&status["active"])) {
                (s, 0, 0) if complete => format!("concluído ({s})"),
                (_, f, a) if a > 0 => format!("{a} rodando{}", if f > 0 { format!(", {f} falhas") } else { String::new() }),
                (s, f, _) => format!("{s} ok, {f} falhas"),
            });
        }
        _ => {
            let pod = pod_status(object);
            node.health = pod.health.into();
            node.health_message = pod.message;
            node.info.push(pod.status);
            node.info.push(format!("{}/{} prontos", pod.ready, pod.total));
            if pod.restarts > 0 {
                node.info.push(format!("{} reinícios", pod.restarts));
            }
            if let Some(n) = spec["nodeName"].as_str() {
                node.info.push(n.to_owned());
            }
            node.images = spec["containers"].as_array().into_iter().flatten().filter_map(|c| c["image"].as_str().map(str::to_owned)).collect();
        }
    }
    node
}

/// Saúde de Deployment, StatefulSet e DaemonSet como nas regras do Argo CD (gitops-engine):
/// esperando o controller ver a mudança, réplicas novas subindo, prazo estourado.
fn workload_health(kind: &str, object: &Value) -> (&'static str, String, u64, u64) {
    let (meta, spec, status) = (&object["metadata"], &object["spec"], &object["status"]);
    let num = |v: &Value| v.as_u64().unwrap_or(0);
    let stale = num(&status["observedGeneration"]) < num(&meta["generation"]);
    let (ready, want, updated, available) = match kind {
        "DaemonSet" => (
            num(&status["numberReady"]),
            num(&status["desiredNumberScheduled"]),
            num(&status["updatedNumberScheduled"]),
            num(&status["numberAvailable"]),
        ),
        _ => {
            let want = spec["replicas"].as_u64().unwrap_or(1);
            let updated = if kind == "StatefulSet" && status["currentRevision"] != status["updateRevision"] {
                num(&status["updatedReplicas"])
            } else if kind == "StatefulSet" {
                want
            } else {
                num(&status["updatedReplicas"])
            };
            let available = if kind == "StatefulSet" { num(&status["readyReplicas"]) } else { num(&status["availableReplicas"]) };
            (num(&status["readyReplicas"]), want, updated, available)
        }
    };
    let deadline = status["conditions"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|c| c["type"] == "Progressing" && c["reason"] == "ProgressDeadlineExceeded");
    let (health, message) = if let Some(c) = deadline {
        ("Degraded", text(&c["message"]))
    } else if spec["paused"] == true {
        ("Suspended", "pausado".to_owned())
    } else if stale {
        ("Progressing", "esperando o controller ver a mudança".to_owned())
    } else if updated < want {
        ("Progressing", format!("{updated} de {want} réplicas atualizadas"))
    } else if available < want {
        ("Progressing", format!("{available} de {want} réplicas disponíveis"))
    } else {
        ("Healthy", String::new())
    };
    (health, message, ready, want)
}

struct PodStatus {
    status: String,
    health: &'static str,
    message: String,
    ready: usize,
    total: usize,
    restarts: u64,
}

/// O "Status" do `kubectl get pods` (motivo do container que está travando) e a saúde.
fn pod_status(pod: &Value) -> PodStatus {
    let status = &pod["status"];
    let phase = status["phase"].as_str().unwrap_or("Unknown");
    let containers: Vec<&Value> = status["containerStatuses"].as_array().into_iter().flatten().collect();
    let total = pod["spec"]["containers"].as_array().map_or(containers.len(), Vec::len);
    let ready = containers.iter().filter(|c| c["ready"] == true).count();
    let restarts = containers.iter().map(|c| c["restartCount"].as_u64().unwrap_or(0)).sum();
    let mut shown = status["reason"].as_str().unwrap_or(phase).to_owned();
    let mut message = text(&status["message"]);
    for c in status["initContainerStatuses"].as_array().into_iter().flatten() {
        let state = &c["state"];
        if let Some(reason) = state["waiting"]["reason"].as_str().filter(|r| *r != "PodInitializing") {
            shown = format!("Init:{reason}");
            message = text(&state["waiting"]["message"]);
        } else if state["terminated"]["exitCode"].as_i64().is_some_and(|code| code != 0) {
            shown = "Init:Error".into();
        }
    }
    if !shown.starts_with("Init:") {
        for c in &containers {
            let state = &c["state"];
            if let Some(reason) = state["waiting"]["reason"].as_str() {
                shown = reason.to_owned();
                message = text(&state["waiting"]["message"]);
            } else if let Some(reason) = state["terminated"]["reason"].as_str().filter(|_| phase != "Succeeded") {
                shown = reason.to_owned();
            }
        }
    }
    let terminating = pod["metadata"]["deletionTimestamp"].is_string();
    if terminating {
        shown = "Terminating".into();
    }
    let broken = ["BackOff", "Err", "Invalid", "OOMKilled", "Error"].iter().any(|bad| shown.contains(bad));
    let health = match phase {
        _ if terminating => "Progressing",
        "Succeeded" => "Healthy",
        "Failed" => "Degraded",
        _ if broken => "Degraded",
        "Running" if ready == total => "Healthy",
        _ => "Progressing",
    };
    PodStatus { status: shown, health, message, ready, total, restarts }
}

/// Descobre o caminho na API de cada tipo (plural e se tem namespace), para abrir o YAML.
async fn resolve(client: &Client, nodes: &mut [Node]) {
    let wanted: HashSet<(String, String)> = nodes.iter().map(|n| (n.group.clone(), n.version.clone())).collect();
    let missing: Vec<(String, String)> = {
        let cache = DISCOVERY.lock().expect("cache");
        wanted.into_iter().filter(|gv| !cache.contains_key(gv)).collect()
    };
    let found = join_all(missing.into_iter().map(|(group, version)| async move {
        let path = if group.is_empty() { format!("/api/{version}") } else { format!("/apis/{group}/{version}") };
        let list = get_json(client, path).await.ok().map(|l| api_resources(&l));
        ((group, version), list)
    }))
    .await;
    let mut cache = DISCOVERY.lock().expect("cache");
    for (gv, list) in found {
        // Falhou (sem permissão, versão que sumiu): tenta de novo na próxima vez.
        if let Some(list) = list {
            cache.insert(gv, list);
        }
    }
    for n in nodes.iter_mut() {
        let known = cache.get(&(n.group.clone(), n.version.clone())).and_then(|kinds| kinds.get(&n.kind));
        n.resource = known.map(|(plural, namespaced)| Custom {
            group: n.group.clone(),
            version: n.version.clone(),
            plural: plural.clone(),
            namespaced: *namespaced,
        });
    }
}

type Kinds = HashMap<String, (String, bool)>;

/// Por grupo/versão: Kind → (plural, tem namespace). Igual em qualquer cluster (os nomes vêm
/// da definição do tipo), então o cache vale para todos os contextos.
static DISCOVERY: LazyLock<Mutex<HashMap<(String, String), Kinds>>> = LazyLock::new(Default::default);

fn api_resources(list: &Value) -> Kinds {
    list["resources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let plural = r["name"].as_str().filter(|n| !n.contains('/'))?;
            Some((text(&r["kind"]), (plural.to_owned(), r["namespaced"] == true)))
        })
        .collect()
}

// ---------- Ações ----------

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub group: String,
    pub kind: String,
    pub namespace: String,
    pub name: String,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Action {
    /// Compara de novo com o git (`hard`: sem o cache de manifests do repo-server).
    Refresh { hard: bool },
    Sync {
        #[serde(default)]
        prune: bool,
        #[serde(default)]
        dry_run: bool,
        #[serde(default)]
        force: bool,
        #[serde(default)]
        apply_out_of_sync_only: bool,
        #[serde(default)]
        server_side: bool,
        #[serde(default)]
        replace: bool,
        /// Vazio = o app inteiro.
        #[serde(default)]
        resources: Vec<Target>,
    },
    /// Volta para uma versão do histórico (só com o auto-sync desligado, como no Argo).
    Rollback { id: i64, #[serde(default)] prune: bool },
    /// Interrompe a operação em andamento.
    Terminate,
    AutoSync { enabled: bool, #[serde(default)] prune: bool, #[serde(default)] self_heal: bool },
}

/// As opções do app (`spec.syncPolicy.syncOptions`, como `CreateNamespace=true`) mais as
/// escolhidas agora, que valem por cima. O controller só lê as da operação: sem copiar as do
/// app, um sync manual ignoraria o que está configurado nele (a interface web faz o mesmo).
fn sync_options(app: &Value, chosen: &[&str]) -> Vec<String> {
    let key = |o: &str| o.split('=').next().unwrap_or_default().to_owned();
    let mut options: Vec<String> = app["spec"]["syncPolicy"]["syncOptions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| o.as_str())
        .filter(|o| !chosen.iter().any(|c| key(c) == key(o)))
        .map(str::to_owned)
        .collect();
    options.extend(chosen.iter().map(|c| (*c).to_owned()));
    options
}

/// Quem pediu, como aparece no histórico do Argo.
const INITIATOR: &str = "nexo";

pub async fn act(client: &Client, namespace: &str, name: &str, action: Action) -> Result<(), String> {
    let path = app_path(namespace, name)?;
    let body = match action {
        Action::Refresh { hard } => json!({ "metadata": { "annotations": { REFRESH: if hard { "hard" } else { "normal" } } } }),
        Action::Terminate => {
            let app = get_json(client, path.clone()).await?;
            if app["status"]["operationState"]["phase"] != "Running" {
                return Err("nenhuma operação em andamento".into());
            }
            json!({ "status": { "operationState": { "phase": "Terminating" } } })
        }
        Action::AutoSync { enabled: false, .. } => json!({ "spec": { "syncPolicy": { "automated": null } } }),
        Action::AutoSync { enabled: true, prune, self_heal } => {
            let app = get_json(client, path.clone()).await?;
            let mut automated = json!({ "prune": prune, "selfHeal": self_heal });
            // Argo CD 3: um `enabled: false` que já esteja lá continuaria desligando.
            if app["spec"]["syncPolicy"]["automated"].get("enabled").is_some() {
                automated["enabled"] = true.into();
            }
            json!({ "spec": { "syncPolicy": { "automated": automated } } })
        }
        sync @ (Action::Sync { .. } | Action::Rollback { .. }) => {
            let app = get_json(client, path.clone()).await?;
            json!({ "operation": operation(&app, sync)? })
        }
    };
    send(client, merge_patch(path, &body)?).await.map_err(|e| e.replace("applications.argoproj.io", "Application"))
}

/// O pedido de sync que o controller do Argo executa (o mesmo que a interface web e o
/// `argocd app sync` gravam no Application).
fn operation(app: &Value, action: Action) -> Result<Value, String> {
    if busy(app) {
        return Err("já há uma operação em andamento neste app".into());
    }
    let mut sync = serde_json::Map::new();
    match action {
        Action::Sync { prune, dry_run, force, apply_out_of_sync_only, server_side, replace, resources } => {
            let mut chosen = Vec::new();
            if apply_out_of_sync_only {
                chosen.push("ApplyOutOfSyncOnly=true");
            }
            if server_side {
                chosen.push("ServerSideApply=true");
            }
            if replace {
                chosen.push("Replace=true");
            }
            sync.insert("prune".into(), prune.into());
            sync.insert("dryRun".into(), dry_run.into());
            sync.insert("syncStrategy".into(), json!({ "hook": { "force": force } }));
            let options = sync_options(app, &chosen);
            if !options.is_empty() {
                sync.insert("syncOptions".into(), json!(options));
            }
            if !resources.is_empty() {
                let list: Vec<Value> = resources
                    .iter()
                    .map(|r| json!({ "group": r.group, "kind": r.kind, "namespace": r.namespace, "name": r.name }))
                    .collect();
                sync.insert("resources".into(), list.into());
            }
        }
        Action::Rollback { id, prune } => {
            if automated(&app["spec"]).is_some() {
                return Err("desligue o auto-sync para voltar uma versão (senão o Argo volta para o git logo em seguida)".into());
            }
            let entry = app["status"]["history"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|h| h["id"].as_i64() == Some(id))
                .ok_or_else(|| format!("a versão {id} não está mais no histórico"))?;
            if entry["sources"].as_array().is_some_and(|s| !s.is_empty()) {
                sync.insert("sources".into(), entry["sources"].clone());
                sync.insert("revisions".into(), entry["revisions"].clone());
            } else {
                sync.insert("source".into(), entry["source"].clone());
                sync.insert("revision".into(), entry["revision"].clone());
            }
            sync.insert("prune".into(), prune.into());
            let options = sync_options(app, &[]);
            if !options.is_empty() {
                sync.insert("syncOptions".into(), json!(options));
            }
        }
        _ => unreachable!("só sync e rollback viram operação"),
    }
    Ok(json!({ "sync": sync, "initiatedBy": { "username": INITIATOR }, "retry": {} }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_fixture() -> Value {
        json!({
            "metadata": { "name": "loja", "namespace": "argocd", "creationTimestamp": "2026-10-01T10:00:00Z", "labels": { "time": "pagamentos" } },
            "spec": {
                "project": "pagamentos",
                "source": { "repoURL": "https://github.com/empresa/gitops", "path": "apps/loja", "targetRevision": "main" },
                "destination": { "server": "https://kubernetes.default.svc", "namespace": "loja" },
                "syncPolicy": { "automated": { "prune": true } }
            },
            "status": {
                "sync": { "status": "OutOfSync", "revision": "abc123" },
                "health": { "status": "Degraded" },
                "reconciledAt": "2026-11-02T12:00:00Z",
                "conditions": [{ "type": "SyncError", "message": "falhou", "lastTransitionTime": "2026-11-02T11:00:00Z" }],
                "resources": [
                    { "group": "apps", "version": "v1", "kind": "Deployment", "namespace": "loja", "name": "api", "status": "Synced", "health": { "status": "Degraded", "message": "progress deadline" } },
                    { "version": "v1", "kind": "Service", "namespace": "loja", "name": "api", "status": "OutOfSync", "health": { "status": "Healthy" } },
                    { "version": "v1", "kind": "ConfigMap", "namespace": "loja", "name": "velho", "status": "OutOfSync", "requiresPruning": true }
                ],
                "operationState": {
                    "phase": "Failed", "message": "one or more objects failed to apply",
                    "startedAt": "2026-11-02T11:00:00Z", "finishedAt": "2026-11-02T11:00:05Z",
                    "operation": { "sync": { "revision": "abc123" }, "initiatedBy": { "automated": true } },
                    "syncResult": { "revision": "abc123", "resources": [
                        { "group": "apps", "kind": "Deployment", "namespace": "loja", "name": "api", "status": "SyncFailed", "message": "image inválida" },
                        { "kind": "Service", "namespace": "loja", "name": "api", "status": "Synced", "message": "service/api configured" }
                    ] }
                },
                "history": [
                    { "id": 1, "revision": "aaa", "deployedAt": "2026-10-01T10:00:00Z", "source": { "repoURL": "https://github.com/empresa/gitops", "path": "apps/loja", "targetRevision": "main" }, "initiatedBy": { "username": "admin" } },
                    { "id": 2, "revision": "abc123", "deployedAt": "2026-11-02T11:00:05Z", "source": { "repoURL": "https://github.com/empresa/gitops", "path": "apps/loja", "targetRevision": "main" }, "initiatedBy": { "automated": true } }
                ]
            }
        })
    }

    #[test]
    fn summary_reads_status() {
        let s = summary(&app_fixture());
        assert_eq!((s.name.as_str(), s.project.as_str(), s.sync.as_str(), s.health.as_str()), ("loja", "pagamentos", "OutOfSync", "Degraded"));
        assert!(s.in_cluster && s.auto_sync && s.prune && !s.self_heal && !s.busy);
        assert_eq!(s.counts, Counts { total: 3, out_of_sync: 2, degraded: 1, progressing: 0, missing: 0 });
        assert_eq!(s.revisions, ["abc123"]);
        assert_eq!(s.sources[0].path, "apps/loja");
        let op = s.operation.unwrap();
        assert_eq!((op.phase.as_str(), op.automated, op.revision.as_str()), ("Failed", true, "abc123"));
        assert_eq!(s.conditions[0].kind, "SyncError");
        assert_eq!(s.labels["time"], "pagamentos");
    }

    #[test]
    fn summary_defaults_and_argo3_disabled() {
        let s = summary(&json!({
            "metadata": { "name": "x", "namespace": "argocd" },
            "spec": { "destination": { "name": "prod" }, "syncPolicy": { "automated": { "enabled": false, "prune": true } } }
        }));
        assert_eq!((s.project.as_str(), s.sync.as_str(), s.health.as_str()), ("default", "Unknown", "Unknown"));
        assert!(!s.in_cluster && !s.auto_sync && !s.prune);
        assert!(s.operation.is_none());
    }

    #[test]
    fn managed_nodes_sorted_with_results() {
        let nodes = managed(&app_fixture());
        let kinds: Vec<_> = nodes.iter().map(|n| n.kind.as_str()).collect();
        assert_eq!(kinds, ["ConfigMap", "Service", "Deployment"]);
        let deploy = &nodes[2];
        assert_eq!(deploy.result, "SyncFailed: image inválida");
        assert_eq!(deploy.health_message, "progress deadline");
        // Synced sem erro não aparece.
        assert_eq!(nodes[1].result, "");
        assert!(nodes[0].requires_pruning);
    }

    #[test]
    fn history_newest_first() {
        let h = history(&app_fixture());
        assert_eq!(h.iter().map(|h| h.id).collect::<Vec<_>>(), [2, 1]);
        assert!(h[0].automated && h[1].initiated_by == "admin");
    }

    fn owned(kind: &'static str, name: &str, owner_kind: &str, owner: &str, extra: Value) -> Live {
        let mut object = json!({ "metadata": { "name": name, "namespace": "loja", "ownerReferences": [{ "kind": owner_kind, "name": owner, "controller": true }] } });
        if let (Some(o), Some(e)) = (object.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                if k == "metadata" {
                    for (mk, mv) in v.as_object().unwrap() {
                        o["metadata"][mk] = mv.clone();
                    }
                } else {
                    o.insert(k.clone(), v.clone());
                }
            }
        }
        Live { kind, object }
    }

    #[test]
    fn link_builds_tree() {
        let nodes = managed(&app_fixture());
        let live = vec![
            // Pod antes do ReplicaSet na lista: as camadas resolvem a ordem.
            owned("Pod", "api-7d-x", "ReplicaSet", "api-7d", json!({
                "spec": { "containers": [{ "image": "loja:1.2" }], "nodeName": "worker" },
                "status": { "phase": "Running", "containerStatuses": [{ "ready": false, "restartCount": 4, "state": { "waiting": { "reason": "CrashLoopBackOff", "message": "back-off" } } }] }
            })),
            owned("ReplicaSet", "api-7d", "Deployment", "api", json!({ "metadata": { "annotations": { "deployment.kubernetes.io/revision": "3" } }, "spec": { "replicas": 1 }, "status": { "replicas": 1, "readyReplicas": 0 } })),
            owned("ReplicaSet", "api-6c", "Deployment", "api", json!({ "spec": { "replicas": 0 }, "status": { "replicas": 0 } })),
            // De outro app: não entra.
            owned("ReplicaSet", "outro-1", "Deployment", "outro", json!({})),
            owned("Pod", "solto", "ReplicaSet", "nao-existe", json!({})),
        ];
        let tree = link(nodes, &live);
        let find = |name: &str| tree.iter().find(|n| n.name == name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(tree.len(), 6);
        let rs = find("api-7d");
        assert_eq!(rs.parent, "apps/Deployment/loja/api");
        assert_eq!(rs.info, ["rev 3", "0/1 prontos"]);
        assert_eq!(rs.health, "Progressing");
        assert!(find("api-6c").old);
        let pod = find("api-7d-x");
        assert_eq!(pod.parent, "apps/ReplicaSet/loja/api-7d");
        assert_eq!(pod.health, "Degraded");
        assert_eq!(pod.info, ["CrashLoopBackOff", "0/1 prontos", "4 reinícios", "worker"]);
        assert_eq!(pod.images, ["loja:1.2"]);
        assert!(!tree.iter().any(|n| n.name == "outro-1" || n.name == "solto"));
    }

    #[test]
    fn workload_health_rules() {
        let d = |spec: Value, status: Value| workload_health("Deployment", &json!({ "metadata": { "generation": 2 }, "spec": spec, "status": status }));
        let ok = d(json!({ "replicas": 2 }), json!({ "observedGeneration": 2, "updatedReplicas": 2, "availableReplicas": 2, "readyReplicas": 2 }));
        assert_eq!((ok.0, ok.2, ok.3), ("Healthy", 2, 2));
        assert_eq!(d(json!({ "replicas": 2 }), json!({ "observedGeneration": 1 })).0, "Progressing");
        assert_eq!(d(json!({ "replicas": 2 }), json!({ "observedGeneration": 2, "updatedReplicas": 2, "availableReplicas": 1 })).0, "Progressing");
        let late = d(json!({}), json!({ "observedGeneration": 2, "conditions": [{ "type": "Progressing", "reason": "ProgressDeadlineExceeded", "message": "prazo" }] }));
        assert_eq!((late.0, late.1.as_str()), ("Degraded", "prazo"));
        assert_eq!(d(json!({ "paused": true }), json!({ "observedGeneration": 2 })).0, "Suspended");
        let ds = workload_health("DaemonSet", &json!({ "status": { "desiredNumberScheduled": 3, "updatedNumberScheduled": 3, "numberAvailable": 3, "numberReady": 3 } }));
        assert_eq!(ds.0, "Healthy");
    }

    #[test]
    fn link_fills_workload_health() {
        let mut nodes = managed(&app_fixture());
        nodes[2].health.clear(); // Argo CD 3: sem saúde gravada.
        nodes.push(Node { id: "apps/Deployment/loja/sumiu".into(), kind: "Deployment".into(), namespace: "loja".into(), name: "sumiu".into(), managed: true, ..Node::default() });
        let deploy = Live { kind: "Deployment", object: json!({ "metadata": { "name": "api", "namespace": "loja" }, "spec": { "replicas": 1 }, "status": { "updatedReplicas": 1, "availableReplicas": 1, "readyReplicas": 1 } }) };
        let tree = link(nodes, &[deploy]);
        let api = tree.iter().find(|n| n.kind == "Deployment" && n.name == "api").unwrap();
        assert_eq!((api.health.as_str(), api.info.as_slice()), ("Healthy", ["1/1 prontos".to_owned()].as_slice()));
        assert_eq!(tree.iter().find(|n| n.name == "sumiu").unwrap().health, "Missing");
    }

    #[test]
    fn pod_states() {
        let pod = |phase: &str, extra: Value| {
            let mut p = json!({ "spec": { "containers": [{}] }, "status": { "phase": phase } });
            for (k, v) in extra.as_object().unwrap() {
                p["status"][k] = v.clone();
            }
            pod_status(&p)
        };
        let ok = pod("Running", json!({ "containerStatuses": [{ "ready": true, "state": { "running": {} } }] }));
        assert_eq!((ok.status.as_str(), ok.health), ("Running", "Healthy"));
        let starting = pod("Running", json!({ "containerStatuses": [{ "ready": false, "state": { "running": {} } }] }));
        assert_eq!(starting.health, "Progressing");
        let done = pod("Succeeded", json!({ "containerStatuses": [{ "ready": false, "state": { "terminated": { "reason": "Completed" } } }] }));
        assert_eq!((done.status.as_str(), done.health), ("Succeeded", "Healthy"));
        let pull = pod("Pending", json!({ "containerStatuses": [{ "ready": false, "state": { "waiting": { "reason": "ImagePullBackOff" } } }] }));
        assert_eq!((pull.status.as_str(), pull.health), ("ImagePullBackOff", "Degraded"));
        let init = pod("Pending", json!({ "initContainerStatuses": [{ "state": { "terminated": { "exitCode": 1 } } }] }));
        assert_eq!((init.status.as_str(), init.health), ("Init:Error", "Degraded"));
        let evicted = pod("Failed", json!({ "reason": "Evicted" }));
        assert_eq!((evicted.status.as_str(), evicted.health), ("Evicted", "Degraded"));
    }

    #[test]
    fn sync_operation() {
        let mut app = app_fixture();
        let op = operation(
            &app,
            Action::Sync {
                prune: true,
                dry_run: false,
                force: false,
                apply_out_of_sync_only: true,
                server_side: false,
                replace: false,
                resources: vec![Target { group: "apps".into(), kind: "Deployment".into(), namespace: "loja".into(), name: "api".into() }],
            },
        )
        .unwrap();
        assert_eq!(op["initiatedBy"]["username"], "nexo");
        assert_eq!(op["sync"]["prune"], true);
        assert_eq!(op["sync"]["syncOptions"], json!(["ApplyOutOfSyncOnly=true"]));
        assert_eq!(op["sync"]["resources"][0]["kind"], "Deployment");
        assert_eq!(op["sync"]["syncStrategy"], json!({ "hook": { "force": false } }));

        // Com o auto-sync ligado não volta versão.
        let err = operation(&app, Action::Rollback { id: 1, prune: false }).unwrap_err();
        assert!(err.contains("auto-sync"), "{err}");
        app["spec"]["syncPolicy"] = json!({});
        let op = operation(&app, Action::Rollback { id: 1, prune: false }).unwrap();
        assert_eq!(op["sync"]["revision"], "aaa");
        assert_eq!(op["sync"]["source"]["path"], "apps/loja");
        assert!(operation(&app, Action::Rollback { id: 9, prune: false }).unwrap_err().contains("histórico"));

        // Ocupado.
        app["operation"] = json!({ "sync": {} });
        assert!(operation(&app, Action::Rollback { id: 1, prune: false }).unwrap_err().contains("andamento"));
    }

    #[test]
    fn keeps_app_sync_options() {
        let mut app = app_fixture();
        app["spec"]["syncPolicy"]["syncOptions"] = json!(["CreateNamespace=true", "ServerSideApply=false"]);
        let sync = |server_side| Action::Sync {
            prune: false,
            dry_run: false,
            force: false,
            apply_out_of_sync_only: false,
            server_side,
            replace: false,
            resources: vec![],
        };
        assert_eq!(operation(&app, sync(false)).unwrap()["sync"]["syncOptions"], json!(["CreateNamespace=true", "ServerSideApply=false"]));
        assert_eq!(operation(&app, sync(true)).unwrap()["sync"]["syncOptions"], json!(["CreateNamespace=true", "ServerSideApply=true"]));
        app["spec"]["syncPolicy"]["automated"] = Value::Null;
        assert_eq!(operation(&app, Action::Rollback { id: 1, prune: false }).unwrap()["sync"]["syncOptions"], json!(["CreateNamespace=true", "ServerSideApply=false"]));
    }

    #[test]
    fn action_json() {
        let a: Action = serde_json::from_value(json!({ "type": "sync", "prune": true, "applyOutOfSyncOnly": true })).unwrap();
        assert!(matches!(a, Action::Sync { prune: true, apply_out_of_sync_only: true, .. }));
        let a: Action = serde_json::from_value(json!({ "type": "autoSync", "enabled": true, "selfHeal": true })).unwrap();
        assert!(matches!(a, Action::AutoSync { enabled: true, self_heal: true, prune: false }));
        let a: Action = serde_json::from_value(json!({ "type": "refresh", "hard": true })).unwrap();
        assert!(matches!(a, Action::Refresh { hard: true }));
    }

    #[test]
    fn discovery_parsing() {
        let kinds = api_resources(&json!({ "resources": [
            { "name": "deployments", "kind": "Deployment", "namespaced": true },
            { "name": "deployments/scale", "kind": "Scale", "namespaced": true },
            { "name": "clusterroles", "kind": "ClusterRole", "namespaced": false }
        ] }));
        assert_eq!(kinds["Deployment"], ("deployments".to_owned(), true));
        assert_eq!(kinds["ClusterRole"], ("clusterroles".to_owned(), false));
        assert!(!kinds.contains_key("Scale"));
    }

    #[test]
    fn projects_sorted() {
        let p = projects_of(&json!({ "items": [
            { "metadata": { "name": "z", "namespace": "argocd" }, "spec": {} },
            { "metadata": { "name": "a", "namespace": "argocd" }, "spec": { "description": "time A" } }
        ] }));
        assert_eq!(p[0], Project { name: "a".into(), namespace: "argocd".into(), description: "time A".into() });
    }
}

/// Contra um Argo CD de verdade no cluster do contexto atual, com o app `guestbook` (manual) do
/// argocd-example-apps no projeto `web`:
/// `cargo test --lib k8s_argo::live -- --ignored --nocapture --test-threads=1`.
#[cfg(test)]
mod live {
    use super::*;
    use crate::k8s::{contexts, Kube};
    use std::time::Duration;

    async fn client() -> Client {
        let current = contexts(None).unwrap().current.unwrap();
        Kube::default().client(None, &current).await.unwrap()
    }

    /// Espera o app chegar num estado (o controller trabalha em segundo plano).
    async fn until(client: &Client, what: &str, ok: impl Fn(&Detail) -> bool) -> Detail {
        for _ in 0..90 {
            let d = app(client, "argocd", "guestbook").await.unwrap();
            if ok(&d) {
                return d;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        panic!("guestbook não chegou em: {what}");
    }

    #[tokio::test]
    #[ignore]
    async fn live_argo() {
        let client = client().await;
        let list = apps(&client).await.unwrap();
        println!("{} apps; projetos {:?}", list.apps.len(), list.projects.iter().map(|p| &p.name).collect::<Vec<_>>());
        for a in &list.apps {
            println!("  {:<22} {:<11} {:<10} {:<11} {:?}", a.name, a.project, a.sync, a.health, a.operation.as_ref().map(|o| &o.phase));
        }
        assert!(list.projects.iter().any(|p| p.name == "web"));
        let errado = list.apps.iter().find(|a| a.name == "caminho-errado").expect("caminho-errado");
        assert!(errado.conditions.iter().any(|c| c.kind == "ComparisonError"), "{:?}", errado.conditions);

        // Desliga o auto-sync caso um teste anterior tenha deixado ligado.
        act(&client, "argocd", "guestbook", Action::AutoSync { enabled: false, prune: false, self_heal: false }).await.unwrap();
        act(&client, "argocd", "guestbook", Action::Refresh { hard: false }).await.unwrap();
        let d = until(&client, "sem operação", |d| !d.app.busy && d.app.sync != "Unknown").await;
        println!("antes: {} / {} · {} recursos", d.app.sync, d.app.health, d.nodes.len());

        // Sync do app inteiro; o segundo pedido logo em seguida é recusado.
        let sync = |resources: Vec<Target>| Action::Sync {
            prune: true,
            dry_run: false,
            force: false,
            apply_out_of_sync_only: false,
            server_side: false,
            replace: false,
            resources,
        };
        act(&client, "argocd", "guestbook", sync(Vec::new())).await.unwrap();
        let err = act(&client, "argocd", "guestbook", sync(Vec::new())).await.unwrap_err();
        assert!(err.contains("andamento"), "{err}");
        let d = until(&client, "Synced e Healthy", |d| !d.app.busy && d.app.sync == "Synced" && d.app.health == "Healthy").await;
        let op = d.app.operation.as_ref().unwrap();
        assert_eq!((op.phase.as_str(), op.initiated_by.as_str()), ("Succeeded", INITIATOR));

        // Árvore: Deployment → ReplicaSet → Pod, todos com caminho para abrir o YAML.
        for n in &d.nodes {
            println!("  {:<40} pai={:<45} {:<9} {:<8} {:?}", format!("{}/{}", n.kind, n.name), n.parent, n.sync, n.health, n.info);
        }
        let deploy = d.nodes.iter().find(|n| n.kind == "Deployment").expect("deployment");
        let rs = d.nodes.iter().find(|n| n.kind == "ReplicaSet" && n.parent == deploy.id).expect("replicaset");
        let pod = d.nodes.iter().find(|n| n.kind == "Pod" && n.parent == rs.id).expect("pod");
        assert_eq!(deploy.health, "Healthy", "saúde calculada do Deployment");
        assert!(!pod.managed && pod.health == "Healthy" && !pod.images.is_empty());
        assert!(d.nodes.iter().all(|n| n.resource.is_some()), "tipo sem caminho na API");
        assert!(!d.history.is_empty());

        // Sync de um recurso só.
        let service = Target { group: "".into(), kind: "Service".into(), namespace: deploy.namespace.clone(), name: "guestbook-ui".into() };
        act(&client, "argocd", "guestbook", sync(vec![service]))
            .await
            .unwrap();
        // Sync parcial não entra no histórico: espera a operação nova terminar.
        let before = d.app.operation.as_ref().map(|o| o.started_at.clone()).unwrap_or_default();
        let d = until(&client, "sync do Service", |d| !d.app.busy && d.app.operation.as_ref().is_some_and(|o| o.started_at != before)).await;
        assert_eq!(d.app.operation.as_ref().unwrap().phase, "Succeeded");

        // Rollback: recusado com auto-sync, aceito sem.
        act(&client, "argocd", "guestbook", Action::AutoSync { enabled: true, prune: false, self_heal: false }).await.unwrap();
        let first = d.history.last().unwrap().id;
        let err = act(&client, "argocd", "guestbook", Action::Rollback { id: first, prune: false }).await.unwrap_err();
        assert!(err.contains("auto-sync"), "{err}");
        let auto = until(&client, "auto-sync ligado", |d| d.app.auto_sync).await;
        assert!(!auto.app.prune);
        act(&client, "argocd", "guestbook", Action::AutoSync { enabled: false, prune: false, self_heal: false }).await.unwrap();
        until(&client, "auto-sync desligado", |d| !d.app.auto_sync && !d.app.busy).await;
        act(&client, "argocd", "guestbook", Action::Rollback { id: first, prune: false }).await.unwrap();
        let count = d.history.len();
        let d = until(&client, "rollback concluído", |d| !d.app.busy && d.history.len() > count).await;
        assert_eq!(d.app.operation.as_ref().unwrap().phase, "Succeeded");
        println!("histórico: {:?}", d.history.iter().map(|h| (h.id, &h.revisions[0], &h.initiated_by)).collect::<Vec<_>>());

        // Encerrar sem operação rodando.
        let err = act(&client, "argocd", "guestbook", Action::Terminate).await.unwrap_err();
        assert!(err.contains("nenhuma"), "{err}");
    }
}
