# Nexo

Terminal rápido e multiplataforma para SSH, túneis, SFTP e RDP. Inspirado no Termius e no MobaXterm.

## Rodando em desenvolvimento

```powershell
npm install
npm run tauri dev
```

Build de release (gera instalador em `src-tauri/target/release/bundle`):

```powershell
npm run tauri build
```

## Atalhos

| Atalho | Ação |
|---|---|
| `Ctrl+Shift+T` | Nova aba |
| `Ctrl+Shift+D` / `Ctrl+Shift+E` | Dividir à direita / abaixo |
| `Ctrl+Shift+W` | Fechar painel |
| `Ctrl+Shift+B` | Broadcast na aba atual |
| `Ctrl+Shift+F` | Painel de arquivos (SFTP) |
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
- Painel encerrado: `Enter` ou o botão reconecta.

## Arquitetura

- `src-tauri/` — backend Rust (Tauri 2). Cada sessão implementa o trait `Session`
  (`session.rs`): PTY local (`local.rs`) e SSH (`ssh.rs`). A saída vai ao frontend por
  `Channel` em binário; o input passa por uma fila por sessão que preserva a ordem das teclas.
  `flow.rs` pausa a leitura quando o frontend fica mais de 2 MB atrás (ack do xterm.js).
  `sftp.rs` abre o subsistema SFTP sob demanda na conexão de cada `SshSession`.
  `tunnel.rs` mantém os túneis (conexão própria via `ssh::establish`, reconexão, SOCKS5).
  `rdp.rs` é a sessão RDP (IronRDP): decodifica a tela e manda só as regiões alteradas, com ack
  por mensagem desenhada; input chega por `rdp_input` (scancodes, mouse, roda).
  Testes contra servidores reais: `cargo test -- --ignored live` (ver comentários nos testes;
  o de RDP usa um container xRDP na porta 13389).
- `src/` — frontend Svelte 5 + xterm.js (renderer WebGL). `lib/state.svelte.ts` guarda abas,
  árvore de painéis e broadcast; `lib/term.ts` encapsula cada terminal; `lib/sftp.svelte.ts`
  guarda o navegador de arquivos de cada painel SSH e as transferências; `lib/rdp.ts` é o painel
  RDP (canvas, mapa de scancodes, cursor do servidor).
