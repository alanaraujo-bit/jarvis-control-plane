# M22 — Métricas que valem a tela (§52, §53)

**Estado: implementado em 2026-08-26/27. Suíte verde, verificado no app real
sobre uma cópia do banco do Alan.**

O Alan foi direto: a tela de métricas *"está uma bosta, tudo muito genérico"*.
Ele pediu filtro por dia, ofensivas, um mapa de calor no espírito do GitHub mas
melhor e na temática do projeto, e que fosse intuitiva, saudável e de leitura
fácil. Este documento é o registro do que foi medido, decidido e construído.

---

## 1. O achado que decidiu tudo

A primeira coisa medida não foi design, foi **quanto dado existe**. E o
resultado mudou o projeto inteiro:

| fonte | dias | turnos com uso |
|---|---|---|
| `usage_samples` no banco | **2** (25–26 ago) | 889 |
| transcrições do provedor no disco | **20** (4–26 ago) | **45.487** |

O J.A.R.V.I.S. só conseguia contar o que ele mesmo viu acontecer. Todo o
trabalho anterior à instalação estava no disco, em `~/.claude/projects/**.jsonl`,
invisível para a tela.

**Sobre dois dias, um calendário e uma ofensiva são decoração.** Sobre vinte
dias com lacunas reais — 19, 20 e 21 de agosto estão vazios no corpus — eles
*são* a tela. Então a primeira metade deste milestone não é a superfície, é
recuperar a história.

---

## 2. O backfill de uso (`analytics/backfill.rs`)

Mesma forma e mesmos motivos do backfill do Global Search (D30): a **migração 19
adiciona colunas**, e a caminhada sobre dados de disco de tamanho desconhecido
acontece depois do startup, numa thread própria, retomável.

### 2.1 Duas garantias independentes contra contar duas vezes

Isto escreve **números que uma pessoa lê**, então uma garantia só não basta:

1. **`origin_uuid` + índice único.** Toda linha de transcrição com uso carrega
   um `uuid` do provedor (verificado no corpus inteiro). `INSERT OR IGNORE`
   torna uma segunda caminhada um no-op.
2. **Transcrição de sessão que o J.A.R.V.I.S. rodou nunca é caminhada.** Esses
   turnos já estão na tabela pelo tailer ao vivo, com `origin_uuid` nulo que a
   garantia 1 não pegaria. Medido: **7 de 207** transcrições se sobrepõem. O
   nome do arquivo *é* o id da sessão (Claude Code roda com `--session-id`, M3),
   então a checagem é exata.

### 2.2 O que ele não faz

Não inventa linhas em `sessions` nem em `projects`. O corpus cobre **35 pastas**
contra 3 projetos registrados, e colocar pastas que a pessoa nunca abriu aqui na
lista de projetos dela seria pior que a lacuna que isso conserta. O *nome* do
projeto viaja na própria amostra (`project_label`), lido do `cwd` que o provedor
gravou, e a superfície lê `COALESCE(projects.name, project_label)`.

Também não carrega `account_id`. Há um teste que trava isso: história
recuperada do disco não foi gasta sob nenhuma conta que este produto conheça, e
se ela vazasse para `accounts::quota` vinte dias de trabalho alheio cairiam
dentro da janela de cinco horas de alguém.

### 2.3 Resultado real

```
files=199 skipped=7 rows=29251 in 14584ms   samples 889 -> 30140
distinct days = 20
second walk: files=0 rows=0
```

Idempotência **provada em dado real**, não só em teste unitário.

---

## 3. O bug de fuso que ninguém tinha visto

`by_day` agrupava em **UTC**. O Alan está em UTC−3, então tudo que ele fez
depois das 21h aparecia no *dia seguinte*. A varredura do corpus mostrou o
sintoma na cara: um balde `2026-08-27` com 60 turnos, que era a noite do dia 26.

Numa sparkline sem rótulo isso é invisível. **Num calendário com datas é
simplesmente errado**, e quem lê é a pessoa de quem era a noite. Todo o balde
de dia em `analytics` passou para hora local (`LOCAL_DAY`).

`accounts::quota::daily_tokens` continua em UTC **de propósito**: alimenta uma
sparkline de 14 barras sem datas, onde a convenção é invisível e a consistência
com as janelas de cota importa mais. A divergência está comentada nos dois
lados.

---

## 4. As decisões da superfície

### 4.1 A ofensiva, sem virar cobrança

O Alan pediu ofensiva **e** pediu que a tela fosse saudável. Isso puxa para
lados opostos, e §52 é explícito que métrica é informação, não gamificação. A
resolução:

- Ofensiva atual e recorde são **fatos, sem meta, sem aviso de "não perca sua
  sequência"**. A legenda embaixo de "dias trabalhados" diz *"dia de folga é dia
  de folga"*.
- **Um turno** basta para o dia contar (`ACTIVE_DAY_MIN_TURNS = 1`). Qualquer
  regra com número seria o produto decidindo quanto trabalho conta como
  trabalho. A magnitude não se perde — é o que a intensidade do calendário
  carrega.
- A ofensiva atual **aceita ontem como fim**. Sem isso ela apareceria quebrada
  toda madrugada até o primeiro turno do dia: o produto repreendendo a pessoa
  pelo crime de ser cedo.

### 4.2 O calendário é o herói *e* o controle

Clicar num dia escopa a tela inteira. O calendário **não** se escopa junto — ele
é a figura e o controle ao mesmo tempo, e um controle que se apaga ao ser usado
não deixa caminho de volta.

Um dia sem nada é `disabled`: filtrar para uma tela vazia é armadilha, não
recurso.

### 4.3 Ausência de história ≠ dia parado

Dias anteriores ao primeiro turno registrado são desenhados como **contorno
vazio**, distintos de um dia ocioso. Numa janela de 90 dias sobre 20 dias de
dado, a versão ingênua inventaria setenta dias de preguiça que nunca
aconteceram. O GitHub não sabe fazer essa distinção; aqui ela é obrigatória
(§28).

### 4.4 Âmbar, não verde

A referência foi o GitHub, mas o produto tem **um** acento. Uma segunda cor de
sinal numa tela só leria como status, não como quantidade. A rampa vai do
inset da própria página até âmbar cheio, em 4 níveis, com **escala de raiz
quadrada** — um dia de 15M achataria duas semanas de trabalho real na banda mais
pálida, e o corpus tem exatamente essa forma.

### 4.5 A alavancagem diz até onde ela enxerga

Tokens vão tão longe quanto as transcrições — 20 dias. Mas **atenção humana e
tempo de sessão só existem desde que o J.A.R.V.I.S. está rodando** — 2 dias.
Imprimir uma razão de 30 dias calculada sobre 2 dias de atenção seria o número
mais lisonjeiro da tela e o menos verdadeiro. Por isso `Leverage.observed_from`
e o rótulo *"Medido desde 25 de agosto"*.

---

## 5. O que o QA visual pegou

Duas coisas que nenhuma suíte veria:

1. **O calendário nasceu com 13px de célula** (o tamanho do GitHub) e numa
   janela de 30 dias isso é 5 colunas: cem pixels de calendário perdidos num
   card de mil. Lia como defeito de renderização. Corrigido em duas etapas — a
   primeira tentativa usou frações (`1fr`) e ficou pior: células de 46px
   sozinhas em colunas de 200px, e no modo 90 dias as células deixaram de ser
   quadradas. A versão final fixa o tamanho da célula (40px com espaço, 22px
   sem) e **põe os números ao lado do calendário**, então o card se adapta e o
   calendário mantém as proporções que o tornam legível. Acima de 7 semanas as
   células ganham o número do dia dentro — aí deixa de ser um mapa de calor com
   tooltip e vira um calendário que se lê.
2. **A faixa de meses virou uma barra de rolagem cinza.** Cada grid tinha seu
   próprio `overflow-x`, e a barra ficou entre o cabeçalho e a primeira linha,
   escondendo os nomes dos meses. Agora a faixa e a grade rolam juntas, de uma
   caixa só. E a legenda saiu de dentro dessa caixa, porque a largura dela
   estourava o container por um fio e desenhava uma barra sob um calendário que
   cabia perfeitamente.

---

## 6. Evidência

- `cargo test --no-fail-fast`: **605 passando, 0 falhando, 19 ignored**.
- `pnpm typecheck`: cinco projetos verdes.
- `pnpm run docs`: 94 páginas, `no problems`.
- Backfill contra o corpus real, em cópia: 199 arquivos, 29.251 linhas, 20 dias,
  segunda passada com 0 linhas.
- App real (identificador `dev.jarvis.desktop.m22qa`, cópia do banco): ofensiva
  de 5 dias, recorde de 15 (4→18 ago), 20 de 30 dias, 117,4M tokens,
  alavancagem 25,4×, projetos recuperados do disco (CALL, j.a.r.v.i.s, SLATE).
  Filtro por dia verificado clicando 23 de agosto: 117,4M → 15,2M, projetos e
  modelos re-escopados, ofensivas inalteradas, calendário intacto.
  Capturas em `.tmp/m22-qa/`.

---

## 7. O que ficou de fora, e por quê

- **Commits por dia.** Seria um sinal forte de "dias programando", mas nada no
  produto conta commits ainda e inventar isso rodando `git log` por projeto
  registrado só cobriria 3 das 35 pastas do corpus. Fica como candidato honesto
  para um próximo milestone.
- **Backfill do Codex.** As sessões dele em `~/.codex/sessions/**.jsonl` têm
  formato próprio e este backfill só lê Claude Code. Os 54 arquivos existem; o
  parser não.
- **Rótulos de projeto genéricos.** Como o rótulo vem do `cwd`, subpastas
  aparecem como se fossem projetos (`src`, `src-tauri`). É honesto — foi ali que
  o trabalho aconteceu — mas merece um agrupamento por repositório depois.
