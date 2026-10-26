//! Mede a latência de ida e volta de uma tecla via SSH (russh), com um servidor de eco
//! local. Isola o custo do processamento no cliente — rede fica fora da conta.
//!
//!     cargo run --example ssh_latency            (debug)
//!     cargo run --example ssh_latency --release  (otimizado)

use std::sync::Arc;
use std::time::{Duration, Instant};

use russh::keys::ssh_key::private::Ed25519Keypair;
use russh::keys::{PrivateKey, PublicKeyOrCertificate};
use russh::server::{Msg, Server as _, Session};
use russh::{client, server, Channel, ChannelId, ChannelMsg};

#[derive(Clone)]
struct Echo;

impl server::Server for Echo {
    type Handler = Self;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
        Echo
    }
}

impl server::Handler for Echo {
    type Error = russh::Error;

    async fn auth_password(&mut self, _: &str, _: &str) -> Result<server::Auth, Self::Error> {
        Ok(server::Auth::Accept)
    }

    async fn channel_open_session(
        &mut self,
        _: Channel<Msg>,
        reply: server::ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn data(&mut self, channel: ChannelId, data: &[u8], session: &mut Session) -> Result<(), Self::Error> {
        session.data(channel, data.to_vec())?;
        Ok(())
    }
}

struct Trust;

impl client::Handler for Trust {
    type Error = russh::Error;
    async fn check_server_key(&mut self, _: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

#[tokio::main]
async fn main() {
    let key = PrivateKey::from(Ed25519Keypair::from_seed(&[7; 32]));
    let server_config = Arc::new(server::Config { keys: vec![key], nodelay: true, ..Default::default() });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { Echo.run_on_socket(server_config, &listener).await });

    let t = Instant::now();
    let config = Arc::new(client::Config { nodelay: true, ..Default::default() });
    let socket = tokio::net::TcpStream::connect(addr).await.unwrap();
    socket.set_nodelay(true).unwrap();
    let mut handle = client::connect_stream(config, socket, Trust).await.unwrap();
    assert!(handle.authenticate_password("u", "p").await.unwrap().success());
    let mut channel = handle.channel_open_session().await.unwrap();
    println!("handshake + auth:  {:>8.2} ms", t.elapsed().as_secs_f64() * 1000.0);

    let mut samples = Vec::new();
    for i in 0..2000 {
        let t = Instant::now();
        channel.data_bytes(vec![b'a' + (i % 26) as u8]).await.unwrap();
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Data { .. }) => break,
                Some(_) => continue,
                None => panic!("canal fechou"),
            }
        }
        samples.push(t.elapsed());
    }
    report("tecla (1 byte)", &mut samples);

    // Uma "tela cheia" de saída (ex.: redesenho do htop), 8 KB.
    let mut samples = Vec::new();
    for _ in 0..300 {
        let t = Instant::now();
        channel.data_bytes(vec![b'x'; 8192]).await.unwrap();
        let mut got = 0;
        while got < 8192 {
            if let Some(ChannelMsg::Data { data }) = channel.wait().await {
                got += data.len();
            }
        }
        samples.push(t.elapsed());
    }
    report("tela 8 KB", &mut samples);
}

fn report(name: &str, samples: &mut [Duration]) {
    samples.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let p = |q: f64| ms(samples[((samples.len() as f64 * q) as usize).min(samples.len() - 1)]);
    println!("{name:<18} p50 {:>6.3} ms   p90 {:>6.3} ms   p99 {:>6.3} ms", p(0.5), p(0.9), p(0.99));
}
