# M16 — Cotas ao vivo, oficiais e legíveis (§66 continuado)

**Estado: em execução.** Continuação direta do M13, aberta porque o Alan olhou a
tela pronta e disse a coisa que importa: *"ainda não estou achando usável e
funcional, ainda não consigo ver quando reseta e nem qual percentual ainda
tenho disponível, e nem qual das cotas está travada esperando o reset."*

Ele está certo, e o M13 explica por quê: ele foi construído sobre a premissa de
que **não existe medidor ao vivo para o Claude Code**. Essa premissa era
verdadeira para as fontes que o M13 investigou (transcripts). Ela é **falsa**
para uma fonte que o M13 não testou.

---

## 1. A descoberta que reabre a feature

Os dois provedores expõem cota ao vivo, oficial, sob demanda, **sem gastar
token, sem tocar em credencial e respeitando o diretório de configuração da
conta**. Verificado nesta máquina em 2026-08-24 contra os binários reais.

### 1.1 Claude Code 2.1.241 — `get_usage` no protocolo de controle

```
claude -p --safe-mode --input-format stream-json --output-format stream-json --verbose
stdin: {"type":"control_request","request_id":"u1","request":{"subtype":"get_usage"}}
```

Resposta medida (recortada):

```json
{"type":"control_response","response":{"subtype":"success","request_id":"u1","response":{
  "session":{"total_cost_usd":0},
  "subscription_type":"pro",
  "rate_limits_available":true,
  "rate_limits":{
    "five_hour":{"utilization":5,"resets_at":"2026-08-24T23:20:00.094226+00:00"},
    "seven_day":{"utilization":99,"resets_at":"2026-08-26T04:00:00.094247+00:00"},
    "extra_usage":{"is_enabled":false,"monthly_limit":0,"used_credits":0,
                   "currency":"BRL","disabled_reason":"org_level_disabled_until",
                   "spend_limit_reached":false,"credits_ever_enabled":true},
    "limits":[
      {"kind":"session","group":"session","percent":5,"severity":"normal",
       "resets_at":"2026-08-24T23:20:00+00:00","scope":null,"is_active":false},
      {"kind":"weekly_all","group":"weekly","percent":99,"severity":"critical",
       "resets_at":"2026-08-26T04:00:00+00:00","scope":null,"is_active":true}]}}}}
```

O próprio binário descreve o pedido: *"Requests the structured /usage data:
session cost/usage totals plus claude.ai plan rate-limit utilization when
available. **Experimental — the response shape may change.**"* Essa frase é a
razão de todo o parse ser tolerante (§1.4).

### 1.2 Codex 0.149.1 — `account/rateLimits/read` no app-server

```
codex app-server                       # JSON-RPC em stdio
-> {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{...}}}
<- {"id":1,"result":{"codexHome":"C:\\Users\\Alan Araujo\\.codex"}}
-> {"jsonrpc":"2.0","method":"initialized","params":null}
-> {"jsonrpc":"2.0","id":2,"method":"account/rateLimits/read","params":null}
<- {"id":2,"result":{"rateLimits":{"limitId":"codex","planType":"plus",
      "primary":{"usedPercent":80,"windowDurationMins":10080,"resetsAt":1788147529},
      "secondary":null,"credits":{"hasCredits":false,"balance":"0"},
      "spendControlReached":false,"rateLimitReachedType":null},
    "rateLimitResetCredits":{"availableCount":1,"credits":[{"title":"Full reset"}]}}}
```

### 1.3 O teste que decidiu se a feature vale para todas as contas

Rodar as duas sondas com um diretório de configuração **vazio**:

| Sonda | Resultado medido |
|---|---|
| `CLAUDE_CONFIG_DIR=<vazio>` | `subscription_type:null`, `rate_limits_available:false`, `rate_limits:null` |
| `CODEX_HOME=<vazio>` | erro JSON-RPC `-32600 "codex account authentication required to read rate limits"` |

**As duas sondas leem a conta do diretório, não a conta ambiente.** É isso que
torna o pedido inteiro entregável: as quatro contas do Alan podem mostrar
número oficial ao vivo, cada uma na sua, sem ele abrir o Claude Code web.

E as duas **falham dizendo que falharam** em vez de devolver número plausível de
outra conta — que era o risco real.

### 1.4 Armadilhas confirmadas nos dados (não inventadas)

1. **`session.total_cost_usd` é da sessão da própria sonda — sempre ~0.**
   Ligar isso em "gasto" desenha um zero confiante. Gasto continua vindo de
   `usage_samples`.
2. **Ler `limits[]`, não as chaves nomeadas.** `cinder_cove`, `nimbus_quill`,
   `tangelo`, `omelette_promotional` são codinomes rotativos; `limits[]` tem
   forma uniforme (`kind/group/percent/severity/resets_at/scope/is_active`).
   Um `kind` desconhecido renderiza genericamente — nunca some, nunca quebra.
3. **`is_active` + `severity` é a resposta à pergunta mais difícil dele** —
   "qual das cotas está travada". Nesta máquina agora: sessão 5% `normal`
   `is_active:false`; semanal 99% `critical` `is_active:true`.
4. **`extra_usage` é o "mensal" dele.** `monthly_limit`, `used_credits`,
   `currency:"BRL"`, `disabled_reason`. A frase do M13 §2.3 era literalmente
   *"monthly spend limit"*.
5. **Percentual é *usado*; ele pediu *disponível*.** Uma direção só, mantida na
   barra, no rótulo e na contagem.
6. **Três formatos de reset agora:** ISO 8601 (Claude ao vivo), unix segundos
   (transcript do Claude e Codex), ms interno. Uma fronteira de parse, um teste
   por formato.
7. **`--safe-mode` carrega peso.** Sem ele a sonda dispara os hooks
   `SessionStart` do Alan (um deles erra). Com ele a sonda é limpa. A sonda
   também **não escreve transcript** — verificado: o diretório fresco ficou com
   `.claude.json` e `sessions/<pid>.json`, e nenhum `projects/`. Session History
   e o índice FTS do §51 não são poluídos.
8. **Nunca consumir o crédito de reset do Codex automaticamente.**
   `rateLimitResetCredits.availableCount:1` merece ser *mostrado*;
   `account/rateLimitResetCredit/consume` é irreversível e exige clique humano.
9. **A sonda do Claude não devolve identidade** — só `subscription_type`. Uma
   sonda apontada para o diretório errado devolve número plausível atribuído à
   conta errada. Por isso a leitura é sempre emparelhada com
   `accounts::read_identity` pelo mesmo caminho de aplicação de env, e o e-mail
   é guardado junto da leitura. O Codex resolve sozinho: o `initialize` ecoa
   `codexHome`, e a sonda **afirma** que o eco bate com o diretório da conta.

### 1.5 Onde isso deixa o ORCA

O ORCA (instalado nesta máquina, e a régua que o Alan citou) colhe os mesmos
números **passivamente**, por um statusline hook —
`out/shared/claude-statusline-rate-limits.js` peneira o payload atrás de
`"rate_limits"`. Isso exige injetar um statusline na configuração do usuário e
só rende número enquanto uma sessão roda. A sonda sob demanda é estritamente
melhor: não altera configuração nenhuma e responde com zero sessões abertas.

O que vale copiar dele é o acabamento da contagem
(`out/shared/rate-limit-reset-format.js`): unidades inteiras (`47m`, `3h 54m`,
`6d 7h`) e um timer que acorda **na virada da unidade**, não a cada segundo.

---

## 2. O que muda no produto

1. **Fonte oficial ao vivo** por conta, para os dois provedores, atrás de
   verificação própria — a "terceira fonte aditiva" que o M13 §2.3 autorizou.
   O degrau Observed/Estimated continua existindo e vira o *fallback*.
2. **Calibração contínua.** Percentual oficial + soma de tokens da janela dá
   franquia implícita (`tokens / (percent/100)`). Isso afia o degrau Estimated
   continuamente, em vez de só numa recusa.
3. **A tela responde as três perguntas que ele fez**, em uma olhada: quanto
   sobra, quando reseta, e **qual janela está travando**.
4. **Migração 12** para o que a leitura ao vivo precisa guardar. A 11 está
   congelada (M13 §3.1) e não se toca.

## 3. Registro de progresso

Atualizado a cada etapa concluída e verificada.

- [x] Investigação empírica das duas sondas (§1), incluindo o teste do
      diretório vazio que decidiu o alcance da feature.
