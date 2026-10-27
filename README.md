# Nexo

Terminal rápido e multiplataforma para SSH, túneis, SFTP e RDP. Inspirado no Termius e no MobaXterm.

## Rodando em desenvolvimento

```powershell
npm install
npm run tauri dev
```

## Versões e builds

- **Windows** (`.msi` e `-setup.exe`): `npm run tauri build` no Windows; os instaladores ficam em
  `src-tauri/target/release/bundle`.
- **Linux** (`.deb`, `.rpm`, `.AppImage`): `docker/linux-build/build.sh` (no WSL ou num Linux com
  Docker) compila o commit atual num Ubuntu 22.04 e põe os pacotes em `release/linux`. Rodam em
  distros com glibc 2.35+ (Ubuntu 22.04, Debian 12, Fedora 36 em diante).
- **GitHub**: `.github/workflows/release.yml` compila os dois numa tag `v*`
  (`git tag v0.1.0 && git push origin v0.1.0`) e cria uma release em rascunho com os instaladores.

A versão fica em `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` e `package.json`.
Os instaladores não são assinados: o Windows SmartScreen avisa na primeira execução
("Mais informações" → "Executar assim mesmo").

Só a interface, no navegador (`npm run dev` e abrir http://localhost:1420): fora do Tauri, o
`src/dev-mock.ts` simula o backend com dados de exemplo. `?view=credentials|tunnels|settings|session`,
`?dialog=host|newhost|credential|tunnel|palette|generate`, `?panel=host|sftp` (com `view=session`) e
`?theme=light` abrem direto uma tela.
O mock fica fora do build de produção.

## Interface

- **Hosts**: pastas e todos os hosts em cards (clique conecta; `⋯` ou botão direito: editar, duplicar,
  túnel, excluir). Com uma sessão aberta, os hosts viram uma lista ao lado (botão na barra da esquerda).
- **Credenciais**: usuário + senha, chave SSH (arquivo ou guardada no cofre) ou agente, usadas por
  vários hosts — SSH e RDP. Trocou a senha? Muda num lugar só. "Gerar chave SSH" cria um par ed25519
  e mostra a chave pública para o `authorized_keys`.
- **Túneis** e **Configurações** (tema claro/escuro/sistema, cor de destaque, fonte do terminal,
  importar do `~/.ssh/config`, importar/exportar hosts em JSON — sem senhas nem chaves).
- `Ctrl+K`: conectar a qualquer host, abrir terminal ou ir para uma página.
- Feita com [shadcn-svelte](https://shadcn-svelte.com) (bits-ui + Tailwind 4); componentes em
  `src/lib/components/ui`. Os estilos dos painéis (terminal, RDP, SFTP) ficam em `src/panes.css`.

## Atalhos

| Atalho | Ação |
|---|---|
| `Ctrl+K` | Conectar a um host / buscar |
| `Ctrl+Shift+T` | Nova aba |
| `Ctrl+Shift+D` / `Ctrl+Shift+E` | Dividir à direita / abaixo |
| `Ctrl+Shift+W` | Fechar painel |
| `Ctrl+Shift+B` | Broadcast na aba atual |
| `Ctrl+Shift+F` | Painel de arquivos (SFTP) |
| `Ctrl+Shift+M` | Painel do host (métricas e containers) |
| `Alt+setas` | Navegar entre painéis |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Próxima / anterior aba |
| `Ctrl+C` (com seleção) / `Ctrl+V` | Copiar / colar |
| Botão direito | Copia a seleção ou cola |

## SSH e cofre

- Hosts e credenciais ficam em `vault.bin` (pasta de dados do app), criptografado com
  XChaCha20-Poly1305. A chave fica no cofre do sistema (Gerenciador de Credenciais do
  Windows, Keychain no macOS, Secret Service no Linux).
- Verificação de host usa o `~/.ssh/known_hosts` do OpenSSH: o que já é confiado no `ssh`
  continua confiado no Nexo, e vice-versa.
- Autenticação por senha (com fallback para keyboard-interactive), chave ou agente SSH. Sem
  caminho de chave configurado, tenta `~/.ssh/id_ed25519`, `id_ecdsa`, `id_rsa` e depois o agente.
- Agente: no Windows, o serviço *OpenSSH Authentication Agent* (`\\.\pipe\openssh-ssh-agent`) e
  depois o Pageant; no Linux/macOS, `SSH_AUTH_SOCK`.
- Jump host (ProxyJump): qualquer host salvo pode ser o salto de outro, inclusive em cadeia.
  Vale para terminal, SFTP e túneis. Senhas/fingerprints de cada salto são pedidas separadamente.
- Importar do `~/.ssh/config` (botão de download no cabeçalho de Hosts): entende `Host` com
  curingas, `HostName`, `User`, `Port`, `IdentityFile`, `ProxyJump` e `Include`.
- Sessão SSH encerrada: `Enter` reconecta.
- Prompt de senha no terminal (`sudo`, `su`, `Password:`, `Senha:`): aparece uma lista dos hosts
  com senha salva (o host da sessão primeiro). `Enter` preenche, `Esc` ignora, digitar segue normal.
  A senha vai do cofre direto para a sessão, sem passar pelo frontend.

## SFTP

- Painel lateral (`Ctrl+Shift+F` ou botão **SFTP**) com os arquivos do painel SSH focado.
  Usa um canal extra na mesma conexão do terminal: sem novo login.
- Duplo clique abre pasta / baixa arquivo (para a pasta Downloads; "Baixar para…" no menu).
- Enviar: botão de upload ou arrastar arquivos e pastas do Explorer para o painel.
- Botão direito: renomear (`F2`), excluir (`Del`, recursivo), copiar caminho, "abrir no terminal" (`cd`).
- Transferências com progresso e cancelamento; pastas são copiadas recursivamente.

## Painel do host

- Painel lateral (`Ctrl+Shift+M` ou o botão de atividade na barra da esquerda) com o servidor do
  painel SSH focado: CPU (com gráfico dos últimos 2 minutos e carga média), memória e swap, rede
  (taxa de download/upload) e uso de cada disco. Atualiza a cada 2 segundos enquanto está aberto.
- Usa um canal extra na conexão do terminal e só lê `/proc` e `df`: nada é instalado no servidor.
  Funciona em servidores Linux (inclusive Alpine/BusyBox).
- Aba **Containers**: os containers Docker do servidor, agrupados por projeto do Compose, com
  status, CPU e memória (a cada 5 s). Iniciar, parar, reiniciar, pausar, remover (só parados).
  **Logs** (acompanhando ao vivo) e **shell** no container abrem numa aba nova, na mesma conexão
  SSH — sem novo login.
- Usuário fora do grupo `docker`: o painel oferece **usar sudo** (com confirmação; também no editor
  do host, "Docker com sudo"). A senha do sudo é a salva do host/credencial, passada direto do cofre
  para o `sudo -S`; sem senha salva, o Nexo pergunta (e pode salvar). Se a salva for recusada (sudo
  com outra senha), a digitada fica só na memória — a do login não é trocada. Logs e shell rodam
  `sudo docker …` no terminal, com a sugestão de senha salva no prompt do sudo.

## Kubernetes

- Página **Kubernetes** na barra da esquerda, no estilo do Lens. Lê o kubeconfig padrão
  (`KUBECONFIG` ou `~/.kube/config`, em `%USERPROFILE%\.kube\config` no Windows) ou outro
  arquivo escolhido no menu `…` (por exemplo `\\wsl.localhost\Ubuntu\home\<usuário>\.kube\config`).
  Plugins `exec` de autenticação (`aws`, `gke-gcloud-auth-plugin`, `kubelogin`…) funcionam como no `kubectl`.
- Escolha de contexto e namespace (ou todos), e os recursos mais usados: pods, deployments,
  statefulsets, daemonsets, jobs, services, ingresses, configmaps, secrets, PVCs, nodes, eventos…
  As colunas são as do `kubectl get`, vindas do próprio cluster (botão de colunas = `-o wide`);
  estados com cor, filtro e ordenação. A lista visível atualiza a cada 5 s.
- Clicar num item mostra o YAML (sem `managedFields`). Em Secrets os valores ficam escondidos
  até clicar em revelar.
- **Editar o YAML** (lápis no painel; `Ctrl+S` salva): o objeto é substituído com o
  `resourceVersion` que foi aberto, então se alguém mudou o objeto nesse meio-tempo o cluster
  recusa e nada é sobrescrito. Nome e namespace não mudam; Secrets só com os valores revelados.
- Com **metrics-server** no cluster, pods e nodes ganham as colunas de CPU e memória.
- **Custom resources**: os CRDs do cluster aparecem na barra, agrupados por API group, com as
  colunas definidas no CRD, YAML, edição e exclusão.
- Menu de cada item (botão `…` ou clique direito): **logs** (acompanhando ao vivo, ou do container
  anterior depois de um restart) e **shell** de pods numa aba de terminal, **port-forward**,
  **escalar** e **reiniciar** workloads (como o `kubectl rollout restart`) e **excluir** (com
  confirmação; namespaces, nodes e volumes ficam de fora). Pods com vários containers mostram cada
  um no menu do painel de YAML.
- **Port-forward** para pods, services e deployments/statefulsets: escolhe a porta (as declaradas
  aparecem como sugestão) e a local; fica em `127.0.0.1` enquanto o Nexo estiver aberto, listado na
  barra da página (clique abre no navegador). Se o pod for trocado (rollout), o próximo acesso
  escolhe outro pelo seletor.

## Túneis

- Seção **Túneis** na sidebar: `+` cria, ▶/■ inicia e para, lápis edita.
- **Local** (`-L`): porta desta máquina → destino visto pelo servidor (ex.: `localhost:5432` → banco da VM).
- **Remoto** (`-R`): porta no servidor → destino visto desta máquina.
- **SOCKS** (`-D`): proxy SOCKS5 local; o tráfego sai pela rede do servidor.
- Cada túnel tem conexão SSH própria (não depende de terminal aberto) e reconecta sozinho se cair.
  "Iniciar automaticamente" sobe o túnel quando o Nexo abre.
- O servidor precisa de `AllowTcpForwarding yes` (padrão do OpenSSH).

## RDP

- No formulário de host, escolha **RDP**: a área de trabalho abre numa aba (ou painel), como um terminal.
- Login automático: com usuário e senha salvos no cofre, o servidor pula a tela de login (o xRDP
  também). Sem senha salva, o Nexo pergunta ao conectar (com opção de salvar).
- **Gateway SSH**: qualquer host SSH salvo pode ser o túnel até o servidor RDP (jump hosts em
  cadeia valem) — útil para xRDP que só escuta na rede interna.
- Certificado TLS: confiança no primeiro uso, como o `known_hosts`; o Nexo avisa se ele mudar.
- Resolução acompanha o painel (ao vivo no xRDP 0.10+/Windows; reconecta em servidores antigos)
  ou fixa. "Alta resolução" usa os pixels físicos da tela.
- Teclado por scancode: o layout é o do servidor (o Nexo informa o layout do Windows, ex.: ABNT2).
- **Captura de atalhos** (Windows, ligada por padrão): com o painel RDP focado, *todas* as teclas vão
  para o servidor — Win, Alt+Tab, Alt+F4, Ctrl+Esc e os atalhos do próprio Nexo. **Ctrl+Alt+Home**
  solta o teclado; clicar na tela remota captura de novo. Ctrl+Alt+Del e Win+L o Windows não deixa
  capturar: use o botão da barra de conexão.
- **Área de transferência** (texto, nos dois sentidos): copiou no servidor, já está no Windows;
  copiou no Windows, fica disponível no servidor quando o painel ganha o foco.
- **GFX** (pipeline gráfico, ligado por padrão): H.264 e RemoteFX progressivo, bem mais fluido que
  bitmaps. O H.264 é decodificado pelo OpenH264 compilado junto (instalar o NASM antes do build deixa
  a decodificação mais rápida). No xRDP, o H.264 depende de o servidor ter sido compilado com ele;
  sem isso, ele usa o RemoteFX progressivo.
- **Barra de conexão**: leve o mouse à borda de cima do painel — Ctrl+Alt+Del, tela cheia, desconectar.
- Painel encerrado: `Enter` ou o botão reconecta.

## Arquitetura

- `src-tauri/` — backend Rust (Tauri 2). Cada sessão implementa o trait `Session`
  (`session.rs`): PTY local (`local.rs`) e SSH (`ssh.rs`). A saída vai ao frontend por
  `Channel` em binário; o input passa por uma fila por sessão que preserva a ordem das teclas.
  `flow.rs` pausa a leitura quando o frontend fica mais de 2 MB atrás (ack do xterm.js).
  `sftp.rs` abre o subsistema SFTP sob demanda na conexão de cada `SshSession`.
  `hostinfo.rs` lê as métricas do servidor (um script em `/proc` num canal extra da sessão);
  `docker.rs` lista os containers e executa as ações pelo CLI `docker` do servidor.
  `tunnel.rs` mantém os túneis (conexão própria via `ssh::establish`, reconexão, SOCKS5).
  `rdp.rs` é a sessão RDP (IronRDP): decodifica a tela e manda só as regiões alteradas, com ack
  por mensagem desenhada; input chega por `rdp_input` (scancodes, mouse, roda).
  Testes contra servidores reais: `cargo test -- --ignored live` (ver comentários nos testes;
  o de RDP usa um container xRDP na porta 13389).
- `src/` — frontend Svelte 5 + xterm.js (renderer WebGL). `lib/state.svelte.ts` guarda abas,
  árvore de painéis e broadcast; `lib/term.ts` encapsula cada terminal; `lib/sftp.svelte.ts`
  guarda o navegador de arquivos de cada painel SSH e as transferências; `lib/rdp.ts` é o painel
  RDP (canvas, mapa de scancodes, cursor do servidor).
