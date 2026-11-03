# Nexo: contexto e regras de trabalho

Nexo é um terminal multiplataforma (Windows, Linux, macOS) para SSH, túneis, SFTP,
RDP, Docker e Kubernetes, inspirado no Termius, MobaXterm e Lens. Backend Rust
(Tauri 2), frontend Svelte 5 + xterm.js. Licença GPL-3.0-or-later. Guarda senhas e
chaves num cofre (vault.bin, XChaCha20-Poly1305, chave no keychain do SO), então
segurança é prioridade.

Versão atual: **0.4.0** (tag `v0.4.0`). Repositório: github.com/carloseduardo21/nexo.

## Objetivo atual
Preparar o projeto para uso público pela comunidade: segurança, testes, CI
multiplataforma, documentação e releases confiáveis.

## Regras
- Trabalhe numa branch por etapa (ex.: `chore/ci`, `security/csp`) e faça commits
  pequenos em Conventional Commits.
- Antes de mexer, explore o código e me mostre um plano curto. Só então implemente.
- Não mude comportamento visível ao usuário sem avisar. Nunca mude o formato do
  vault.bin sem migração e testes, e me pergunte antes.
- Nunca invente segredos, chaves, IDs ou tokens. Se algo depender de ação manual
  minha (secrets do GitHub, conta Apple, SignPath, configurações do repositório),
  liste no final em "Ações manuais para o Carlos", com passo a passo.
- Rode `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test` e `npm run check`
  (ou equivalentes) antes de concluir e reporte o resultado. Não finja que rodou.
- Se algo não puder ser verificado neste ambiente (ex.: macOS, Windows), diga
  explicitamente.
- Ao terminar, resuma: o que mudou, o que foi testado, riscos e próximos passos.

## O que já existe (até a 0.4.0)
- **SSH** (russh com `ring`), terminal local (portable-pty; shells do Windows, WSL,
  Linux), abas com painéis divididos, broadcast, importação do `~/.ssh/config`.
- **Túneis** locais/remotos/dinâmicos e **SFTP** (painel de arquivos).
- **RDP embutido** (IronRDP): autologon, gateway via SSH, TOFU do certificado TLS no
  cofre, área de transferência, captura de atalhos no Windows (hook de teclado),
  pipeline GFX/H.264. Falta áudio.
- **Credenciais** reutilizáveis (senha, chave no arquivo ou no cofre, agente),
  geração de chave ed25519, exportação/importação de hosts em JSON (sem segredos).
- **Painel do host** (métricas CPU/memória/rede/discos e containers Docker, com
  `sudo docker` se preciso) em sessões SSH e terminais locais Linux/WSL.
- **Kubernetes** (kube-rs, kubeconfig local): navegação estilo Lens, YAML com edição,
  logs, shell, port-forward, scale/restart/delete, métricas, CRDs.
  **Sealed Secrets** com selagem nativa compatível com o kubeseal e tela de
  **Argo CD** (lê os CRDs direto da API; sync/rollback por patch, como `argocd --core`).
- **Atualização automática** (tauri-plugin-updater) pelo `latest.json` da última
  release publicada.

## Mapa do código
- `src-tauri/src/`: um módulo por área — `vault.rs` (cofre), `commands.rs` (comandos
  Tauri), `ssh.rs`, `sftp.rs`, `tunnel.rs`, `local.rs`/`shells.rs` (terminal local),
  `rdp.rs`/`rdp_clipboard.rs`/`keyhook.rs`, `hostinfo.rs`/`docker.rs`,
  `k8s*.rs` (`k8s_seal.rs`, `k8s_argo.rs`, `k8s_forward.rs`, `k8s_term.rs`).
  Registro dos comandos em `lib.rs`.
- `src-tauri/capabilities/default.json`: permissões do frontend. APIs novas do Tauri
  precisam de permissão aqui. `tauri.conf.json` ainda está com `"csp": null`.
- `src/`: `App.svelte`, páginas e diálogos em `src/components/`, estado em
  `src/lib/*.svelte.ts`, chamadas ao backend em `src/lib/backend.ts`, componentes
  shadcn-svelte (bits-ui + Tailwind 4) em `src/lib/components/ui`.
- `src/dev-mock.ts`: simula o backend no navegador (`npm run dev`, porta 1420), com
  parâmetros de URL para abrir telas (ver README). Fica fora do build de produção.
- `.github/workflows/`: `release.yml` (tag `v*` → release em rascunho com instaladores
  Windows/Linux, assinatura do updater e `latest.json`) e `cache.yml` (aquece o cache
  do Rust na main para a release). `.signpath/`: configurações da SignPath.
- `docker/linux-build/` (build Linux local num Ubuntu 22.04) e `docker/xrdp/`
  (servidor xRDP para testes).

## Comandos
- Dev: `npm install` e `npm run tauri dev`. Só a interface: `npm run dev`.
- Frontend: `npm run check` (svelte-check; não há ESLint nem testes de frontend ainda),
  `npm run build`.
- Rust (em `src-tauri/`): `cargo fmt`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`. Testes marcados `#[ignore]` são **testes ao vivo** (precisam de
  servidor SSH/xRDP, Docker, cluster kind com Sealed Secrets/Argo CD) e não entram no
  CI; rode-os com `--ignored --test-threads=1` só quando o ambiente existir.
- Ambiente local: o código fica em `C:\dev\nexo` e é editado pelo WSL, mas o
  toolchain Rust e o npm do app são os do Windows (`cargo.exe`, `npm.cmd` via
  `cmd.exe`). O `git push` funciona pelo `git.exe` (credenciais do Windows); não há
  `gh` instalado.

## Versões e releases
- A versão fica em três lugares, sempre juntos: `package.json`,
  `src-tauri/Cargo.toml` e `src-tauri/tauri.conf.json`.
- Release: `git tag vX.Y.Z && git push origin vX.Y.Z` → rascunho no GitHub; o Carlos
  publica. Só releases publicadas chegam ao auto-update.
- Assinatura do updater: segredo `TAURI_SIGNING_PRIVATE_KEY` (par da `pubkey` do
  `tauri.conf.json`). Não ligar `createUpdaterArtifacts` (exigiria a chave em builds
  locais).
- Windows sai sem assinatura de código: a SignPath Foundation recusou o projeto por
  falta de visibilidade (pode-se tentar de novo depois). O Defender já deu falso
  positivo no instalador; mudanças que pareçam "RAT" (processos ocultos, hooks)
  merecem cuidado.
- macOS ainda não tem build nem release.
