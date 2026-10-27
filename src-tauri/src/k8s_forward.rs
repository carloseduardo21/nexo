//! Port-forward para pods, services e workloads (como o `kubectl port-forward`).
//!
//! Escuta numa porta local; cada conexão aceita abre um port-forward na API do cluster para
//! o pod escolhido. Se o pod sumiu (rollout, reinício), escolhe outro e tenta de novo.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use k8s_openapi::api::core::v1::Pod;
use kube::{Api, Client};
use serde::Serialize;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use crate::k8s::{self, describe, Kind};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ForwardInfo {
    pub id: u32,
    pub context: String,
    pub namespace: String,
    pub kind: Kind,
    pub name: String,
    pub port: u16,
    pub local_port: u16,
    pub connections: usize,
    /// Último erro de uma conexão ('' = nenhum).
    pub error: String,
}

struct Running {
    info: ForwardInfo,
    state: Arc<State>,
    task: JoinHandle<()>,
}

#[derive(Default)]
struct State {
    connections: AtomicUsize,
    error: Mutex<String>,
    /// Pod e porta atuais (re-resolvidos quando falham).
    target: Mutex<(String, u16)>,
}

#[derive(Default)]
pub struct Forwards {
    next_id: AtomicU32,
    running: Mutex<HashMap<u32, Running>>,
}

impl Forwards {
    pub fn list(&self) -> Vec<ForwardInfo> {
        let mut list: Vec<ForwardInfo> = self
            .running
            .lock()
            .unwrap()
            .values()
            .map(|r| ForwardInfo {
                connections: r.state.connections.load(Ordering::Relaxed),
                error: r.state.error.lock().unwrap().clone(),
                ..r.info.clone()
            })
            .collect();
        list.sort_by_key(|f| f.id);
        list
    }

    pub fn stop(&self, id: u32) {
        if let Some(running) = self.running.lock().unwrap().remove(&id) {
            running.task.abort();
        }
    }

    /// `local_port` 0 = a mesma do pod, se estiver livre; senão uma qualquer.
    #[allow(clippy::too_many_arguments)]
    pub async fn start(
        &self,
        client: Client,
        context: &str,
        namespace: &str,
        kind: Kind,
        name: &str,
        port: u16,
        local_port: u16,
    ) -> Result<ForwardInfo, String> {
        // Resolve já: erros (sem pod rodando, porta inexistente) aparecem na hora.
        let target = k8s::resolve(&client, kind, namespace, name, port).await?;
        let listener = if local_port == 0 {
            match TcpListener::bind(("127.0.0.1", port)).await {
                Ok(l) => l,
                Err(_) => TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| e.to_string())?,
            }
        } else {
            TcpListener::bind(("127.0.0.1", local_port))
                .await
                .map_err(|e| format!("não deu para usar a porta local {local_port}: {e}"))?
        };
        let local_port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let info = ForwardInfo {
            id,
            context: context.to_owned(),
            namespace: namespace.to_owned(),
            kind,
            name: name.to_owned(),
            port,
            local_port,
            connections: 0,
            error: String::new(),
        };
        let state = Arc::new(State { target: Mutex::new(target), ..Default::default() });
        let spec = Spec { client, namespace: namespace.to_owned(), kind, name: name.to_owned(), port };
        let task = tokio::spawn(accept(listener, Arc::new(spec), state.clone()));
        self.running.lock().unwrap().insert(id, Running { info: info.clone(), state, task });
        Ok(info)
    }
}

impl Drop for Forwards {
    fn drop(&mut self) {
        for running in self.running.get_mut().unwrap().values() {
            running.task.abort();
        }
    }
}

struct Spec {
    client: Client,
    namespace: String,
    kind: Kind,
    name: String,
    port: u16,
}

async fn accept(listener: TcpListener, spec: Arc<Spec>, state: Arc<State>) {
    // As conexões ficam nesta task: parar o forward (abort) derruba todas junto.
    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((socket, _)) = accepted else { continue };
                let _ = socket.set_nodelay(true);
                let (spec, state) = (spec.clone(), state.clone());
                connections.spawn(async move {
                    state.connections.fetch_add(1, Ordering::Relaxed);
                    let result = relay(socket, &spec, &state).await;
                    state.connections.fetch_sub(1, Ordering::Relaxed);
                    *state.error.lock().unwrap() = result.err().unwrap_or_default();
                });
            }
            Some(_) = connections.join_next() => {}
        }
    }
}

async fn relay(mut socket: TcpStream, spec: &Spec, state: &State) -> Result<(), String> {
    let api: Api<Pod> = Api::namespaced(spec.client.clone(), &spec.namespace);
    let (pod, mut port) = state.target.lock().unwrap().clone();
    let mut forwarder = match api.portforward(&pod, &[port]).await {
        Ok(f) => f,
        // O pod pode ter sido trocado: escolhe de novo e tenta mais uma vez.
        Err(first) if spec.kind != Kind::Pod => {
            let target = k8s::resolve(&spec.client, spec.kind, &spec.namespace, &spec.name, spec.port)
                .await
                .map_err(|_| describe(first))?;
            *state.target.lock().unwrap() = target.clone();
            port = target.1;
            api.portforward(&target.0, &[port]).await.map_err(describe)?
        }
        Err(e) => return Err(describe(e)),
    };
    let mut upstream = forwarder.take_stream(port).ok_or("port-forward sem stream")?;
    // Erro do lado do pod (ex.: nada escutando na porta) chega por um canal à parte, e o
    // stream de dados nem sempre fecha junto.
    let error = forwarder.take_error(port).ok_or("port-forward sem canal de erro")?;
    let result = tokio::select! {
        copied = tokio::io::copy_bidirectional(&mut socket, &mut upstream) => copied.map(drop).map_err(|e| e.to_string()),
        Some(message) = error => Err(if message.contains("connection refused") {
            format!("nada escutando na porta {port} do pod")
        } else {
            message.trim().to_owned()
        }),
    };
    drop(upstream);
    forwarder.abort();
    result
}
