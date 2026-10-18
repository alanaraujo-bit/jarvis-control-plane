# Modo Chat — desenho (M29)

**Estado:** aprovado para construir · 2026-10-18 (o Alan disse "pode seguir, faça
como achar melhor, sempre pensando na experiência do usuário final")
**Responsável pelo documento depois de construído:** `docs/M29-CHAT.md` (este
arquivo vira a seção de desenho dele).

Leia antes: `docs/HANDOFF.md` §1–§3, D2, D3, D10–D16, D35, D41, D51 em
`docs/DECISIONS.md`.

---

## 1. Intenção

Um segundo jeito de trabalhar com os agentes, além do terminal. O terminal
continua sendo o padrão do app.

Palavras do Alan: "algo mais intuitivo… igual do Claude Code, igual do ChatGPT,
algo mais sucinto, algo mais simples. Para pessoas leigas é mais fácil… ao invés
de uma tela de terminal."

Quem usa: uma pessoa que não quer — ou não sabe — ler um terminal. Ela escreve
num campo, lê a resposta e vê o trabalho do agente explicado em linguagem
simples. Ela troca o modelo e a potência no próprio chat. Quando o agente
precisa dela (permissão, pergunta), aparece um cartão com botões, não um prompt
numerado de TUI.

É **o mesmo agente** (Claude Code ou Codex), no mesmo projeto, com a mesma
conta, as mesmas configurações da pessoa, o mesmo guardrail e o mesmo log de
sessão. Só a forma de conversar muda.

## 2. O que medi antes de desenhar (2026-10-18, esta máquina)

Claude Code 2.1.302 e Codex 0.163.0, num repositório scratch dentro do
scratchpad da sessão, ambiente sem os marcadores de sessão-filha (D7). Os
scripts de medição ficam fora do repo; o que eles provaram está abaixo.

### Claude Code: `claude -p --input-format stream-json --output-format stream-json --verbose`

1. **O processo fica vivo entre turnos.** Cada linha `{"type":"user",…}` no
   stdin é um turno; cada turno termina com uma linha `{"type":"result"}`. Só
   sai quando o stdin fecha.
2. **O transcript é gravado igual ao do modo interativo**, em
   `<config>/projects/<cwd-codificado>/<session-id>.jsonl` quando se passa
   `--session-id`, com `stop_reason: "end_turn"` nos fins de turno. O tailer de
   `session::transcript` e `claude::parse_line` funcionam sem mudança. (Linhas
   novas como `api-request-blob` já são ignoradas porque `parse_line` só lê
   `type` `user`/`assistant`.)
3. **`--permission-prompt-tool stdio` é obrigatório** para pedidos de permissão
   chegarem ao app. Sem ele, todo pedido vira `system/permission_denied` e o
   agente diz "precisa de aprovação". Com ele, chega um
   `control_request` `can_use_tool` com `tool_name`, `display_name`, `input`,
   `description`, `permission_suggestions`, `decision_reason`, `tool_use_id`.
   Resposta: `control_response` `success` com
   `{"behavior":"allow","updatedInput":…}` ou `{"behavior":"deny","message":…}`.
4. **Perguntas do agente:** com `--permission-prompt-tool stdio` a ferramenta
   `AskUserQuestion` existe (sem ele, não). A pergunta chega como
   `can_use_tool` com `tool_name: "AskUserQuestion"`,
   `requires_user_interaction: true` e `input.questions[]` (`question`,
   `header`, `options[{label,description}]`, `multiSelect`). Resposta: `allow`
   com `updatedInput` = o input original + `answers: {"<pergunta>": "<rótulo>"}`.
   Medido: o agente respondeu "Blue." à escolha enviada.
5. **Modelo no meio da sessão:** `control_request` `set_model {model}` →
   sucesso, e o turno seguinte veio de `claude-sonnet-5-5`. O transcript ganha
   três linhas `user`: `<local-command-caveat>` (com `isMeta: true`),
   `<command-name>/model…` e `<local-command-stdout>Set model to…` (as duas sem
   `isMeta`). **Essas duas hoje viram balões de "Você" na Conversation View** —
   bug que já existe no terminal quando alguém roda `/model`; corrigido aqui.
6. **Esforço no meio da sessão:** `control_request`
   `apply_flag_settings {settings:{effortLevel}}` → sucesso. Níveis aceitos por
   `--effort`: `low, medium, high, xhigh, max`.
7. **O que está valendo agora:** `control_request` `get_settings` responde
   `applied: {model: "claude-opus-5-5", effort: "xhigh"}` — e reflete `--model`
   e `--effort` da linha de comando. Sem tokens gastos (não inicia turno).
8. **Lista de modelos vem do CLI:** a resposta do `initialize` traz `models[]`
   com `value`, `resolvedModel`, `displayName`, `description`,
   `supportedEffortLevels`. Também traz `account` (e-mail, plano).
9. **Interromper:** `control_request` `interrupt` → `{"still_queued":[]}` e o
   turno termina com `result/error_during_execution`.
10. **Texto ao vivo:** `--include-partial-messages` emite
    `{"type":"stream_event","event":{…}}` com `content_block_delta`/`text_delta`.
11. **Cobrança:** cada turno `-p` emite `rate_limit_event` contra as janelas
    `five_hour`/`seven_day` da assinatura, com `isUsingOverage: false` e
    `fallback_credit: null`. Hoje um chat gasta da mesma cota que o terminal. A
    página de suporte da Anthropic (artigo 15036540, atualização de
    2026-10-07) diz que `claude -p` "ainda" usa os limites da assinatura e que a
    mudança anunciada para junho está pausada. Se voltar, o chat passa a ser
    cobrado diferente do terminal; fica registrado como risco em D54.
12. **`set_model` e `apply_flag_settings` não gravam nas configurações da
    pessoa.** Depois das medições, `get_settings` ainda mostrava
    `userSettings.model = "opus"` e só o esforço do Opus em `modelSettings`.
13. **Confiança na pasta:** o `-p` pula o diálogo "Do you trust this folder?"
    (dito no próprio `--help`). Um chat numa pasta nunca confiada carregaria
    hooks, MCP e configurações do projeto sem perguntar.
14. O caminho do projeto precisa ser o longo. Com o caminho 8.3
    (`ALANAR~1`) o Claude Code bloqueou escrita por segurança. Projetos do app
    já usam o caminho longo; o QA também precisa usar.

### Codex: `codex app-server` (JSON-RPC 2.0 por linha no stdio)

1. `initialize` → `initialized` → `model/list` → `thread/start {cwd}` →
   `turn/start {threadId, input:[{type:"text",text}], model?, effort?}`.
2. **O rollout é gravado** em `<CODEX_HOME>/sessions/AAAA/MM/DD/rollout-…-<id>.jsonl`
   no mesmo envelope que `codex::parse_line` lê (`response_item`,
   `event_msg/task_complete`, `token_count`). A resposta do `thread/start` traz
   o `path` exato, o `model` e o `reasoningEffort` em uso.
3. **`model/list`** traz por modelo `model`, `displayName`, `description`,
   `isDefault`, `hidden`, `defaultReasoningEffort` e
   `supportedReasoningEfforts[{reasoningEffort,description}]` (aqui:
   `low…max`, alguns com `ultra`).
4. **Modelo e esforço por turno:** `turn/start` com `model`/`effort` vale "para
   este turno e os seguintes". Medido: o turno seguinte veio de `gpt-6-astra`.
5. **Aprovações chegam como pedidos do servidor:**
   `item/commandExecution/requestApproval` e `item/fileChange/requestApproval`
   (`decision`: `accept`, `acceptForSession`, `decline`, `cancel`). Medido
   com `approvalPolicy: "untrusted"`.
6. **Perguntas:** `item/tool/requestUserInput` existe, mas o Codex responde
   "request_user_input is unavailable in Default mode". No modo padrão, a
   pergunta veio como `agentMessage` com `questions: [{title, options[]}]`.
7. **Interromper:** `turn/interrupt {threadId, turnId}` → `turn/completed` com
   `status: "interrupted"`.
8. **Texto ao vivo:** `item/agentMessage/delta`.
9. **Mandar mensagem com o turno andando:** `turn/steer` existe no protocolo.

### O que isso decide

- **O chat fala o protocolo estruturado de cada CLI. Não digita num PTY
  escondido.** Digitar exigiria tudo que D16 e `notify::detect` mostraram ser
  frágil: esperar o prompt, ler a tela, adivinhar quando apertar Enter.
- **A conversa durável continua vindo do transcript** que o provedor grava
  (D3), pelo mesmo tailer. Uso, busca (§51), analytics, notificação de fim de
  turno (§49), cota e títulos (§88) continuam funcionando sem mudança.
- **O stdout do protocolo serve ao vivo:** texto em streaming, pedidos de
  permissão/pergunta, estado do turno, lista de modelos, modelo/esforço em uso.

## 3. Objetivos

1. Ao abrir uma sessão nova a pessoa escolhe **Chat** ou **Terminal**. Nas
   Configurações ela escolhe qual dos dois vem primeiro.
2. Uma tela de chat limpa, no estilo Claude desktop:
   - balões só para o que a pessoa escreveu;
   - resposta do agente em Markdown legível, escrevendo ao vivo;
   - o que o agente fez (ler, editar, rodar) numa linha discreta e
     recolhível, em linguagem simples;
   - um indicador "Trabalhando…" em âmbar enquanto o agente trabalha.
3. Permissão = cartão com **Permitir**, **Permitir sempre nesta conversa**
   e **Negar**. Pergunta = cartão com as opções clicáveis.
4. Modelo e potência num seletor dentro da caixa de texto
   ("Opus 5.5 · Alto"). A lista vem do próprio CLI. A troca vale a partir da
   próxima mensagem, sem reiniciar nada.
5. Parar o agente com um clique (o botão Enviar vira Parar).
6. Ditado por voz (§54) e o Caderno (M19) entregam o texto na caixa do chat, não
   no terminal.
7. Uma sessão de chat aparece no Histórico, na busca e nas notificações como
   qualquer outra. Continuar uma sessão de chat do Histórico abre um chat.

## 4. Não objetivos (fora deste corte, ditos em voz alta)

- **Trocar uma sessão de chat para terminal (ou o contrário) no meio.** Um chat
  não tem TUI para mostrar. Seria relançar o processo com `--resume`, como a M26
  faz; fica para depois.
- **Rodízio automático de conta (M26) em sessões de chat.** O motor digita no
  terminal (`type_text`) para continuar o turno; num chat isso não existe. O
  motor **ignora** sessões de chat, então nunca digita numa. A escolha da conta
  ao *abrir* um chat continua a mesma do terminal (`accounts::active`).
- Chat para o provedor Local (§92). O `app-server` com `--oss` não foi medido.
  O item Local fica só em Terminal.
- Chat em tela dividida (§20). Uma aba de chat não entra no split.
- Anexar imagem no chat (§22). O protocolo aceita; não foi medido.
- O piloto automático (§32) continua usando terminal.
- Continuar no Histórico uma sessão de chat **do Codex**: o Codex continua
  `AppendInPlace` (D41), sem botão, igual ao terminal.
- Brief do Codex via `developerInstructions`. Parece possível, não foi medido.
  O Codex continua sem brief, como no terminal.

## 5. Anti-objetivos (falhou mesmo se "funcionar")

- O leigo ver JSON, ANSI, caminho 8.3, `tool_use_id` ou nome interno de
  ferramenta cru como texto principal.
- Uma permissão aparecer só no terminal escondido, e o chat ficar parado sem
  explicar.
- Uma segunda fonte da verdade da conversa (§23). O chat não guarda conversa em
  lugar nenhum além do log da sessão.
- Um seletor de modelo com lista inventada no app. Os modelos vêm do CLI (§26).
- Âmbar como decoração. Âmbar só para "o agente está trabalhando / esperando
  você" (§6).
- Um botão que não faz nada (§81). O que não está construído está ausente.
- Um rótulo de modelo ou de potência que não seja o que o provedor diz estar
  valendo (§28). Depois de cada troca, o valor mostrado é o que o CLI confirma.

## 6. Arquitetura

### 6.1 Um modo é um dado da sessão

- Migração nova: `sessions.mode TEXT NOT NULL DEFAULT 'terminal'`
  (`'terminal' | 'chat'`). `SessionInfo` ganha `mode`. Uma sessão nunca muda de
  modo.
- `session_start` recebe `mode: Option<SessionMode>` (ausente = terminal). Uma
  sessão de chat só pode ser `claude-code` ou `codex`. Os demais são recusados
  com `session.chatUnsupported`.
- Capacidade nova no modelo de provedores (§26): `chat: ChatSupport`, com
  `StreamJson` (Claude Code), `AppServer` (Codex) e `None` (Local). Um teste
  falha se os dois descreverem o chat igual. O launcher e a UI leem a
  capacidade; não fazem `match` no id.

### 6.2 O processo de uma sessão de chat

`LiveSession` continua sendo a unidade. Gerenciador, lista de sessões vivas,
fechar, atenção, cota, título e histórico continuam funcionando. O que muda é o
processo por trás dela:

- `LiveSession` passa a guardar `Process::Terminal(PtyHandle)` ou
  `Process::Chat(ChatHandle)`. `write`, `resize`, `kill`, `exited` e
  `replace_process_with` decidem pelo tipo:
  - `write` num chat é recusado com `session.chatMode`. É o que impede o ditado,
    o Caderno, o piloto e o rodízio de digitarem bytes no stdin de um protocolo.
  - `resize` num chat é um no-op.
  - `replace_process_with` num chat é recusado.
- `ChatHandle` = `std::process::Child` com stdin/stdout/stderr em pipe, criado
  por `envscan::tool_command` (resolve o shim `.cmd` do npm, sem janela),
  com os marcadores de sessão-filha removidos (D7), o ambiente da conta
  (`accounts::provider_env`) e preso num `pty::job::ProcessJob` (a árvore morre
  com a sessão e com o app).
- Uma thread lê o stdout linha por linha e entrega ao **driver** do provedor.
  Fim do stdout = `Lifecycle::Exited` + estado `Completed`/`Failed`, como faz o
  pump do PTY.
- O que vai para o log da sessão (§23, um único escritor):
  - `Lifecycle::Started` com o comando real (como hoje);
  - o transcript, pelo tailer de sempre;
  - cada pedido de permissão/pergunta **e** sua resolução, como frame
    `Approval` (o tipo reservado para isso, kind 50), com
    `{"source":"chat", …}`;
  - cada troca de modelo/esforço como `Lifecycle::ChatSettings {model, effort}`;
  - uma interrupção pedida pela pessoa como `Lifecycle::Interrupted`.
- **O stream bruto do protocolo não vai para o log.** A conversa é o transcript
  (D3), e os deltas de streaming são transitórios, como o repaint de um
  terminal.

### 6.3 Os drivers (Rust, `src-tauri/src/chat/`)

Funções puras por provedor, testadas com linhas reais capturadas na medição:

- `claude.rs`: argumentos de lançamento; codifica mensagem, resposta de
  permissão, `set_model`, `apply_flag_settings`, `get_settings`, `interrupt`;
  lê cada linha do stdout e devolve eventos normalizados.
- `codex.rs`: a sequência `initialize` → `thread/start`; correlação de
  respostas por id; `turn/start` com modelo/esforço pendentes; `turn/steer`
  quando já há turno andando; `turn/interrupt`; respostas aos pedidos do
  servidor.
- `mod.rs`: o runtime da sessão — estado vivo (modelos, modelo/esforço atuais,
  ocupado, rascunho do texto em streaming, pedidos pendentes, fila de mensagens
  enviadas antes do `Ready`) e os comandos Tauri.

**Lançamento do Claude Code em chat:**

```
claude -p --input-format stream-json --output-format stream-json --verbose
       --include-partial-messages --permission-prompt-tool stdio
       --session-id <nosso id>
       [--resume <id> --fork-session]   # continuar do Histórico (D41)
       [--settings <guardrail-settings.json>]
       [--append-system-prompt-file <project-brief.md>]
```

Sem `--model`, `--effort` ou `--permission-mode`: o chat começa com o que a
pessoa já configurou no CLI, igual ao terminal. Os mesmos arquivos de guardrail
e brief que `launch` já escreve.

**Lançamento do Codex em chat:** `codex app-server` (com `CODEX_HOME` da conta).
`thread/start {cwd}`, sem `approvalPolicy`/`sandbox`: vale a configuração da
pessoa. O hook de guardrail do Codex continua o mesmo arquivo no projeto
(D10, `PreExecutionWhenTrusted`). O tailer localiza o rollout pela correlação de
sempre.

**Eventos normalizados para a tela** (por um `Channel` do Tauri, só enquanto a
tela de chat está anexada):

| Evento | Claude Code | Codex |
|---|---|---|
| `ready {models, model, effort}` | resposta do `initialize` + `get_settings.applied` | `model/list` + resposta do `thread/start` |
| `settings {model, effort}` | `get_settings` depois de cada troca | o que foi mandado no último `turn/start` |
| `busy {busy}` | mandou mensagem → `true`; `result` → `false` | `turn/started` / `turn/completed` |
| `draft {text}` | `stream_event` `text_delta` acumulado por bloco | `item/agentMessage/delta` acumulado por item |
| `draftDone` | fim do bloco / linha `assistant` | `item/completed` `agentMessage` |
| `request {…}` | `can_use_tool` | `…/requestApproval`, `item/tool/requestUserInput` |
| `resolved {id}` | depois da resposta | `serverRequest/resolved` |
| `quickReplies {options}` | — | `agentMessage.questions` |
| `exited {code}` | fim do stdout | fim do stdout |

**Pedido normalizado:** `{id, kind: "permission" | "question", tool, title,
detail, canAlways, questions?}`. `title`/`detail` são montados da entrada da
ferramenta (comando, caminho do arquivo, URL), nunca do JSON cru.

**"Permitir sempre nesta conversa":**
- Claude: `allow` com `updatedPermissions` = as `permission_suggestions` do
  pedido, **com `destination` reescrita para `"session"`**. A sugestão do Bash
  vem com `localSettings`, que gravaria no `.claude/settings.local.json` do
  repositório da pessoa — nada nosso vai para o repo dela (D23). Sem sugestões,
  o botão não aparece.
- Codex: `acceptForSession`.

**Estado da sessão (§21) fica exato, sem o heurístico de 16 ms:** `Working`
enquanto `busy`, `Waiting` enquanto há pedido pendente, `Idle` no resto.

### 6.4 Guardrail, notificação, atenção

- Anexar/desanexar a tela de chat chama
  `guardrail::sessions::set_attended(true/false)`, como o terminal faz. Um
  `ask` do guardrail vira pedido do Claude e chega como cartão. Sem tela
  anexada, o guardrail recusa (D12), igual ao terminal.
- Pedido com ninguém olhando → `notify::bus::raise(Reason::ProviderPrompt,
  Confidence::Official, …)`. É **Official** porque o provedor declarou pelo
  protocolo; o terminal só consegue Observed (D35). A prévia é o texto do
  próprio agente (comando, arquivo ou pergunta). A supressão "a pessoa já está
  olhando" é a de sempre: a aba de chat ativa entra em `setVisibleSessions` como
  a do terminal.
- `notify::watch` (ler a tela) **não** é ligado para chat: não há tela.
- Fim de turno continua vindo do transcript → `announce_turn_ended` como hoje.

### 6.4.1 Confiança na pasta

O chat não começa numa pasta que o provedor nunca confiou. O `launch` lê a
confiança com os leitores que já existem — `claude::folder_is_trusted_in` /
`codex::folder_is_trusted_in`, no diretório da conta ativa (ou o ambiente, sem
conta) — e recusa com `chat.folderNotTrusted` quando a resposta é
`Some(false)`. `None` (não dá para saber) não bloqueia, como no resto do código
(§28). Nada aqui grava confiança: é decisão da pessoa, na tela do próprio
provedor (D51 item 5).

A tela explica e dá o caminho: um cartão "Antes do primeiro chat neste projeto,
o {provedor} precisa que você confie nesta pasta. É uma vez só, na tela do
próprio {provedor}." com **Abrir para confiar**, que abre uma aba de terminal
do mesmo provedor no projeto. Depois de confiar, o chat abre normalmente.

### 6.5 Rodízio (M26)

`AppHost::live_sessions` passa a pular sessões `mode = 'chat'`. Sem isso o motor
poderia relançar um chat como TUI e digitar nele.

### 6.6 Ditado e Caderno

- `voice_stop_recording` não digita numa sessão de chat; só devolve o texto. A
  caixa do chat insere o `lastTranscript` no cursor, sem enviar (§54: nunca
  auto-enviado).
- O Caderno: `surfaces/terminal/live.ts` ganha um registro paralelo de caixas de
  chat. "Enviar para o prompt" insere o texto (com quebras de linha) na caixa do
  chat da sessão em tela, sem enviar.

### 6.7 Preferência

`chat.defaultMode` na tabela `settings` (D33: validado no core; "não definido"
= sem linha = `terminal`). `Preferences` ganha `agentsOpenAs: "terminal" |
"chat"`. Comando dedicado `settings_set_agents_open_as`, como
`settings_set_performance_hud`. Fica em **Aparência**: é a seção que um leigo
abre primeiro.

## 7. A tela

### 7.1 Abrir

- Menu "+" da faixa de abas: dois grupos com título, **Chat** (Claude, Codex) e
  **Terminal** (Shell, Claude Code, Codex, Local). O grupo da preferência vem
  primeiro. Os itens indisponíveis (CLI ausente) ficam desabilitados com
  explicação, como hoje.
- Estado vazio do projeto: com preferência Chat, o botão principal é "Novo chat"
  (Claude se pronto, senão Codex), e "Shell" fica como secundário.
- A aba de chat tem um ícone de balão (Lucide `MessageSquare`) no lugar do
  ponto, e título "Claude"/"Codex" (numerado como hoje).
- Com uma aba de chat ativa, somem do cabeçalho: o seletor Terminal/Conversa, o
  botão de dividir e o HUD de performance (lê o terminal). Ficam a cota da sessão
  e o microfone.

### 7.2 O chat

Coluna central de leitura (máx. ~46rem), tipografia do app (Inter 13.5px base;
resposta do agente um pouco maior para leitura corrida), espaçamento generoso.
Temas escuro e claro desenhados cada um com os tokens existentes.

- **Mensagem da pessoa:** balão à direita, superfície neutra elevada, cantos
  arredondados, texto selecionável.
- **Resposta do agente:** texto corrido à esquerda, sem balão, em Markdown:
  títulos, parágrafos, listas (um nível de aninhamento), citação, código inline,
  bloco de código (mono, fundo sutil, botão Copiar), tabela, negrito, itálico,
  links. Links `http(s)` abrem no navegador pelo plugin opener; o resto é texto.
  Renderizado como elementos React (nada de `innerHTML`): a saída do agente pode
  trazer HTML de páginas da web, e a webview tem IPC.
- **Atividade:** chamadas de ferramenta e raciocínio entre duas falas viram **uma
  linha** "Trabalhou em N passos" (ícone + chevron). Aberta, cada passo em
  linguagem simples: "Leu `src/app.ts`", "Editou `login.ts`", "Executou
  `pnpm test`", "Pesquisou na web", "Pensou". Mostra ✓/✗ quando há resultado.
  Ferramenta desconhecida: "Usou {nome}".
- **Trabalhando:** enquanto `busy`, ao pé da conversa, um ponto âmbar pulsando +
  "Trabalhando…" (ou "Pensando…" antes do primeiro texto). Respeita
  reduced-motion.
- **Rascunho em streaming:** o texto vivo aparece como resposta normal e é
  trocado pela versão durável quando ela chega no log (mesmo texto). Sem piscar.
- **Mensagem enviada:** aparece na hora (otimista) e é trocada pela durável
  quando ela chega.
- **Cartão de permissão:** acima da caixa de texto, com a cor de "esperando
  você" dos tokens (`--state-waiting`, azul — o âmbar é só "trabalhando"). Título
  simples ("Quer executar um comando", "Quer editar um arquivo", "Quer criar um
  arquivo", "Quer acessar a web", "Quer usar {ferramenta}"), o detalhe (comando em
  mono / caminho relativo ao projeto) e os botões **Permitir**, **Permitir
  sempre nesta conversa**, **Negar**. Teclado: Enter = Permitir, Esc = Negar.
- **Cartão de pergunta:** a pergunta, as opções como botões (marcáveis se
  `multiSelect`), "Outra resposta…" com campo de texto, e **Responder**.
- **Respostas rápidas (Codex):** as `options` de `agentMessage.questions` viram
  botões pequenos abaixo da resposta. Um clique manda o rótulo como mensagem.
- **Caixa de texto:** cartão arredondado ao pé, textarea que cresce até ~40% da
  altura. Placeholder "Peça algo ao agente…". Enter envia, Shift+Enter quebra
  linha. Embaixo, à esquerda: o seletor de modelo/potência. À direita: Enviar
  (seta para cima). O microfone fica no cabeçalho da sessão, no mesmo lugar do
  terminal, e o texto ditado cai na caixa (decisão de construção: um botão só,
  sempre no mesmo lugar). Ocupado: Enviar vira
  **Parar** (quadrado) quando a caixa está vazia. Com texto, Enviar manda mesmo
  ocupado (Claude enfileira; Codex usa `turn/steer`).
- **Seletor de modelo e potência:** botão "Opus 5.5 · Alto" que abre o `Popover`
  da casa **para cima** (ele hoje só abre para baixo; ganha uma opção de
  posição): modelos (nome + descrição do CLI; o atual marcado) e, embaixo, as
  potências que **aquele** modelo aceita como segmentos (Baixa, Média, Alta,
  Muito alta, Máxima; valor desconhecido mostrado como veio). Trocar fecha o
  popover e grava `ChatSettings` no log.
- **Fim da sessão:** se o processo sair, um aviso discreto "Esta conversa
  terminou" no lugar da caixa. Para o Claude, um botão **Continuar** (D41).
- **Erro do provedor** (ex.: sem login): linha de erro do log, como hoje.

### 7.3 i18n e temas

Todo texto novo em `en` e `pt-BR` desde a primeira string. O pt-BR é tipado
contra o en. Âmbar = token de agente trabalhando. Escuro e claro conferidos por
pixel amostrado (HANDOFF §5 itens 42/64), não a olho.

## 8. Decisões que tomei (o Alan delegou)

1. **Nome do modo: "Chat"** (en e pt-BR). "Conversa" já é o nome da projeção
   ao lado do terminal; usar a mesma palavra confundiria as duas.
2. **A preferência fica em Aparência.**
3. **Sem `--model`/`--effort`/`--permission-mode` no lançamento:** o chat
   começa onde o CLI da pessoa já está.
4. **Markdown próprio, sem dependência nova:** um subconjunto pequeno,
   renderizado como elementos React, com testes. Segurança e peso decidem: sem
   `innerHTML` numa webview com IPC.
5. **"Sempre nesta conversa" só no escopo da sessão.** Nunca no repositório nem
   nas configurações da pessoa.
6. **Sem rodízio automático, sem split, sem Local, sem imagem neste corte**
   (§4).
7. **Chat só em pasta já confiada pelo provedor** (§6.4.1), com o caminho
   para confiar na tela dele. Seguir em silêncio rodaria hooks e MCP de um
   repositório desconhecido sem consentimento.
8. **A escolha de modelo/potência vale só para aquela conversa.** O próximo
   chat começa no padrão do CLI da pessoa, sempre visível na caixa de texto.
   Lembrar entre conversas fica para depois.
9. **O guardrail do Codex no chat é o mesmo do terminal**
   (`PreExecutionWhenTrusted`), e a cobertura continua declarada onde já é:
   Configurações → Guardrails ("Impedido depois que você confiar no hook no
   Codex"). O chat não repete o aviso.
10. **A conversa na tela vem do log** (`session_conversation`, poll de 500 ms
   no chat), e o vivo vem do canal. Mesma fonte do terminal: a conversa é igual
   viva, reaberta ou lida no Histórico.

## 9. Testes e verificação

Rust (com linhas reais da medição como fixture):
- drivers: cada tipo de linha → evento; codificação de cada comando; o
  `destination` reescrito para `session`; resposta de `AskUserQuestion` com
  `answers`; correlação de ids do Codex; `turn/steer` com turno ativo.
- `parse_line` não vira balão para `<command-name>`/`<local-command-stdout>`.
- `write` num chat é recusado; `live_sessions` do rodízio pula chat.
- capacidade `chat` distingue Claude, Codex e Local.
- migração: `mode` padrão `terminal`; `SessionInfo.mode` volta do banco.
- recusa `chat.folderNotTrusted` para pasta sem confiança, e nenhuma para
  confiança desconhecida;
- ainda não medido, medido nos testes reais abaixo: um `ask` do guardrail vira
  `can_use_tool` no `-p`; duas mensagens seguidas no Claude (fila) e
  `turn/steer` no Codex; `settings.json` da pessoa intacto depois de
  `set_model`;
- testes `#[ignore]` contra os CLIs reais: um turno de chat do Claude
  (permissão + pergunta + troca de modelo) e um do Codex (aprovação + troca de
  modelo), num repo scratch temporário.

TS: o renderizador de Markdown (blocos, inline, link seguro, nada de HTML cru)
e a humanização das ferramentas, rodados como os testes de i18n
(Node + TypeScript direto).

App real (`pnpm tauri dev --config '{"identifier":"dev.jarvis.desktop.chatqa"}'`,
nunca o `jarvis.db` real), num projeto scratch:
- abrir chat Claude e chat Codex; mandar mensagem; ver streaming; permissão
  (Permitir, Sempre, Negar); pergunta; trocar modelo e potência e ver o turno
  seguinte vir do outro modelo; Parar; ditado e Caderno caindo na caixa;
  notificação com a janela sem foco; Histórico e busca achando o chat;
  Continuar um chat Claude encerrado;
- escuro e claro, en e pt-BR, olhando screenshots e amostrando pixels.

## 10. O que fica para quem constrói

Nomes internos, a divisão de componentes React, o desenho exato dos ícones, o
espaçamento fino e o texto exato das strings (dentro do tom acima).
