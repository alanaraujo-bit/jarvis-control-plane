# M13 — Contas e cotas (§66)

**Estado: concluído em 2026-08-24. Funcional, testado e validado visualmente.**

Este documento é a fonte de verdade para quem continuar o M13. Foi escrito no
meio da implementação, depois de uma fase de investigação empírica contra as
ferramentas reais instaladas nesta máquina. **Tudo que está marcado como
*verificado* abaixo foi medido, não lembrado** — e a diferença importa, porque
metade das decisões de arquitetura aqui existe por causa de um fato que
contrariou a suposição óbvia.

Leia `docs/HANDOFF.md` seções 2, 3 e 5 antes de tocar em qualquer coisa. As
regras de lá valem aqui inteiras.

---

## 1. O que o Alan pediu, na íntegra

Ele tem **quatro contas Claude Pro**, cada uma com sua própria cota de cinco
horas. O pedido, palavra por palavra reduzida ao essencial:

1. Gerenciamento de contas para **Claude Code e Codex**.
2. Um **painel de cotas** — janelas de 5 horas, janela semanal, quanto falta,
   quando reseta.
3. **Alternância de conta** que *não interrompe o trabalho em andamento*: se a
   agente estiver trabalhando e a conta trocar, ela continua.
4. **Troca automática**: quando uma conta acaba — ou está perto de acabar — o
   produto muda para a próxima sozinho e continua o trabalho.
5. **Login persistente.** Uma vez logada, a conta fica salva. Nada de refazer
   login toda vez.
6. Padrão de acabamento Vale do Silício: bonito, animado, os dois temas, pt-BR e
   en, verificado *olhando a tela*, não só compilando.

Ele classificou isso como uma das coisas mais importantes do projeto.

---

## 2. Investigação empírica — o que é verdade nesta máquina

Isto é o núcleo do documento. **Não repita esta investigação; ela já custou uma
sessão.** E não substitua nenhum destes fatos por memória de modelo — vários
deles contrariam o que "todo mundo sabe".

### 2.1 Claude Code guarda o quê, e onde

Verificado em `~/.claude/` e `~/.claude.json` (Claude Code 2.1.240/2.1.241):

- `~/.claude/.credentials.json` → `claudeAiOauth: { accessToken, refreshToken,
  expiresAt, refreshTokenExpiresAt, scopes[], subscriptionType, rateLimitTier }`.
  **Nunca leia o conteúdo disto no produto.** `envscan::detect_auth` já
  estabeleceu a regra: presença sim, conteúdo nunca (§60/§61).
- `~/.claude.json` → `oauthAccount: { accountUuid, emailAddress,
  organizationUuid, organizationName, organizationType, displayName, fullName,
  organizationRole, billingType, hasExtraUsageEnabled, … }`, além do mapa
  `projects` que carrega `hasTrustDialogAccepted` (o que
  `providers::claude::folder_is_trusted` já lê).
- `~/.claude/projects/<cwd-codificado>/<session-id>.jsonl` → os transcripts.

### 2.2 Existe uma forma oficial de perguntar quem está logado

```
$ claude auth status --json
{
  "loggedIn": true,
  "authMethod": "claude.ai",
  "apiProvider": "firstParty",
  "email": "alanvitoraraujo2a@gmail.com",
  "orgId": "018647f1-a01d-4d8d-ab30-7c4d2d945ffe",
  "orgName": "alanvitoraraujo2a@gmail.com's Organization",
  "subscriptionType": "pro"
}
```

Subcomandos existentes: `claude auth login|logout|status`. `login` aceita
`--claudeai` (padrão), `--console`, `--email <email>` (pré-preenche o campo na
página) e `--sso`.

`accounts::parse_claude_identity` já converte essa saída. Ela é a **única** via
de identidade usada para Claude Code — nada de ler `.credentials.json`.

### 2.3 Claude Code NÃO tem medidor de cota ao vivo — isto é o achado central

Foram varridos **115 transcripts** desta máquina procurando qualquer campo de
limite. Existe exatamente **uma** forma de dado de cota, e ela só aparece no
turno que foi **recusado**:

```json
"quotaLimits": {
  "status": "rejected",
  "resetsAt": 1787556000,
  "unifiedRateLimitFallbackAvailable": false,
  "rateLimitType": "five_hour",
  "overageStatus": "rejected",
  "overageDisabledReason": "org_level_disabled",
  "upgradePaths": ["upgrade_plan"],
  "isUsingOverage": false
}
```

Na mesma linha, `message.content[0].text` traz a frase que o Claude Code mostrou
ao usuário — *"You've hit your monthly spend limit · … · your session limit
resets 5:20pm (America/Sao_Paulo)"*. `resetsAt` é **unix em segundos**, não
milissegundos.

Consequências, que definem todo o modelo de cota:

- **O momento em que a conta esgota e o momento exato em que volta são
  Oficiais.** O provedor disse, ao segundo.
- **Quanto a janela está cheia antes disso não é Oficial em lugar nenhum.** O
  melhor honesto é *Observado*: somar os tokens que o próprio provedor reportou
  por turno dentro da janela — números que `usage_samples` já guarda.
- **Uma porcentagem exige saber a franquia, que ninguém publica.** Ela é
  aprendida do histórico desta máquina: cada recusa diz "esta conta foi recusada
  depois de ~N tokens na janela". Até a primeira recusa, a porcentagem é
  **Unknown** e a tela precisa dizer isso em vez de desenhar uma barra confiante
  sobre um número que ninguém tem.

> **Não construa o painel sobre um endpoint HTTP lembrado.** Durante a
> investigação surgiu a tentação de chamar algo como
> `api.anthropic.com/api/oauth/usage`. Isso é recordação, não evidência, e é a
> fundação errada para a feature que o Alan chamou de mais importante. Se
> alguém quiser um medidor ao vivo depois, que entre como uma **terceira fonte
> aditiva atrás da sua própria verificação** — nunca como aquilo de que o painel
> depende.

### 2.4 Codex TEM medidor ao vivo, e o adaptador atual lê o campo errado

Verificado nos rollouts em `~/.codex/sessions/**.jsonl`. Todo evento
`token_count` carrega:

```json
"rate_limits": {
  "limit_id": "codex",
  "primary":   { "used_percent": 0, "window_minutes": 10080, "resets_at": 1788050255 },
  "secondary": null,
  "credits":   { "has_credits": false, "unlimited": false, "balance": "0" }
}
```

**Bug real, identificado e corrigido no adaptador:** `providers/codex.rs` lia
apenas `resets_in_seconds`. Esta build do Codex escreve `resets_at` (unix
**segundos**, absoluto). O adaptador e `accounts::quota::codex_observations`
agora leem as duas grafias e preservam as duas janelas.

Segundo ponto: o adaptador dobra `primary` e `secondary` numa só janela. Isso
joga fora a janela que não está limitando no momento — que é justamente a que
alguém planejando a semana quer ver. `codex_observations` devolve as duas.

Identidade do Codex: não existe `codex auth status`. Vem dos claims que ele já
guarda em `~/.codex/auth.json` (`tokens.id_token_claims.email` e
`…["https://api.openai.com/auth"].chatgpt_plan_type`). `parse_codex_identity`
faz isso e **não toca nos tokens ao lado**.

### 2.5 A decisão de arquitetura: diretório de configuração, não troca de credencial

Duas maneiras de trocar de conta:

| | Reescrever `~/.claude/.credentials.json` | Um `CLAUDE_CONFIG_DIR` por conta |
|---|---|---|
| Alan continua logado na sessão que está usando agora | **não** | sim |
| Sessão antiga segue na conta A enquanto a nova começa na conta B | **impossível** | sim |
| Produto manipula segredo do usuário | sim | **nunca** |

A segunda venceu, e o motivo não é elegância: **o Alan está logado nesse arquivo
agora mesmo, muito provavelmente numa sessão de Claude Code que ele usa para
construir este produto.** Uma troca que reescreve esse arquivo desloga ele da
coisa em frente à qual ele está sentado. E como só existe um arquivo global, o
requisito 3 do pedido dele — "ela continua trabalhando e já muda pra outra" — é
mecanicamente impossível nesse desenho.

Portanto: **uma conta é um diretório de configuração.**

- Claude Code → `CLAUDE_CONFIG_DIR`; Codex → `CODEX_HOME`.
- A conta já logada nesta máquina é **adotada**: a linha aponta para o
  `~/.claude` real, nada é copiado para dentro nem para fora, e remover a conta
  no J.A.R.V.I.S. nunca apaga o diretório.
- **A conta adotada roda com a variável ausente, não definida com o mesmo
  caminho.** Não é a mesma coisa: um `CLAUDE_CONFIG_DIR` explícito pode seguir
  outro caminho de código no provedor, e o padrão precisa continuar sendo o
  padrão. `accounts::session_env` devolve vazio para a conta adotada de
  propósito.
- Contas novas ganham `<data_dir>/accounts/<provider>/<id>/`, vazio, e a pessoa
  entra pelo fluxo de login do próprio provedor.

### 2.6 `CLAUDE_CONFIG_DIR` e `CODEX_HOME` — verificados contra os CLIs reais

Verificado em 2026-08-24 por
`pty::tests::provider_config_roots_isolate_state_and_transcripts`, um teste
`#[ignore]`d que inicia Claude Code 2.1.241 e Codex 0.149.1 em PTYs reais, num
repositório Git de scratch e com diretórios de configuração vazios. Nenhum
arquivo de credencial é lido ou copiado. Execute com
`cargo test provider_config_roots_isolate_state_and_transcripts -- --ignored`.

Resultado medido: **o desenho por diretório está confirmado nos dois
provedores**. `CLAUDE_CONFIG_DIR` leva consigo `.claude.json` e `projects/`;
`CODEX_HOME` leva consigo `sessions/`. O teste também prova que um `CODEX_HOME`
fornecido em `PtyOptions.env` sobrevive ao scrub de variáveis `CODEX_*`, porque
o ambiente específico da conta é aplicado depois dele.

Três perguntas, todas no mesmo teste:

1. **Com `CLAUDE_CONFIG_DIR=<dir>`, o `.claude.json` vai para
   `<dir>/.claude.json` ou continua em `$HOME/.claude.json`?**
   - **Vai junto.** Cada conta tem seu próprio `oauthAccount` (bom para
     identidade) **e seu próprio mapa `projects`** — ou seja, uma conta recém-
     adicionada tem **zero pastas confiadas**. Toda run Unattended na conta 2
     bate no item 25 do HANDOFF e é recusada com `autopilot.folderNotTrusted`.
     Isso é um precipício de UX no primeiro switch do Alan. **Mostre no painel**
     ("esta conta ainda não confia nesta pasta") e **não escreva confiança** —
     este código é explícito que isso é decisão da pessoa, na interface do
     próprio Claude Code.
2. **Os transcripts vão para `<config-dir>/projects/`?** **Sim**, e
   é por isso que `accounts::transcript_root` existe. Se a raiz do transcript
   não passar a ser por sessão — resolvida a partir da conta que iniciou a
   sessão — então **Conversation View, usage, evidência, Analytics e Global
   Search ficam silenciosamente vazios** para qualquer sessão numa conta
   alternativa. É exatamente a falha "não casa com nada e parece um estado
   vazio" contra a qual o HANDOFF avisa três vezes.
3. **`CODEX_HOME` se comporta igual?** **Sim.** E atenção:
   `pty::SCRUBBED_ENV_PREFIXES` inclui `"CODEX_"`. A ordem em `pty::spawn`
   salva (o scrub roda antes do `options.env`), mas isso é frágil e merece um
   teste que fixe o comportamento.

---

## 3. O que já está no disco

Tudo abaixo compila e a suíte está **verde: 431 testes Rust passando, 9
`#[ignore]`d**. O teste real de isolamento dos dois CLIs também passou quando
executado explicitamente com `--ignored`.

### 3.1 Migração 11 — aplicada e com fingerprint registrado

`db/migrations.rs`, `SCHEMA_VERSION = 11`, fingerprint
`0xd1c1_2851_e515_228c` em `SHIPPED`.

- `provider_accounts` — id, provider, label, config_dir, adopted, email, org_id,
  org_name, plan, signed_in, checked_at, active, paused, position, created_at,
  last_used_at. Índice único em `(provider, config_dir)`.
- `account_limit_events` — append-only, uma linha por coisa que o provedor
  *disse*: window, status (`ok|warning|rejected`), resets_at_ms, percent,
  detail. Nunca uma inferência nossa; estimativa vive no agregado.
- `sessions.account_id` e `usage_samples.account_id`. **NULL significa
  "gravado antes de contas existirem", que não é a mesma coisa que "a conta
  padrão"** e não pode ser dobrado nela — uma janela de cota calculada sobre
  linhas anteriores à feature atribuiria o gasto de outra pessoa a qualquer
  conta que estiver em primeiro na lista.

> ⚠️ **Aviso operacional, leia antes de rodar o app.** A migração 11 rodou no
> banco descartável do identificador `dev.jarvis.desktop.m13qa`; portanto está
> **congelada** e nunca mais deve ser editada. Qualquer mudança de schema é uma
> migração 12. Ela **não** foi aplicada ao `jarvis.db` do Alan: toda a QA visual
> usou o identificador separado, enquanto a cópia instalada continuou aberta.

### 3.2 `accounts/mod.rs` — modelo e registro

Escrito e comentado. `Account`, `machine_config_dir`, `config_env_key`,
`session_env`, `transcript_root`, `Identity`, `parse_claude_identity`,
`parse_codex_identity`, `read_identity`, `list`, `get`, `active`,
`adopt_machine_account`, `create`, `rename`, `set_paused`, `remove`,
`refresh_identity`, `stamp_used`.

Detalhe de i18n que já foi corrigido uma vez e não deve regredir: `label` vazio
significa "a pessoa não nomeou". O texto que ela lê antes de a conta ter
identidade é uma **string no idioma dela** (§65) e mora na superfície, nunca
numa coluna do banco que congelaria um idioma no registro para sempre.

### 3.3 `accounts/quota.rs` — o modelo de cota

Escrito e comentado. `LimitObservation`, `claude_observation`,
`codex_observations`, `record`, `observe_line` (com guarda por substring, para
não fazer um segundo parse de JSON em toda linha de toda sessão),
`QuotaWindow`, `AccountHealth`, `AccountQuota`, `window_start`, `calibration`,
`for_account`, `report`, `NEARING_PERCENT = 85.0`.

Duas escolhas que merecem sobreviver a uma revisão:

- **`window_start`**: uma franquia de cinco horas não é média móvel — ela começa
  na primeira requisição depois do último reset. Sempre que o provedor tiver
  informado um reset, ancoramos nele exatamente. Sem nenhum reset conhecido, a
  janela móvel é a aproximação honesta — e tudo calculado a partir dela é
  carimbado **Observed/Estimated**, nunca Official.
- **`calibration`** usa o **menor** total já visto numa recusa, não a média nem
  o maior. O número decide quando *sair* de uma conta: trocar um pouco cedo não
  custa nada, trocar tarde custa um turno recusado no meio de uma run.
- `tokens_between` **exclui leitura de cache** de propósito (é a metade barata
  do turno e domina tudo numa sessão longa; contá-la faria toda conta parecer
  perto do limite depois de vinte minutos) e **inclui escrita de cache**, que é
  cobrada como entrada.

### 3.4 `envscan::tool_command` / `envscan::run_tool`

Extraídos de `run_probe` e agora `pub(crate)`. Existem porque `claude` nesta
máquina é um shim `.cmd`: `CreateProcess` não aplica PATHEXT e um `.cmd` só roda
pelo interpretador. `run_tool` aplica **uma** variável de ambiente extra — é
assim que uma conta é interrogada no diretório dela sem tocar no da máquina — e
trata saída diferente de zero como `None`, porque um `status` que falhou não
disse nada, e tratar o texto de erro dele como resposta é como uma conta logada
vira "deslogada" na tela.

### 3.5 Integração e superfície concluídas

`accounts/commands.rs`, `accounts/switch.rs` e `accounts/tests.rs` existem e o
módulo está registrado em `lib.rs`. Sessões resolvem e gravam `account_id`,
recebem o config dir correto, atribuem transcripts/usage à conta que as iniciou
e mantêm o comportamento anterior quando ainda não existe registro.

A superfície `Accounts` está no rail com identidade, estado de verificação,
pausa/ativação, login do provedor, confiança por número, duas janelas, política
automática e aviso explícito quando o limiar usa estimativa. A conta da máquina
não oferece remoção; a última conta pronta não pode ser pausada.

O relé de autopilot inicia uma sessão nova na conta destino, reconstitui o
contexto pelo brief do Brain e `opening_instruction`, mantém orçamento/progresso
da run e deixa o processo antigo vivo. Não copia transcript e não usa
`--resume`. A confiança da pasta é checada na conta destino antes da troca.

---

## 4. O plano, na ordem em que deve ser executado

Cada passo é verificável sozinho. Não pule para a superfície bonita antes do
passo 2 estar provado — todo o resto se apoia nele.

**1. ✅ Provar `CLAUDE_CONFIG_DIR` e `CODEX_HOME`** (seção 2.6), num teste
`#[ignore]`d que roda o CLI real. Ajustar o desenho ao que o teste disser.

**2. ✅ Ligar o config dir na sessão.** `session::commands::launch` resolve
`accounts::active(&db, provider)`, grava `sessions.account_id`, chama
`accounts::stamp_used`, e passa `accounts::session_env(&account)` em
`PtyOptions.env`. `session::transcript::locate` e
`providers::claude::find_transcript` passam a receber a raiz vinda da conta em
vez de `home()/.claude/projects` fixo. **Sem conta registrada, tudo se comporta
exatamente como antes** — nada em iniciar um agente pode depender de contas
terem sido configuradas.

**3. ✅ Registro + painel de identidade honesto.** `adopt_machine_account` no
startup (idempotente). Superfície `Accounts` no rail: cartões por conta, com
e-mail, organização, plano, ativa/pausada, e um estado "não verificado" que é
diferente de "deslogada".

**4. ✅ Cota.** `observe_line` chamado do tailer de transcript com o `account_id`
da sessão. Corrigir `providers/codex.rs` (`resets_at`, e as duas janelas).
Barras com confiança visível: Official ≠ Observed ≠ Estimated ≠ Unknown, e uma
janela sem calibração mostra **tokens**, não uma barra vazia. Contagem
regressiva até o reset.

**5. ✅ Troca manual.** `set_active`. **Sessões em execução não são tocadas** — não
dá para reautenticar um processo já rodando, ele tem as credenciais carregadas —
e a tela precisa **dizer isso**, com a contagem de `live_sessions` que
`AccountQuota` já traz.

**6. ✅ Troca automática.** Preferência em `settings`: `off | onExhaustion |
onThreshold`, com o limiar em `NEARING_PERCENT`. Gatilho oficial (uma recusa
`quotaLimits`) é o piso que não pode falhar; o gatilho por limiar roda sobre a
estimativa **e a interface tem que dizer que é uma estimativa**.

**7. ✅ Continuidade da agente através da troca.** É aqui que mora a parte difícil,
e a decisão precisa ser tomada de propósito, não descoberta no meio:

- Um processo Claude Code em execução **não pode** trocar de conta. Continuidade
  significa necessariamente **uma sessão nova na conta nova**.
- **`--resume` não atravessa contas**: o transcript da conversa gravada sob a
  conta A está no config dir da conta A e é invisível para um `--resume` sob a
  conta B. Ou se copia arquivo interno do provedor entre diretórios (feio,
  frágil), ou se **restabelece o contexto pelo caminho que o produto já tem** —
  o brief do Brain (§38, D21/D23) mais a instrução de abertura da missão
  (`plan::opening_instruction`), com o estado da missão inteiro já persistido em
  banco. **Escolha o caminho do Brain, deliberadamente.**
- Forma sugerida: um *relé* de autopilot — `switch::relay_autopilot` para a run
  atual, inicia uma sessão de agente nova na conta nova via
  `start_agent_session`, e inicia um autopilot novo na mesma missão. A run velha
  para de dirigir; a sessão velha continua viva e o usuário pode assumir.
- **Antes de trocar, cheque `folder_is_trusted` para a conta de destino.** Senão
  a troca "sem costura" abre uma sessão que estaciona no diálogo de confiança
  com ninguém para responder — o item 25 do HANDOFF, chegando por uma rota nova.

**8. ✅ Paridade com o Codex.** `CODEX_HOME`, identidade por `auth.json`, e as
janelas que ele já reporta oficialmente.

**9. ✅ `account_switching: true`** nas capacidades — **só quando for verdade**
(§26). Está `true` nos dois adaptadores.

**10. ✅ Superfície, i18n e verificação visual.** pt-BR e en, tema claro e escuro,
animação nas barras e na contagem regressiva, e o loop do HANDOFF §3: rodar o
app instalado, tirar screenshot, **olhar**, corrigir o que estiver errado de
verdade. Lembre do item 42: não julgue cor por screenshot, amostre o pixel.

---

## 5. Armadilhas específicas desta feature

Além de tudo em `HANDOFF.md` §5, que continua valendo:

1. **`resetsAt` do Claude é unix em segundos.** Lê-lo como milissegundos põe o
   reset em 1970, faz uma conta esgotada parecer permanentemente recuperada, e
   desliga a feature inteira em silêncio.
2. **Uma recusa só vale até o reset dela.** Depois disso a conta é presumida
   recuperada — o provedor disse quando. Esperar uma segunda declaração deixaria
   a conta parada como esgotada para sempre, porque nada roda nela para produzir
   uma observação nova.
3. **`account_id` NULL não é a conta padrão.** Ver 3.1.
4. **Nunca escreva confiança de pasta** (`hasTrustDialogAccepted`). Ver 2.6.
5. **Nenhum segredo entra em struct que cruza para a webview.** `Account` tem
   caminho e identidade, e é tudo.
6. **`pty::SCRUBBED_ENV_PREFIXES` contém `"CODEX_"`.** O scrub roda antes do
   `options.env`, então definir `CODEX_HOME` explicitamente sobrevive — mas isso
   é ordem de código, não contrato. Fixe com um teste.
7. **Não teste agentes nos projetos reais do Alan.** Pasta de scratch da sessão,
   com `git init`. E o seletor de pastas abre onde foi na última vez — tire
   screenshot antes de digitar.
8. **Ele está usando a cópia instalada para trabalhar.** Para testar, abra outra
   — ele disse isso explicitamente. Ver o aviso de migração em 3.1 antes de
   apontar qualquer build para o `jarvis.db` dele.

---

## 6. Bloqueio conhecido

Adicionar as contas 2 a 4 exige um **login OAuth interativo no navegador**
(`claude auth login`, `--email` pré-preenche). Nenhum agente pode fazer isso
pelo Alan. `claude setup-token` **não** é atalho: o primeiro uso exige o mesmo
login interativo.

Isso **não bloqueia construir nem verificar a maquinaria** — ela pode ser
exercitada com a conta viva dele mais um config dir alternativo. Bloqueia apenas
a verificação ponta a ponta de uma troca genuína entre duas contas reais.
Registrado como **B6** em `docs/BLOCKERS.md`.

### 6.1 Evidência de conclusão que não depende do B6

- `cargo test --no-fail-fast -q`: 431 passaram, 0 falharam, 9 ignored.
- teste `#[ignore]` real: Claude Code 2.1.241 e Codex 0.149.1 isolaram estado e
  transcripts num repositório Git de scratch.
- `pnpm typecheck`: cinco projetos do workspace verdes.
- `pnpm tauri build --no-bundle`: executável release produzido.
- QA real em `dev.jarvis.desktop.m13qa`, sem tocar na cópia instalada: pt-BR e
  en, claro e escuro, Claude/Codex, formulário de adição, Unknown, Estimated e
  Official. Screenshots em `.tmp/m13-qa/`.
- pixels medidos, não inferidos da captura: fundo escuro `#141415`, fundo claro
  `#FFFFFF`; barra Estimated alterna `#262116`/`#5F4F2F`, barra Official é
  sólida `#D8D8DC`.

O ciclo visual encontrou e corrigiu dois defeitos que não apareciam na suíte:
`window_ready` retornava sucesso sem revelar a janela, e um reset histórico
aparecia como “resetting now” para sempre. A segunda captura confirmou ambos.

---

## 7. Referências rápidas

```bash
pnpm install
cd apps/desktop/src-tauri && cargo test          # 431 passando, 9 ignored
cargo test -- --ignored                          # inclui os que rodam CLI real
cd apps/desktop && pnpm tauri build --no-bundle  # a build que é o app de verdade (HANDOFF §5 item 31)
```

Arquivos que este trabalho toca: `db/migrations.rs`, `accounts/*`,
`envscan/mod.rs`, `session/commands.rs`, `session/transcript.rs`,
`providers/claude.rs`, `providers/codex.rs`, `providers/mod.rs`,
`autopilot/driver.rs`, `autopilot/commands.rs`, `lib.rs`, e uma superfície nova
em `apps/desktop/src/surfaces/accounts/` com suas strings em `packages/i18n`.
