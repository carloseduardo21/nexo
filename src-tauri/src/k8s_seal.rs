//! Sealed Secrets (bitnami-labs/sealed-secrets): selar valores com o certificado do controller
//! do cluster, como o `kubeseal`, sem precisar do binário.
//!
//! Formato de cada valor (pkg/crypto do sealed-secrets): uma chave AES-256 aleatória cifrada com
//! RSA-OAEP (SHA-256, rótulo conforme o escopo), seguida do valor em AES-GCM com nonce zero (a
//! chave só é usada uma vez): `tamanho do RSA (u16 BE) ‖ RSA ‖ AES-GCM`, em base64.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use kube::Client;
use ring::aead;
use rsa::pkcs8::DecodePublicKey as _;
use rsa::sha2::Sha256;
use rsa::traits::PaddingScheme as _;
use rsa::{Oaep, RsaPublicKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::k8s::{checked, describe, encode, get_json, to_yaml};

const API: &str = "/apis/bitnami.com/v1alpha1";
const CLUSTER_WIDE: &str = "sealedsecrets.bitnami.com/cluster-wide";
const NAMESPACE_WIDE: &str = "sealedsecrets.bitnami.com/namespace-wide";
/// Num Secret que já existe: deixa o controller assumir (senão ele recusa sobrescrever).
const MANAGED: &str = "sealedsecrets.bitnami.com/managed";
/// Chaves de selagem que o controller guarda no próprio namespace (a mais nova é a atual).
const KEY_LABEL: &str = "sealedsecrets.bitnami.com/sealed-secrets-key=active";

/// Em que nomes o valor selado pode ser aberto (o rótulo do RSA-OAEP).
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    /// Só com este nome e namespace (o padrão do kubeseal).
    Strict,
    /// Qualquer nome, no mesmo namespace.
    NamespaceWide,
    /// Qualquer nome e namespace.
    ClusterWide,
}

impl Scope {
    fn label(self, namespace: &str, name: &str) -> Vec<u8> {
        match self {
            Scope::Strict => format!("{namespace}/{name}").into_bytes(),
            Scope::NamespaceWide => namespace.as_bytes().to_vec(),
            Scope::ClusterWide => Vec::new(),
        }
    }

    /// O escopo pelas anotações do SealedSecret (as mesmas que o controller lê).
    fn of(metadata: &Value) -> Scope {
        let on = |key: &str| metadata["annotations"][key] == "true";
        if on(CLUSTER_WIDE) {
            Scope::ClusterWide
        } else if on(NAMESPACE_WIDE) {
            Scope::NamespaceWide
        } else {
            Scope::Strict
        }
    }

    fn annotate(self, metadata: &mut Value) {
        if !metadata["annotations"].is_object() {
            metadata["annotations"] = json!({});
        }
        let annotations = metadata["annotations"].as_object_mut().expect("objeto");
        annotations.remove(CLUSTER_WIDE);
        annotations.remove(NAMESPACE_WIDE);
        match self {
            Scope::Strict => {}
            Scope::NamespaceWide => drop(annotations.insert(NAMESPACE_WIDE.into(), "true".into())),
            Scope::ClusterWide => drop(annotations.insert(CLUSTER_WIDE.into(), "true".into())),
        }
        if annotations.is_empty() {
            metadata.as_object_mut().expect("objeto").remove("annotations");
        }
    }
}

/// Um valor do Secret. Lido: `value` None = não deu para ler (só a chave é conhecida). Ao
/// salvar: None = manter o valor selado que já está lá.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub key: String,
    pub value: Option<String>,
    /// O valor é base64 (conteúdo binário, que não é texto).
    #[serde(default)]
    pub base64: bool,
}

impl Entry {
    fn bytes(&self) -> Result<Option<Vec<u8>>, String> {
        match &self.value {
            None => Ok(None),
            Some(v) if self.base64 => B64.decode(v.trim()).map(Some).map_err(|_| format!("{}: base64 inválido", self.key)),
            Some(v) => Ok(Some(v.clone().into_bytes())),
        }
    }
}

/// Valores de `data` de um Secret (base64): texto quando dá, senão base64 marcado.
fn entries_of(data: &Value) -> Vec<Entry> {
    data.as_object()
        .into_iter()
        .flatten()
        .map(|(key, value)| {
            let raw = value.as_str().unwrap_or_default();
            match B64.decode(raw).ok().map(String::from_utf8) {
                Some(Ok(text)) => Entry { key: key.clone(), value: Some(text), base64: false },
                _ => Entry { key: key.clone(), value: Some(raw.to_owned()), base64: true },
            }
        })
        .collect()
}

// ---------- Certificado do controller ----------

/// Onde está o controller: o Service (para pedir o certificado) e o namespace.
#[derive(Debug, PartialEq)]
struct Controller {
    namespace: String,
    name: String,
    port: String,
}

/// Services do controller: pelos rótulos do chart Helm e do controller.yaml, depois os nomes
/// padrão (o do kubeseal primeiro). Sem permissão para listar, sobram os padrões.
async fn controllers(client: &Client) -> Vec<Controller> {
    let mut found = Vec::new();
    for selector in ["app.kubernetes.io/name=sealed-secrets", "name=sealed-secrets-controller"] {
        if let Ok(list) = get_json(client, format!("/api/v1/services?labelSelector={}", encode(selector))).await {
            found.extend(services(&list));
        }
    }
    for (namespace, name) in [
        ("kube-system", "sealed-secrets-controller"),
        ("kube-system", "sealed-secrets"),
        ("sealed-secrets", "sealed-secrets"),
        ("sealed-secrets", "sealed-secrets-controller"),
    ] {
        if !found.iter().any(|c| c.namespace == namespace && c.name == name) {
            found.push(Controller { namespace: namespace.into(), name: name.into(), port: String::new() });
        }
    }
    found
}

fn services(list: &Value) -> Vec<Controller> {
    list["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|svc| {
            let name = svc["metadata"]["name"].as_str()?;
            // O chart também cria um Service só de métricas, com os mesmos rótulos.
            if name.contains("metrics") {
                return None;
            }
            let ports = svc["spec"]["ports"].as_array()?;
            let port = ports.iter().find(|p| p["name"] == "http").or(ports.first())?;
            let port = match port["name"].as_str().filter(|n| !n.is_empty()) {
                Some(named) => named.to_owned(),
                None => port["port"].as_u64()?.to_string(),
            };
            Some(Controller { namespace: svc["metadata"]["namespace"].as_str()?.to_owned(), name: name.to_owned(), port })
        })
        .collect()
}

/// O certificado público do controller (o mesmo do `kubeseal --fetch-cert`): pelo proxy do
/// Service ou, sem acesso a ele, pela chave ativa guardada num Secret do controller.
pub async fn certificate(client: &Client) -> Result<RsaPublicKey, String> {
    let candidates = controllers(client).await;
    let mut errors = Vec::new();
    for c in &candidates {
        let target = if c.port.is_empty() { c.name.clone() } else { format!("{}:{}", c.name, c.port) };
        let path = format!("/api/v1/namespaces/{}/services/{}/proxy/v1/cert.pem", checked(&c.namespace)?, checked(&target)?);
        let request = http::Request::get(path).body(Vec::new()).map_err(|e| e.to_string())?;
        match client.request_text(request).await {
            Ok(pem) => return public_key(&pem),
            // Nomes padrão que não existem neste cluster não interessam no erro.
            Err(kube::Error::Api(status)) if status.code == 404 && c.port.is_empty() => {}
            Err(e) => errors.push(format!("{}/{}: {}", c.namespace, c.name, describe(e))),
        }
    }
    let keys = get_json(client, format!("/api/v1/secrets?labelSelector={}", encode(KEY_LABEL))).await;
    match keys.as_ref().ok().and_then(newest_key) {
        Some(pem) => public_key(&pem),
        None if errors.is_empty() => Err("controller do Sealed Secrets não encontrado neste cluster".into()),
        None => Err(format!("não deu para pegar o certificado do controller ({})", errors.join("; "))),
    }
}

/// `tls.crt` da chave de selagem mais nova (a que o controller usa para selar).
fn newest_key(list: &Value) -> Option<String> {
    let secret = list["items"]
        .as_array()?
        .iter()
        .filter(|s| s["data"]["tls.crt"].is_string())
        .max_by_key(|s| s["metadata"]["creationTimestamp"].as_str().unwrap_or_default().to_owned())?;
    String::from_utf8(B64.decode(secret["data"]["tls.crt"].as_str()?).ok()?).ok()
}

fn public_key(pem: &str) -> Result<RsaPublicKey, String> {
    use x509_cert::der::{DecodePem as _, Encode as _};
    let cert = x509_cert::Certificate::from_pem(pem.trim()).map_err(|e| format!("certificado do controller inválido: {e}"))?;
    let spki = cert.tbs_certificate().subject_public_key_info().to_der().map_err(|e| e.to_string())?;
    RsaPublicKey::from_public_key_der(&spki).map_err(|e| format!("o certificado do controller não é RSA: {e}"))
}

/// Sela um valor (em base64, como vai em `encryptedData`).
fn seal(key: &RsaPublicKey, label: &[u8], plain: &[u8]) -> Result<String, String> {
    let mut session = [0u8; 32];
    getrandom::fill(&mut session).map_err(|e| format!("sem números aleatórios: {e}"))?;
    let wrapped = Oaep::<Sha256>::new_with_label(label.to_vec())
        .encrypt(&mut getrandom::SysRng, key, &session)
        .map_err(|e| format!("RSA: {e}"))?;
    let gcm = aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM, &session).map_err(|_| "AES: chave inválida")?);
    let mut sealed = plain.to_vec();
    gcm.seal_in_place_append_tag(aead::Nonce::assume_unique_for_key([0; 12]), aead::Aad::empty(), &mut sealed)
        .map_err(|_| "AES: falha ao cifrar")?;
    let size = u16::try_from(wrapped.len()).map_err(|_| "chave RSA grande demais")?;
    let mut out = Vec::with_capacity(2 + wrapped.len() + sealed.len());
    out.extend(size.to_be_bytes());
    out.extend(wrapped);
    out.extend(sealed);
    Ok(B64.encode(out))
}

// ---------- Ler ----------

fn object_path(namespace: &str, name: &str) -> Result<String, String> {
    Ok(format!("{API}/namespaces/{}/sealedsecrets/{}", checked(namespace)?, checked(name)?))
}

fn secret_path(namespace: &str, name: &str) -> Result<String, String> {
    Ok(format!("/api/v1/namespaces/{}/secrets/{}", checked(namespace)?, checked(name)?))
}

/// Um SealedSecret aberto para editar: as chaves e, se der para ler o Secret que o controller
/// criou, os valores atuais.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Sealed {
    pub scope: Scope,
    #[serde(rename = "type")]
    pub secret_type: String,
    pub entries: Vec<Entry>,
    /// Por que os valores atuais não aparecem ('' = vieram do Secret).
    pub values_error: String,
    /// O controller não conseguiu abrir ('' = tudo certo ou ainda não respondeu).
    pub status: String,
    pub resource_version: String,
}

pub async fn sealed(client: &Client, namespace: &str, name: &str) -> Result<Sealed, String> {
    let object = get_json(client, object_path(namespace, name)?).await?;
    let secret = get_json(client, secret_path(namespace, name)?).await;
    Ok(open(&object, secret))
}

fn open(object: &Value, secret: Result<Value, String>) -> Sealed {
    let status = object["status"]["conditions"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|c| c["type"] == "Synced" && c["status"] == "False")
        .map(|c| c["message"].as_str().unwrap_or("o controller não conseguiu abrir").to_owned())
        .unwrap_or_default();
    let (values, values_error) = match secret {
        Ok(secret) => (entries_of(&secret["data"]), String::new()),
        Err(e) if e.starts_with("não encontrado") => (Vec::new(), "o controller ainda não criou o Secret".to_owned()),
        Err(e) => (Vec::new(), e),
    };
    let entries = object["spec"]["encryptedData"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(key, _)| match values.iter().find(|v| v.key == *key) {
            Some(v) => v.clone(),
            None => Entry { key: key.clone(), value: None, base64: false },
        })
        .collect();
    Sealed {
        scope: Scope::of(&object["metadata"]),
        secret_type: object["spec"]["template"]["type"].as_str().unwrap_or("Opaque").to_owned(),
        entries,
        values_error,
        status,
        resource_version: object["metadata"]["resourceVersion"].as_str().unwrap_or_default().to_owned(),
    }
}

/// Os valores de um Secret comum (para selá-lo).
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Plain {
    #[serde(rename = "type")]
    pub secret_type: String,
    pub entries: Vec<Entry>,
}

pub async fn plain(client: &Client, namespace: &str, name: &str) -> Result<Plain, String> {
    let secret = get_json(client, secret_path(namespace, name)?).await?;
    Ok(Plain { secret_type: secret["type"].as_str().unwrap_or("Opaque").to_owned(), entries: entries_of(&secret["data"]) })
}

// ---------- Selar e salvar ----------

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub namespace: String,
    pub name: String,
    pub scope: Scope,
    #[serde(rename = "type")]
    pub secret_type: String,
    pub entries: Vec<Entry>,
    /// Editando: o resourceVersion de quando abriu (None = criar um novo).
    pub resource_version: Option<String>,
    /// Criar/atualizar no cluster; senão só devolve o YAML.
    pub apply: bool,
    /// Criando com o nome de um Secret que já existe: deixar o controller assumir o Secret.
    #[serde(default)]
    pub adopt: bool,
}

/// Sela os valores novos e devolve o YAML do SealedSecret: o salvo no cluster (`apply`) ou
/// um pronto para o git (sem os campos do servidor).
pub async fn save(client: &Client, request: &Request) -> Result<String, String> {
    let (namespace, name) = (checked(&request.namespace)?, checked(&request.name)?);
    check_entries(&request.entries)?;
    let existing = match &request.resource_version {
        Some(_) => Some(get_json(client, object_path(namespace, name)?).await?),
        None => None,
    };
    let needs_key = request.entries.iter().any(|e| e.value.is_some());
    let key = if needs_key { Some(certificate(client).await?) } else { None };
    let mut object = build(request, existing.as_ref(), key.as_ref())?;
    if !request.apply {
        return to_yaml(&mut exported(object), false);
    }
    let saved: Value = match &request.resource_version {
        Some(version) => {
            // O que estava aberto, não o que acabou de ser lido: o servidor recusa se mudou.
            object["metadata"]["resourceVersion"] = version.as_str().into();
            client.request(json_request(http::Method::PUT, object_path(namespace, name)?, &object)?).await.map_err(|e| match e {
                kube::Error::Api(s) if s.code == 409 => "o SealedSecret mudou desde que foi aberto: abra de novo e refaça".to_owned(),
                e => describe(e),
            })?
        }
        None => {
            if request.adopt {
                adopt(client, namespace, name).await?;
            }
            let path = format!("{API}/namespaces/{namespace}/sealedsecrets");
            client.request(json_request(http::Method::POST, path, &object)?).await.map_err(|e| match e {
                kube::Error::Api(s) if s.code == 409 => format!("já existe um SealedSecret {name} em {namespace}"),
                kube::Error::Api(s) if s.code == 404 => "este cluster não tem o Sealed Secrets (CRD sealedsecrets.bitnami.com)".to_owned(),
                e => describe(e),
            })?
        }
    };
    to_yaml(&mut { saved }, false)
}

fn check_entries(entries: &[Entry]) -> Result<(), String> {
    if entries.is_empty() {
        return Err("adicione ao menos um valor".into());
    }
    for (i, e) in entries.iter().enumerate() {
        let valid = !e.key.is_empty() && e.key.len() <= 253 && e.key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if !valid {
            return Err(format!("chave inválida: {:?} (use letras, números, -, _ e .)", e.key));
        }
        if entries[..i].iter().any(|o| o.key == e.key) {
            return Err(format!("chave repetida: {}", e.key));
        }
    }
    Ok(())
}

/// O SealedSecret com os valores selados. Editando, parte do objeto atual (rótulos, anotações
/// e o resto do template continuam) e reaproveita os valores mantidos.
fn build(request: &Request, existing: Option<&Value>, key: Option<&RsaPublicKey>) -> Result<Value, String> {
    let (namespace, name) = (request.namespace.as_str(), request.name.as_str());
    let mut object = match existing {
        Some(current) => {
            let mut object = current.clone();
            object.as_object_mut().ok_or("SealedSecret inválido")?.remove("status");
            object
        }
        None => json!({
            "apiVersion": "bitnami.com/v1alpha1",
            "kind": "SealedSecret",
            "metadata": { "name": name, "namespace": namespace },
            "spec": { "template": { "metadata": { "name": name, "namespace": namespace } } },
        }),
    };
    let previous = existing.map(|o| Scope::of(&o["metadata"]));
    let label = request.scope.label(namespace, name);
    let mut data = serde_json::Map::new();
    for entry in &request.entries {
        let value = match (entry.bytes()?, key) {
            (Some(plain), Some(key)) => seal(key, &label, &plain)?,
            (Some(_), None) => unreachable!("certificado pedido quando há valores novos"),
            (None, _) => {
                if previous.is_some_and(|p| p != request.scope) {
                    return Err(format!("mudar o escopo sela tudo de novo: preencha o valor de {}", entry.key));
                }
                let kept = existing.and_then(|o| o["spec"]["encryptedData"][&entry.key].as_str());
                kept.ok_or_else(|| format!("{}: sem valor", entry.key))?.into()
            }
        };
        data.insert(entry.key.clone(), Value::String(value));
    }
    request.scope.annotate(&mut object["metadata"]);
    if !object["spec"]["template"].is_object() {
        object["spec"]["template"] = json!({});
    }
    let template = &mut object["spec"]["template"];
    if !template["metadata"].is_object() {
        template["metadata"] = json!({});
    }
    request.scope.annotate(&mut template["metadata"]);
    let kind = request.secret_type.trim();
    template["type"] = if kind.is_empty() { "Opaque" } else { kind }.into();
    object["spec"]["encryptedData"] = Value::Object(data);
    Ok(object)
}

/// Sem os campos que o servidor preenche: o YAML que vai para o repositório.
fn exported(mut object: Value) -> Value {
    if let Some(meta) = object["metadata"].as_object_mut() {
        for field in ["uid", "resourceVersion", "generation", "creationTimestamp", "managedFields", "selfLink", "ownerReferences", "finalizers"] {
            meta.remove(field);
        }
        if let Some(annotations) = meta.get_mut("annotations").and_then(Value::as_object_mut) {
            annotations.remove("kubectl.kubernetes.io/last-applied-configuration");
            if annotations.is_empty() {
                meta.remove("annotations");
            }
        }
    }
    object
}

/// O Secret com este nome (se existir e não for de um SealedSecret) passa a ser do controller.
async fn adopt(client: &Client, namespace: &str, name: &str) -> Result<(), String> {
    let secret = match get_json(client, secret_path(namespace, name)?).await {
        Ok(secret) => secret,
        Err(e) if e.starts_with("não encontrado") => return Ok(()),
        Err(e) => return Err(e),
    };
    let owned = secret["metadata"]["ownerReferences"].as_array().into_iter().flatten().any(|o| o["kind"] == "SealedSecret");
    if owned || secret["metadata"]["annotations"][MANAGED] == "true" {
        return Ok(());
    }
    let patch = json!({ "metadata": { "annotations": { MANAGED: "true" } } });
    let request = http::Request::patch(secret_path(namespace, name)?)
        .header(http::header::CONTENT_TYPE, "application/merge-patch+json")
        .header(http::header::ACCEPT, "application/json")
        .body(patch.to_string().into_bytes())
        .map_err(|e| e.to_string())?;
    client.request::<Value>(request).await.map(drop).map_err(describe)
}

fn json_request(method: http::Method, path: String, body: &Value) -> Result<http::Request<Vec<u8>>, String> {
    http::Request::builder()
        .method(method)
        .uri(path)
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::ACCEPT, "application/json")
        .body(body.to_string().into_bytes())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::rand_core::UnwrapErr;
    use rsa::RsaPrivateKey;

    /// O inverso do `seal`, como o controller faz (pkg/crypto HybridDecrypt).
    fn unseal(key: &RsaPrivateKey, label: &[u8], text: &str) -> Vec<u8> {
        let data = B64.decode(text).unwrap();
        let size = u16::from_be_bytes([data[0], data[1]]) as usize;
        let session = key.decrypt(Oaep::<Sha256>::new_with_label(label.to_vec()), &data[2..2 + size]).unwrap();
        let gcm = aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM, &session).unwrap());
        let mut body = data[2 + size..].to_vec();
        let plain = gcm.open_in_place(aead::Nonce::assume_unique_for_key([0; 12]), aead::Aad::empty(), &mut body).unwrap();
        plain.to_vec()
    }

    fn entry(key: &str, value: Option<&str>) -> Entry {
        Entry { key: key.into(), value: value.map(str::to_owned), base64: false }
    }

    fn request(scope: Scope, entries: Vec<Entry>) -> Request {
        Request {
            namespace: "loja".into(),
            name: "db".into(),
            scope,
            secret_type: "Opaque".into(),
            entries,
            resource_version: None,
            apply: false,
            adopt: false,
        }
    }

    #[test]
    fn seal_roundtrip() {
        let private = RsaPrivateKey::new(&mut UnwrapErr(getrandom::SysRng), 2048).unwrap();
        let public = private.to_public_key();
        let req = request(Scope::Strict, vec![entry("password", Some("s3cr3t")), Entry { key: "bin".into(), value: Some("AP8=".into()), base64: true }]);
        let object = build(&req, None, Some(&public)).unwrap();
        let data = &object["spec"]["encryptedData"];
        assert_eq!(unseal(&private, b"loja/db", data["password"].as_str().unwrap()), b"s3cr3t");
        assert_eq!(unseal(&private, b"loja/db", data["bin"].as_str().unwrap()), [0, 255]);
        // Cada selagem usa outra chave de sessão.
        assert_ne!(seal(&public, b"x", b"a").unwrap(), seal(&public, b"x", b"a").unwrap());
        assert_eq!(object["spec"]["template"]["type"], "Opaque");
        assert_eq!(object["spec"]["template"]["metadata"]["name"], "db");
        assert!(object["metadata"]["annotations"].is_null());

        let wide = build(&request(Scope::ClusterWide, vec![entry("a", Some("b"))]), None, Some(&public)).unwrap();
        assert_eq!(unseal(&private, b"", wide["spec"]["encryptedData"]["a"].as_str().unwrap()), b"b");
        assert_eq!(wide["metadata"]["annotations"][CLUSTER_WIDE], "true");
        assert_eq!(wide["spec"]["template"]["metadata"]["annotations"][CLUSTER_WIDE], "true");
        assert_eq!(Scope::of(&wide["metadata"]), Scope::ClusterWide);
        assert_eq!(Scope::NamespaceWide.label("loja", "db"), b"loja");
    }

    #[test]
    fn edit_keeps_values() {
        let current = json!({
            "apiVersion": "bitnami.com/v1alpha1", "kind": "SealedSecret",
            "metadata": {"name": "db", "namespace": "loja", "resourceVersion": "7", "labels": {"app": "loja"}},
            "spec": {"encryptedData": {"user": "AAA", "password": "BBB"},
              "template": {"metadata": {"labels": {"team": "x"}}, "type": "kubernetes.io/basic-auth"}},
            "status": {"conditions": [{"type": "Synced", "status": "True"}]}
        });
        let mut req = request(Scope::Strict, vec![entry("user", None)]);
        req.resource_version = Some("7".into());
        let object = build(&req, Some(&current), None).unwrap();
        assert_eq!(object["spec"]["encryptedData"], json!({"user": "AAA"}));
        assert_eq!(object["spec"]["template"]["metadata"]["labels"]["team"], "x");
        assert_eq!(object["metadata"]["labels"]["app"], "loja");
        assert_eq!(object["spec"]["template"]["type"], "Opaque");
        assert!(object["status"].is_null());
        // Mudar o escopo exige selar tudo de novo.
        req.scope = Scope::NamespaceWide;
        assert!(build(&req, Some(&current), None).unwrap_err().contains("escopo"));
        // Sem o objeto atual não há o que manter.
        assert!(build(&request(Scope::Strict, vec![entry("user", None)]), None, None).is_err());

        let yaml = to_yaml(&mut exported(current.clone()), false).unwrap();
        assert!(!yaml.contains("resourceVersion"), "{yaml}");
    }

    #[test]
    fn entries() {
        assert!(check_entries(&[]).is_err());
        assert!(check_entries(&[entry("a b", Some(""))]).is_err());
        assert!(check_entries(&[entry("a", Some("")), entry("a", Some(""))]).unwrap_err().contains("repetida"));
        assert!(check_entries(&[entry(".dockerconfigjson", Some("{}")), entry("tls.crt", None)]).is_ok());
        let data = json!({"text": B64.encode("olá\n"), "bin": B64.encode([0u8, 159, 146])});
        let list = entries_of(&data);
        assert_eq!(list.iter().find(|e| e.key == "text").unwrap().value.as_deref(), Some("olá\n"));
        let bin = list.iter().find(|e| e.key == "bin").unwrap();
        assert!(bin.base64);
        assert_eq!(bin.bytes().unwrap().unwrap(), [0, 159, 146]);
    }

    #[test]
    fn open_sealed() {
        let object = json!({
            "metadata": {"resourceVersion": "9", "annotations": {NAMESPACE_WIDE: "true"}},
            "spec": {"encryptedData": {"a": "x", "b": "y"}, "template": {"type": "kubernetes.io/tls"}},
            "status": {"conditions": [{"type": "Synced", "status": "False", "message": "no key could decrypt secret"}]}
        });
        let opened = open(&object, Ok(json!({"data": {"a": B64.encode("1")}})));
        assert_eq!(opened.scope, Scope::NamespaceWide);
        assert_eq!(opened.secret_type, "kubernetes.io/tls");
        assert_eq!(opened.entries, [entry("a", Some("1")), entry("b", None)]);
        assert_eq!(opened.status, "no key could decrypt secret");
        assert_eq!(opened.resource_version, "9");
        let missing = open(&object, Err("não encontrado: secrets \"x\" not found".into()));
        assert!(missing.values_error.contains("ainda não criou"));
        assert!(missing.entries.iter().all(|e| e.value.is_none()));
    }

    #[test]
    fn controller_services() {
        let list = json!({"items": [
            {"metadata": {"name": "sealed-secrets", "namespace": "infra"}, "spec": {"ports": [{"name": "http", "port": 8080}]}},
            {"metadata": {"name": "sealed-secrets-metrics", "namespace": "infra"}, "spec": {"ports": [{"name": "metrics", "port": 8081}]}},
            {"metadata": {"name": "outro", "namespace": "x"}, "spec": {"ports": [{"port": 9000}]}}]});
        assert_eq!(
            services(&list),
            [
                Controller { namespace: "infra".into(), name: "sealed-secrets".into(), port: "http".into() },
                Controller { namespace: "x".into(), name: "outro".into(), port: "9000".into() }
            ]
        );
        let keys = json!({"items": [
            {"metadata": {"creationTimestamp": "2026-01-01T00:00:00Z"}, "data": {"tls.crt": B64.encode("velho")}},
            {"metadata": {"creationTimestamp": "2026-06-01T00:00:00Z"}, "data": {"tls.crt": B64.encode("novo")}}]});
        assert_eq!(newest_key(&keys).as_deref(), Some("novo"));
    }
}

/// Contra o controller do Sealed Secrets no cluster do contexto atual (instalado com o
/// controller.yaml da release): `cargo test --lib k8s_seal -- --ignored --test-threads=1`.
#[cfg(test)]
mod live {
    use super::*;
    use crate::k8s::{contexts, Kube};
    use std::collections::BTreeMap;

    const NS: &str = "nexo-test";

    async fn client() -> Client {
        let current = contexts(None).unwrap().current.unwrap();
        Kube::default().client(None, &current).await.unwrap()
    }

    /// Espera o controller abrir o SealedSecret (o Secret com os valores esperados).
    async fn unsealed(client: &Client, name: &str, want: &[(&str, &str)]) -> Sealed {
        for _ in 0..60 {
            let opened = sealed(client, NS, name).await.unwrap();
            assert!(opened.status.is_empty(), "o controller recusou: {}", opened.status);
            let got: BTreeMap<_, _> = opened.entries.iter().filter_map(|e| Some((e.key.as_str(), e.value.as_deref()?))).collect();
            if got == want.iter().copied().collect() {
                return opened;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        panic!("o controller não abriu {name}");
    }

    #[tokio::test]
    #[ignore]
    async fn live_seal() {
        let client = client().await;
        certificate(&client).await.unwrap();
        let name = "nexo-sealed";
        let _ = client.request::<Value>(http::Request::delete(object_path(NS, name).unwrap()).body(Vec::new()).unwrap()).await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let entry = |k: &str, v: Option<&str>| Entry { key: k.into(), value: v.map(str::to_owned), base64: false };
        let mut req = Request {
            namespace: NS.into(),
            name: name.into(),
            scope: Scope::Strict,
            secret_type: "Opaque".into(),
            entries: vec![entry("user", Some("admin")), entry("password", Some("s3cr3t\nlinha 2"))],
            resource_version: None,
            apply: false,
            adopt: false,
        };
        let yaml = save(&client, &req).await.unwrap();
        assert!(yaml.contains("kind: SealedSecret") && !yaml.contains("s3cr3t"), "{yaml}");

        req.apply = true;
        save(&client, &req).await.unwrap();
        assert!(save(&client, &req).await.unwrap_err().contains("já existe"));
        let opened = unsealed(&client, name, &[("user", "admin"), ("password", "s3cr3t\nlinha 2")]).await;

        // Edita: mantém um, troca outro, acrescenta um terceiro.
        req.resource_version = Some(opened.resource_version.clone());
        req.entries = vec![entry("user", None), entry("password", Some("nova")), entry("token", Some("abc"))];
        save(&client, &req).await.unwrap();
        let opened2 = unsealed(&client, name, &[("user", "admin"), ("password", "nova"), ("token", "abc")]).await;
        // O resourceVersion antigo é recusado.
        assert!(save(&client, &req).await.unwrap_err().contains("mudou"));

        // Escopo do namespace.
        req.resource_version = Some(opened2.resource_version);
        req.scope = Scope::NamespaceWide;
        assert!(save(&client, &req).await.unwrap_err().contains("escopo"));
        req.entries = vec![entry("user", Some("root"))];
        save(&client, &req).await.unwrap();
        let opened3 = unsealed(&client, name, &[("user", "root")]).await;
        assert_eq!(opened3.scope, Scope::NamespaceWide);

        // Selar um Secret comum que já existe, no lugar dele.
        let plain_name = "nexo-plain";
        let _ = client.request::<Value>(http::Request::delete(secret_path(NS, plain_name).unwrap()).body(Vec::new()).unwrap()).await;
        let _ = client.request::<Value>(http::Request::delete(object_path(NS, plain_name).unwrap()).body(Vec::new()).unwrap()).await;
        let secret = json!({"apiVersion": "v1", "kind": "Secret", "metadata": {"name": plain_name, "namespace": NS}, "stringData": {"k": "v1"}});
        let _: Value = client.request(json_request(http::Method::POST, format!("/api/v1/namespaces/{NS}/secrets"), &secret).unwrap()).await.unwrap();
        let p = plain(&client, NS, plain_name).await.unwrap();
        assert_eq!(p.entries, [entry("k", Some("v1"))]);
        let convert = Request {
            name: plain_name.into(),
            entries: vec![entry("k", Some("v2"))],
            resource_version: None,
            scope: Scope::Strict,
            adopt: true,
            ..req
        };
        save(&client, &convert).await.unwrap();
        unsealed(&client, plain_name, &[("k", "v2")]).await;

        for n in [name, plain_name] {
            let _ = client.request::<Value>(http::Request::delete(object_path(NS, n).unwrap()).body(Vec::new()).unwrap()).await;
        }
    }
}
