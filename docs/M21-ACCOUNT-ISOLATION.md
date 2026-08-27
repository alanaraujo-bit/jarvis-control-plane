# M21 — Contas realmente diferentes (§66)

**Estado: implementado em 2026-08-26. Suíte verde, verificado contra o banco real
desta máquina.**

Este documento existe porque o Alan relatou o sintoma mais caro que esta área
pode ter: *"tenho duas contas, as estatísticas são iguais, e eu nunca consigo
usar a outra."* A investigação encontrou **duas** falhas encadeadas, e a segunda
é a que escondeu a primeira.

---

## 1. O que estava acontecendo, medido

### 1.1 A causa raiz: o login reaproveita a sessão do navegador

Medido nesta máquina, num `CLAUDE_CONFIG_DIR` descartável e vazio:

```
$ CLAUDE_CONFIG_DIR=<dir novo e vazio> claude auth login --claudeai
Opening browser to sign in…
If the browser didn't open, visit: https://claude.com/cai/oauth/authorize?code=true&client_id=…
Paste code here if prompted > Login successful.
```

Levou cerca de **um segundo** e **não perguntou nada**. O diretório novo ficou
logado em `bddb9ea1-8777-4499-b8c2-3fb4c5429acd` /
`alanvitoraraujo2a@gmail.com` — a conta que o navegador já tinha aberta.

`claude auth login` tem `--claudeai`, `--console`, `--email <email>` e `--sso`.
**Nenhuma delas força o seletor de conta.** `--email` só pré-preenche o campo na
página, e a página nem chega a aparecer quando já existe sessão.

Consequência: *o jeito óbvio de adicionar uma segunda conta adiciona um segundo
diretório na primeira conta.* Foi exatamente isso que aconteceu com o cartão
"CLAUDE 2".

Confirmado nos dois diretórios reais — mesmo `accountUuid`, lido direto do
disco:

| diretório | `oauthAccount.accountUuid` | e-mail |
|---|---|---|
| `~/.claude` (adotada) | `bddb9ea1-…` | `alanvitoraraujo2a@gmail.com` |
| `…/accounts/claude-code/01a03e64-…` | `bddb9ea1-…` | `alanvitoraraujo2a@gmail.com` |

> **Nota de método.** `claude auth status --json` responder a mesma conta nos
> dois diretórios **não prova nada** — uma CLI que ignorasse a variável de
> ambiente produziria a mesma saída. A prova é a leitura dos dois `.claude.json`
> no disco, que são arquivos diferentes com o mesmo `accountUuid` dentro.

### 1.2 O que escondeu: a identidade guardada estava velha

O produto **já tinha** a checagem de gêmeas desde o M13
(`accounts::same_subscription`, `format.sharedSubscriptions`, o aviso no
cartão). Ela não disparou por um motivo só: era chaveada em **e-mail**, e o
e-mail guardado da conta adotada estava **onze horas atrasado**
(`alanvitoraraujo1@icloud.com`, de quem estava logado ali antes). Duas strings
diferentes, nenhuma gêmea detectada, uma cota apresentada como duas.

Por que ficou velho — três coisas, todas reais:

1. **Nenhum caminho barato relia identidade.** Abrir a tela chamava
   `ensureFresh` → `load("quota")` → `accounts_refresh` com `identity: false`.
   O tick de cinco minutos do status bar, idem. Só o botão *Verificar agora*
   (`load("full")`) relia — e ninguém tinha apertado desde as 12:06.
2. **Uma leitura que falhava era indistinguível de uma que não mudou.**
   `refresh_identity` dava `return` cedo sem gravar nada, então não havia como
   dizer "não confirmei desde as 12:06".
3. **O cartão misturava duas frescuras.** O carimbo "lido agora" era da *cota*,
   relida a cada poucos minutos, e era lido como se avalizasse também o nome
   acima dele — que muda só num login e por isso era relido muito menos.

---

## 2. O que foi feito

**Não reconstrua a maquinaria de gêmeas.** Ela estava certa; faltava entrada
confiável.

### 2.1 Migração 18 — três colunas em `provider_accounts`

- `account_uuid` — o identificador do provedor para a assinatura, de
  `oauthAccount.accountUuid`. **Aditivo**, nunca substituto: ausente continua
  comparando por e-mail, porque o Codex não publica equivalente.
- `identity_attempted_at` — quando a identidade foi *tentada*. `checked_at`
  passa a significar a última vez que foi **lida com sucesso**.
- `subscription_since` — desde quando esta linha pertence à assinatura em que
  está agora. Tudo em `account_limit_events` e `usage_samples` antes disso é de
  outra pessoa e é excluído de toda janela, calibração e sparkline.

### 2.2 A regra de "mesma assinatura"

`SubscriptionKey::Uuid` ganha de `Email`, **nos dois sentidos**: uuids iguais
são a mesma conta qualquer que seja o e-mail guardado, e uuids diferentes são
contas diferentes qualquer que seja o e-mail. Um lado sem uuid cai para e-mail.
Ausente é **desconhecido, nunca igual**.

O uuid só é aceito quando o `.claude.json` e o `auth status` concordam no
e-mail — um arquivo de configuração deixado por uma conta anterior não pode
grudar a assinatura errada num diretório.

> ⚠️ **Armadilha medida.** O `.claude.json` da conta **adotada** não fica dentro
> do config dir dela. Com `CLAUDE_CONFIG_DIR` definido o arquivo viaja com o
> diretório; sem a variável — que é exatamente como a conta adotada roda, de
> propósito — ele fica em `$HOME/.claude.json`, e o que existe dentro de
> `~/.claude` é um stub sem `oauthAccount`. Medido: 87 KB contra 343 bytes.
> `claude_identity_file` trata isso, e há um teste que trava o comportamento.

### 2.3 Identidade nunca mais velha

`accounts_report` — o caminho barato, o que roda ao abrir a tela e depois de
toda mutação — relê a identidade de quem precisar. Isso é o que pega um login
feito **fora do produto**, que é a única pegada que ele deixa.

O portão é `identity_is_stale`, e **ele compara a identidade, não datas de
arquivo.** A primeira versão comparava mtime e estava errada de um jeito que só
a máquina real mostra: o Claude Code reescreve `.claude.json` a cada ~10 minutos
enquanto uma sessão roda — os `.claude.json.backup.*` de um diretório vivo estão
exatamente 600 s um do outro — e reescreve `.credentials.json` a cada refresh de
token. Nada disso é troca de conta. Com mtime o portão abriria em quase toda
pintura do painel e em toda mutação da tela (`load("cached")` roda depois de
renomear, pausar, remover, ativar), iniciando uma CLI por conta cada vez:
**pausar uma conta congelaria a janela.**

Ler o `oauthAccount` do arquivo e comparar custa um read e um parse, roda dentro
do processo, e é exato. O `claude auth status` continua sendo a autoridade sobre
`signed_in` e organização — o portão só decide se vale a pena perguntar.

Há um teste que trava isso nos dois sentidos (arquivo reescrito com a mesma
conta = fechado; conta diferente = aberto), e o diagnóstico contra o registro
real afirma que o portão está **fechado logo após uma leitura bem-sucedida** —
se não estivesse, estaria comparando algo que se move sozinho.

`refresh_identity` agora carimba a tentativa mesmo quando falha, preserva a
última identidade conhecida, e quando a assinatura muda: move
`subscription_since`, apaga a leitura viva em cache (numa transação só), e
corrige o rótulo se ele era só o e-mail anterior preenchido automaticamente.

> ⚠️ **`changed` só dispara quando os dois lados são conhecidos.** Perder
> informação não é trocar de conta. Encontrado rodando o diagnóstico contra o
> registro real: o Codex 0.149.1 não escreve mais `id_token_claims`, então a
> leitura devolve e-mail nenhum para um diretório claramente logado — e a
> primeira versão gravava esse `None` por cima do endereço conhecido, lia como
> *"este diretório agora é de outra pessoa"* e **descartava todo o histórico de
> cota do Codex. A cada refresh.** Agora um campo ausente é tratado como em todo
> o resto do módulo: desconhecido, não uma afirmação.
>
> A mesma sutileza é o **caminho de upgrade de todo mundo**: toda linha chega
> nesta build com `account_uuid` nulo, e a primeira leitura preenche. Se isso
> contasse como troca, cada usuário perderia todo o histórico no primeiro
> lançamento depois de atualizar. Dois testes travam esse comportamento.

Depois de um login, **todas** as contas do provedor são relidas, não só a que
logou — a colisão só é visível comparando as duas.

### 2.4 O login em si: link + código

Medido, com o navegador impedido de completar o fluxo: a CLI imprime o link,
imprime `Paste code here if prompted > ` e **fica esperando** — ainda viva
depois de doze segundos. O `redirect_uri` é
`platform.claude.com/oauth/code/callback`: é um fluxo de **colar o código**, não
um callback em localhost.

Ou seja, recomendar a janela anônima sem ter onde devolver o código deixaria a
CLI travada para sempre num prompt que ninguém pode responder — um processo de
GUI não tem stdin de console. Então `account_begin_sign_in` mantém o stdin do
filho, e `account_submit_sign_in_code` escreve nele. O campo na tela só é usado
quando o provedor está mesmo esperando.

### 2.5 O que a tela faz a respeito

- Aviso **antes** do clique, no formulário de adicionar: o navegador vai
  reaproveitar a sessão e não vai perguntar.
- O **link de autorização** é capturado do stdout da CLI e mostrado, com botão
  de copiar, para abrir em janela anônima — a única rota confiável para outra
  conta. `switch::authorize_url` extrai por formato de URL, não pela frase em
  inglês em volta.
- No cartão gêmeo, **"Entrar com outra conta"**: desloga aquele diretório e
  reabre o login. Deslogar primeiro é a metade que importa — logar por cima de
  um diretório já autenticado é o que produziu a colisão.
- A conta **adotada** nunca oferece deslogar. É o login da máquina, muito
  possivelmente a sessão em que o Alan está trabalhando agora.
- `IdentityStamp` separa "quando perguntei quem é" de "quando perguntei quanto
  falta". Silencioso enquanto a resposta é recente.

---

## 3. Evidência

- `cargo test --no-fail-fast`: **591 passando, 0 falhando, 18 ignored**.
- `pnpm typecheck`: cinco projetos verdes.
- `pnpm run docs`: 94 páginas, `no problems`.
- Teste `#[ignore]`d contra uma **cópia** do banco real desta máquina
  (`real_machine_registry_tells_its_accounts_apart`), saída literal:

```
alanvitoraraujo1@icloud.com claude-code  email=Some("alanvitoraraujo2a@gmail.com") uuid=Some("bddb9ea1-…") signed_in=true
CLAUDE 2                    claude-code  email=Some("alanvitoraraujo2a@gmail.com") uuid=Some("bddb9ea1-…") signed_in=true
SHARED: alanvitoraraujo1@icloud.com draws on the same subscription as ["CLAUDE 2"]
SHARED: CLAUDE 2 draws on the same subscription as ["alanvitoraraujo1@icloud.com"]
```

Ou seja: o estado exato em que a máquina estava quebrada, agora lido
corretamente e sinalizado nos dois cartões, com a rotação recusando mover
trabalho entre eles.

### 3.1 QA visual, na cópia instalada de verdade

Build de release com identificador `dev.jarvis.desktop.m21qa`, passado por
`--config` na linha de comando — **`tauri.conf.json` não foi editado** — rodando
sobre uma **cópia** do `jarvis.db` do Alan. A trava de instância única é por
identificador, então isso não disputa nada com a cópia instalada. Capturas em
`.tmp/m21-qa/`.

O que a tela mostrou, com os dados reais:

- O cartão da conta da máquina, que estava intitulado
  `alanvitoraraujo1@icloud.com`, passou a se chamar `alanvitoraraujo2a@gmail.com`
  — o rótulo automático seguiu a identidade.
- **Os dois** cartões trazem o selo *Cota compartilhada* e a frase "Mesma
  assinatura que …".
- O cartão não adotado (CLAUDE 2) traz **"Entrar com outra conta"**; o adotado
  não, como deve ser.
- Os dois mostram 50% na sessão — idênticos, porque é uma assinatura só, e agora
  a tela **diz isso** em vez de deixar parecer um medidor quebrado.

E encontrou um defeito que nenhuma suíte pegaria: `.accounts__add` é um grid de
três colunas, e o aviso novo virou **uma célula** — a frase mais útil do
formulário flutuando entre dois campos, empurrando "Nome da conta" para a
terceira coluna. Corrigido com `grid-column: 1 / -1`, reconfirmado no tema claro
e no escuro por render headless (memória: uma sonda de 10 s no Edge responde uma
pergunta de CSS que um rebuild de 2 min só chuta).

Um cartão que trocou de assinatura mostra **0 tokens nas últimas 24 horas** e
uma sparkline vazia, porque `subscription_since` acabou de se mover. Isso é
correto — aquele histórico é de outra conta — mas parece bug se ninguém avisar.

---

## 4. O que continua fora do alcance deste produto

**Não dá para fazer o navegador perguntar.** A sessão do claude.ai não é nossa,
e não existe flag na CLI que force o seletor. O produto avisa antes, entrega o
link e o campo de código, e detecta depois. Para *garantir* contas distintas o
Alan precisa, uma vez por conta, abrir o link numa janela anônima ou num perfil
de navegador separado, e colar o código de volta.

**O que ainda não foi observado ponta a ponta:** um código real, de uma conta
real diferente, entrando por esse campo. O que foi medido é que a CLI fica
esperando no stdin (então o campo é necessário e o stdin está lá para recebê-lo);
o que não foi medido é o login completo, porque isso exige a segunda assinatura
que o B6 descreve.

Isso mantém o **B6** do `docs/BLOCKERS.md` no lugar: a verificação ponta a ponta
de uma troca entre duas assinaturas *genuinamente diferentes* ainda depende de
um login interativo que nenhum agente pode fazer pelo Alan.
