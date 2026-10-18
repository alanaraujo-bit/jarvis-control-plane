# Rodízio de contas sem costura — desenho (M26)

**Estado:** proposta para aprovação · 2026-10-18
**Responsável pelo documento depois de aprovado:** `docs/M26-ACCOUNT-ROTATION.md`
(este arquivo vira a seção de desenho dele).

Leia antes: `docs/M13-ACCOUNTS.md` (uma conta **é** um diretório de configuração;
armadilhas §5), `docs/M16-QUOTA.md` (cota oficial ao vivo), `docs/M21-ACCOUNT-ISOLATION.md`
(contas gêmeas), `docs/M24-ACCOUNT-RESILIENCE.md` (diretórios privados).

---

## 1. Intenção

O Alan tem várias assinaturas Claude Pro (e Codex). Ele quer ligar quantas
quiser dentro do J.A.R.V.I.S., escolher uma para trabalhar, e **nunca mais
parar por falta de cota**: quando a conta em uso estiver acabando — ou acabar,
conforme ele escolher — o J.A.R.V.I.S. passa para outra conta **e o agente
continua a mesma conversa, na mesma sessão**, sem ele reabrir nada. Ele também
quer que as contas do J.A.R.V.I.S. sejam **independentes do resto da máquina**,
e ver ali dentro, ao vivo, tudo que hoje o faria abrir o site do provedor.

Palavras dele que definem o tom: modal "bonitinho, bem sucinto, minimalista";
o rodízio "tem que ser muito inteligente" — usar primeiro a conta cuja cota
**reseta mais cedo**, porque cota que reseta sem ser usada é cota perdida.

## 2. O que medi antes de desenhar (2026-10-18, esta máquina)

Claude Code 2.1.302, Codex 0.163.0, J.A.R.V.I.S. instalado 0.10.0.

1. **`--resume` atravessa contas.** Num repositório scratch, com ambiente
   limpo: a conta A (`1a@outlook`) ouviu uma palavra; o `<id>.jsonl` foi
   copiado para `projects/<cwd-codificado>/` do diretório da conta B
   (`2a@gmail`); `claude -p --resume <id>` sob B respondeu a palavra e **anexou
   no mesmo arquivo** (26 → 39 linhas). O M13 §4.7 e o comentário em
   `switch::relay_autopilot` ("`--resume` não atravessa contas") estavam
   errados: não atravessa *sozinho*, mas atravessa se o arquivo estiver lá.
2. O diretório de transcripts do Claude Code é fixo em
   `<CLAUDE_CONFIG_DIR>/projects` (sem variável que o mova).
3. `codex resume [SESSION_ID] [PROMPT]` existe no 0.163.0. **Não verificado**
   entre contas: só há uma conta Codex logada nesta máquina (a da máquina, no
   cofre do Windows) e a cópia do J.A.R.V.I.S. está **deslogada** — o M24
   copiou `~/.codex` mas o login não estava em arquivo.
4. **Isolamento hoje:** terminal logado em `1a@outlook`, pasta da conta adotada
   do J.A.R.V.I.S. continua `2a@gmail`. O sintoma relatado pelo Alan ("logo
   outra conta no terminal e o J.A.R.V.I.S. passa a usá-la") é o bug pré-0.9.0,
   quando a conta adotada rodava sem `CLAUDE_CONFIG_DIR`. Já corrigido pelo M24.
5. **O que o M24 deixou aberto:**
   - A conta adotada foi isolada **copiando** `.credentials.json` da máquina.
     O J.A.R.V.I.S. e o CLI de fora passaram a dividir **uma mesma linhagem de
     token OAuth**: uma renovação com rotação ou um logout/revogação de um lado
     pode derrubar o outro. É exatamente "bagunça nos dois sentidos". (Hipótese
     forte, não reproduzida — reproduzir exigiria deslogar a máquina do Alan.)
   - `envscan::tool_command` (sondas de cota, `auth status`, login) não limpa
     nada do ambiente herdado. O PTY já remove `CLAUDE_CODE_*` e `CODEX_*`
     (`pty/mod.rs:62`), mas deixa passar `ANTHROPIC_*`, `OPENAI_*` e
     `CLAUDE_CONFIG_DIR` — que passam por cima do diretório da conta se quem
     abriu o app os tiver.
6. **Por que o rodízio "não funciona bem":** a rotação só move a flag global
   `active` (o tailer chama `maybe_rotate_recorded` a cada linha,
   `session/transcript.rs:264`, e as sondas também). Só runs de autopilot seguem
   essa flag; sessões interativas nunca trocam. E a troca abre **outra** sessão
   do J.A.R.V.I.S. com uma conversa **nova**, reconstruída pelo brief — o fio da
   conversa quebra. As sondas oficiais ao vivo só rodam quando a UI as dispara
   (status bar a cada 5 min); sem elas o gatilho por limiar só enxerga o degrau
   estimado.

## 3. Objetivos

- **G1 Isolamento total.** Nenhum login, logout ou troca de conta feito fora do
  J.A.R.V.I.S. altera a conta que o J.A.R.V.I.S. usa, e vice-versa.
- **G2 Troca na mesma sessão.** A sessão do J.A.R.V.I.S. (id, log, terminal,
  Conversation View) continua; só o processo do provedor é trocado, e ele
  retoma **a mesma conversa** na conta nova.
- **G3 Gatilho escolhido pelo usuário** em Configurações: *perto do limite*
  (percentual ajustável) ou *quando acabar*; e modo *automático* (avisa) ou
  *perguntar antes* (modal pede permissão). Troca manual também existe.
- **G4 Rodízio inteligente** ("use ou perca", §6).
- **G5 Sem conta nenhuma com cota:** balão com o horário da próxima volta e
  permissão para retomar sozinho quando voltar.
- **G6 Números ao vivo dentro do app**, compactos e detalhados, com confiança.

## 4. Não-objetivos

- Trocar a credencial de um processo em execução (impossível; troca = novo
  processo retomando a conversa).
- Escrever confiança de pasta (`hasTrustDialogAccepted`) — continua decisão da
  pessoa (M13 §5.4).
- Consumir o crédito de reset do Codex automaticamente (M16 §1.4.8).
- Forçar o seletor de conta do navegador (impossível, M21 §4).
- Processo em segundo plano com o app fechado. A espera do G5 vive com o app.

## 5. Antiobjetivos (falha mesmo que "funcione")

- A troca corta o agente no meio de uma edição ou comando.
- Duas contas na mesma assinatura entram no rodízio como se fossem duas cotas.
- Um número estimado aparece como oficial.
- Uma ação do J.A.R.V.I.S. desloga a conta da máquina (ou o contrário).
- Pingue-pongue entre contas.
- Qualquer coisa disto passa a exigir que contas estejam configuradas para
  iniciar um agente (sem conta registrada, tudo roda como antes).

---

## 6. O rodízio

### 6.1 Entradas

Por conta, da leitura oficial ao vivo (M16) quando existe, senão do degrau
Observed/Estimated que já existe em `quota::for_account`:

- janela de sessão (5h): `% usado`, `resets_at` (ou *não iniciada*);
- janela semanal: `% usado`, `resets_at`;
- estado: logada, pausada, gêmea (`same_subscription`), pasta confiada.

### 6.2 Quem pode receber a troca

Mesmo provedor; não é a conta atual; **não é a mesma assinatura**; logada;
não pausada; login próprio (§8.1); pasta do projeto confiada nessa conta
(Claude Code) — senão é pulada e o cartão dela diz por quê; e folga:

- gatilho *perto do limite*: toda janela abaixo de `limiar − 10` pontos;
- gatilho *quando acabar*: nenhuma janela esgotada e toda janela abaixo de 90%.

A folga existe para não trocar de novo minutos depois. Não há exceção para a
semanal: uma conta que não passa nesse filtro não é destino.

### 6.3 Ordem ("use ou perca")

1. **Cota que vai expirar primeiro.** Contas cuja janela de 5h **já está
   correndo**, ordenadas pelo `resets_at` mais cedo. É sobra que se perde se
   ninguém usar.
2. **Reservas.** Contas sem janela de 5h aberta (usar começa o relógio delas),
   depois das do item 1.
3. **Desempate semanal.** Em empate (resets a menos de 15 min um do outro), ganha
   a de **maior urgência semanal** = `% semanal livre ÷ horas até o reset semanal`
   (sobra semanal que vai se perder antes).
4. **Desconhecido por último.** Conta sem número nenhum (sonda falhou) só entra
   se nenhuma conhecida servir, e a ordem entre elas é a `position` atual.
5. **Histerese.** Depois de trocar, a nova conta só sai quando ela mesma atingir
   o gatilho. Não existe "voltar para a melhor" no meio do trabalho.

Função pura, testável: `rotation::rank(now, current, candidates) -> Vec<Ranked>`,
cada item com o **motivo** (para o modal dizer "reseta às 15:40").

### 6.4 Gatilhos

| Configuração | Dispara quando | Momento da troca |
|---|---|---|
| Perto do limite (padrão 90%, ajustável 50–98) | qualquer janela da conta da sessão ≥ limiar | **no fim do turno** — espera o agente terminar a resposta |
| Quando acabar | o provedor recusa um turno (recusa no transcript ou sonda `exhausted`) | **imediato** — o turno já parou |

Um gatilho oficial ou estimado vale; o modal diz qual. Uma recusa oficial vale
nos dois modos (piso que não pode falhar, como hoje).

### 6.4.1 Um motor só, por conta

- O motor novo (`accounts::rotation`) **substitui** `maybe_rotate` /
  `maybe_rotate_recorded` / `relay_needed` / `relay_autopilot`. O tailer e as
  sondas só alimentam dados; quem decide é o motor.
- A decisão é **por conta-janela**: quando a conta A dispara, todas as sessões
  vivas em A vão para o mesmo destino (topo do §6.3), cada uma no seu ponto
  seguro. Um modal/toast por evento, listando as sessões afetadas. "Agora não"
  vale para a conta-janela, não por sessão.
- `active` (a conta em que **novas** sessões começam) continua sendo a escolha
  do Alan. Ela só se move quando a conta ativa dispara o gatilho e houve troca —
  então passa a ser o destino, para que trabalho novo não comece numa conta no
  limite. Troca manual de uma sessão não mexe em `active`.

### 6.5 Automático × perguntar antes

- **Automático:** troca e mostra um aviso compacto (toast) — "Continuando na
  conta X · Y chegou a 90% · reseta 15:40". Some sozinho; fica no Activity.
- **Perguntar antes:** modal pequeno — "Conta Y em 90%. Continuar na conta X
  (reseta 15:40)?" — `Trocar` / `Agora não`. "Agora não" vale até o próximo
  gatilho da mesma janela (não repergunta a cada turno). Se ninguém responder e
  a conta acabar, a sessão para como pararia sem o J.A.R.V.I.S.
  Com autopilot dirigindo, o driver **segura o próximo turno** enquanto o modal
  está aberto; orçamento e contador de estagnação ficam congelados durante a
  espera (e durante a espera do §6.6).
- **Desligado:** nada acontece sozinho.
- **Manual:** no cartão da sessão e na tela de Contas, "Trocar conta desta
  sessão" (lista ranqueada pelo §6.3) — mesma maquinaria do §7.

Configuração: o par *modo* (`off | ask | auto`) + *gatilho*
(`nearLimit(percent) | exhausted`) substitui `accounts.autoSwitch`
(`off|onExhaustion|onThreshold`), migrado sem perda: `onExhaustion → auto+exhausted`,
`onThreshold → auto+nearLimit(85)`. Mora em **Configurações → Contas**
(um cômodo novo em `settings/categories.ts`), com atalho a partir da tela de Contas.

### 6.6 Nenhuma conta com cota

Balão: "Sem cota em nenhuma conta · a próxima volta às 17:20 (conta X)" com
`Retomar automaticamente` / `Não`. Aceito: a sessão fica *em espera* (estado
visível no cartão e no rail), um timer no core acorda no reset (+60 s de margem),
relê a cota oficial, e faz a troca do §7 com a mensagem de continuação. A
permissão vale para essa espera só. Fechar o app cancela.

---

## 7. A troca na mesma sessão (o núcleo)

### 7.1 Forma

Uma sessão do J.A.R.V.I.S. passa a poder **substituir o processo do provedor**
mantendo id, log, terminal anexado e tailer. `LiveSession.pty` deixa de ser fixo
(hoje é um `Arc<PtyHandle>` imutável e o pump encerra o writer do log no
`Exited`).

Sequência (`accounts::rotation::relay_session`):

1. **Ponto seguro.** Gatilho *perto do limite*: aguarda o fim do turno (evento
   de fim de turno no transcript + sessão `Idle`). Gatilho *acabou*: imediato.
2. Grava no log um evento `Lifecycle::AccountSwitch { from, to, reason }` —
   o log continua sendo a única história da sessão (HANDOFF §1).
3. Encerra o processo atual **sem** que o pump trate isso como fim da sessão
   (flag de substituição; o writer do log não recebe `Stop`).
4. **Leva a conversa** para a conta destino:
   - Claude Code: copia `<A>/projects/<cwd>/<id>.jsonl` (e o diretório
     `<id>/` de subagentes, se existir) para `<B>/projects/<cwd>/`. Cópia, não
     move: a de A fica como estava (útil se a troca falhar).
   - Codex: copia o rollout `sessions/AAAA/MM/DD/rollout-*-<id>.jsonl` para o
     mesmo caminho relativo em B. **Só depois de verificado** (§10); até lá,
     Codex segue o §7.4.
5. Atualiza `sessions.account_id` para B e `stamp_used(B)`. O tailer passa a
   ler o arquivo em B a partir do **mesmo offset** (o prefixo é idêntico), então
   nada é contado duas vezes; `usage_samples` daí em diante vão para B.
   **Exige reestruturar o tailer:** hoje `transcript::spawn` recebe
   `account_id` e `transcript_root` uma vez e localiza o arquivo uma vez. Ele
   passa a ter um comando de "retarget" (novo caminho + conta, mesmo offset).
   A substituição também não pode mandar `Beat::Exited` ao vigia de
   notificações (`manager.rs:432`) — senão a troca vira "sessão terminou".
6. Inicia o processo em B com `session_env(B)` e:
   - Claude: `claude --resume <id> [--settings …] [--append-system-prompt-file …]`
     — **sem** `--fork-session` e sem `--session-id`: o mesmo arquivo continua
     (medido no §2.1), a correlação continua determinística.
   - Codex: `codex resume <id>`.
7. Se o gatilho foi *acabou* (o turno foi interrompido) ou é a retomada do §6.6,
   envia ao agente a instrução de continuação — `"Continue exactly where you
   left off; the previous turn was interrupted by a usage limit."` — pelo mesmo
   caminho de digitação que o autopilot já usa. No *perto do limite*, nada é
   enviado: o turno tinha terminado, a próxima mensagem é da pessoa/autopilot.
8. Autopilot: a run **continua a mesma** (mesmo `session_id`); não existe mais
   relay que cria outra sessão para o caso Claude. O orçamento e o progresso não
   mudam.

### 7.2 Falhas

- Destino recusa logo de cara (logado errado, pasta não confiada, CLI ausente):
  tenta o próximo da lista do §6.3; esgotada a lista, volta à conta original se
  ela ainda tiver cota, senão §6.6. Tudo registrado no Activity com o motivo.
- A cópia falha: não mata nada antes de copiar (passo 4 vem depois do 3 só
  porque o arquivo precisa estar fechado; se o kill acontecer e a cópia falhar,
  reinicia em A com `--resume` — nada se perde).

### 7.3 O que a pessoa vê

O terminal mostra o processo novo retomando (o próprio CLI redesenha a conversa
ao fazer `--resume`); a Conversation View não pisca, porque é o mesmo
transcript; o cartão da sessão mostra a conta nova.

### 7.4 Codex

O Codex recebe a mesma maquinaria (`codex resume <id>` + cópia do rollout). A
retomada entre `CODEX_HOME`s é medida durante a implementação, assim que
existir uma conta Codex do J.A.R.V.I.S. logada (hoje está deslogada — vou pedir
ao Alan um login na hora). Se a medição mostrar que `codex resume` não acha ou
não continua o rollout copiado, a troca automática do Codex **fica desligada**
(o cartão diz "troca automática indisponível para Codex nesta versão" e a troca
manual abre conversa nova, dito explicitamente), em vez de fingir continuidade.
Não há caminho de reserva por brief para sessões sem missão.

---

## 8. Isolamento (G1)

1. **Nenhum token clonado.** Uma conta do J.A.R.V.I.S. só fica *pronta* depois
   de um login feito para ela, pelo fluxo de login dentro do app (link + código,
   M21). A conta adotada pelo M24 é marcada **"login compartilhado com a
   máquina"** (nova coluna `credential_origin = copied|own`): continua
   funcionando, mas o cartão pede "Entrar nesta conta pelo J.A.R.V.I.S." e ela
   **não entra no rodízio** até lá (um token compartilhado pode cair a qualquer
   momento por ação de fora — exatamente o antiobjetivo). Nada é apagado; nada
   na máquina é tocado. A importação inicial do M24 deixa de copiar
   `.credentials.json` / `auth.json` para instalações novas: a conta adotada
   nasce deslogada com o e-mail da máquina sugerido.
   *Minha decisão:* não bloqueia trabalho — sem outra conta, a adotada continua
   sendo usada normalmente; só fica fora do rodízio.
2. **Ambiente limpo em todo processo de provedor que roda numa conta
   registrada** — PTY e `tool_command`. Sem conta registrada, nada é removido
   (quem usa chave de API, proxy ou Bedrock sem contas continua funcionando, §5):
   remover `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_BASE_URL`,
   `CLAUDE_CODE_OAUTH_TOKEN`, `CLAUDE_CONFIG_DIR`, `OPENAI_API_KEY`,
   `OPENAI_BASE_URL`, `CODEX_*` herdados, e só então aplicar `session_env`.
   Um único ponto (`accounts::provider_env`) usado pelas duas rotas.
3. **Codex sempre em arquivo** (`cli_auth_credentials_store = "file"`, M24) —
   mantido.
4. **Teste de propriedade com CLI real** (`#[ignore]`, scratch): com a conta do
   J.A.R.V.I.S. apontada para o diretório dela, mudar `CLAUDE_CONFIG_DIR`
   ambiente / variáveis acima no processo pai não muda o `auth status` da sessão.

---

## 9. Números ao vivo (G6)

Fonte: leitura oficial (M16) com confiança; tokens e duração do log da sessão.
Coleta passa a ser **do core**, não da UI: um agendador em Rust sonda a conta de
cada sessão viva a cada **60 s** e as demais a cada **5 min** (hoje é a UI que
dispara a cada 5 min — o rodízio não pode depender de uma tela aberta). Sondas
não gastam token (M16 §1).

- **Compacto — cartão da sessão** (cabeçalho do terminal/conversa) e o chip do
  status bar que já existe: conta, `5h 62% · reseta 15:40`, `sem 41%`, e o ETA.
- **Detalhado — popover ao clicar** e a tela de Contas: as duas janelas com
  barra, % usado/livre, reset com contagem regressiva, **"no ritmo atual acaba
  ~14:55 (em 38 min)"**, tokens da sessão (entrada/saída/cache) e da janela,
  duração da sessão, conta em que cada trecho rodou (do evento
  `AccountSwitch`), e a fila do rodízio ("próxima: X, reseta 15:40").
- **ETA = Estimated**, sempre rotulado: inclinação do `% usado` nas leituras
  oficiais dos últimos 30 min da janela (já ficam em `account_limit_events`);
  sem duas leituras com variação, mostra "ritmo desconhecido", nunca um número.
- Tudo carrega Official/Observed/Estimated/Unknown como hoje.

---

## 10. Verificação (definição de pronto)

1. Unit: `rotation::rank` (ordem use-ou-perca, gêmeas, histerese, desconhecidos,
   empate semanal), migração da política, ETA, `provider_env`.
2. Integração com PTY real e CLI falso: substituição de processo mantém id, log
   contínuo com `AccountSwitch`, tailer sem dupla contagem.
3. CLI real, repositório scratch, nunca projeto do Alan:
   - Claude: troca A→B **dentro de uma sessão interativa do J.A.R.V.I.S.**, e o
     agente lembra o que foi dito antes (estende o experimento do §2.1).
   - Codex: `codex resume` entre dois `CODEX_HOME` — **bloqueado** até existir
     uma conta Codex do J.A.R.V.I.S. logada; resultado decide o §7.4.
   - Isolamento: o teste do §8.4.
4. App rodando (`pnpm tauri dev`, identificador de QA, cópia do banco): toast,
   modal, balão de espera, cartão compacto e popover, Configurações → Contas,
   **nos dois temas**, pt-BR e en.
5. `pnpm verify` verde; `docs/M26-ACCOUNT-ROTATION.md`, M13 §4.7 (corrigir a
   afirmação sobre `--resume`), HANDOFF e DECISIONS (nova D51) atualizados.

## 11. Decisões que tomei (para conferir)

- Limiar padrão 90%, folga 10 pontos, janela de ritmo 30 min, sonda 60 s/5 min.
- "Perto do limite" troca no fim do turno; "acabou" troca imediato e manda
  "continue".
- Cópia do transcript por troca (em vez de pasta `projects/` compartilhada por
  junção): explícito, sem truque de sistema de arquivos, sem migrar pastas.
- Conta com token copiado fica fora do rodízio até ganhar login próprio, mas
  continua utilizável.
- "Agora não" no modal vale até o próximo gatilho da mesma janela.
- Codex: mesma maquinaria; se a retomada entre contas não for confirmada,
  troca automática do Codex fica desligada (sem fingir continuidade).
- Um motor por conta-janela; `active` só se move quando a conta ativa dispara.
- Limpeza de ambiente só para sessões em conta registrada.
- Autopilot segura o próximo turno durante modal/espera; orçamento congelado.
- Espera do §6.6 morre com o app.

## 12. Deixado ao construtor

Layout exato do cartão compacto/popover dentro dos tokens de `docs/design.md`;
nomes de chaves i18n; forma do evento de fim de turno usado como ponto seguro
(reusar o que o autopilot/notify já detecta); como o pump sinaliza substituição.
