# M20 — Identidade: contas de pessoa (§67)

**Estado: concluído em 2026-08-25. Funcional, testado e validado olhando a
tela.** Este arquivo é o registro do trabalho — plano, decisões, e o que foi
verificado de que jeito. Comece por aqui e depois por `docs/HANDOFF.md` e D48.

---

## 1. O que o Alan pediu

> "já está na hora de irmos para outro patamar, termos contas para guardar
> configurações dos usuários e tudo mais que fizer sentido no banco de dados,
> vamos fazer uma tela de login agradável e elegante já com o botão de entrar
> via Google já que vamos ter isso também futuramente, quero uma tela de login
> e cadastro bem feita e profissional, com sacadas geniais e animações tipo
> motion e elementos visuais foda."

Três entregas, então: **a conta**, **o que a conta guarda**, e **a tela**.

---

## 2. A decisão que define tudo: entrar não é um portão

A tentação óbvia é fazer da tela de login um portão — nada acontece até
alguém entrar. Isso está errado neste produto, por três razões concretas:

1. **§3 local-first é inegociável.** Este produto trabalha com os projetos, as
   sessões e as credenciais da máquina. Um portão transforma uma ferramenta
   local em uma que não abre sem servidor de identidade.
2. **Metade do produto roda sem ninguém presente.** Execuções não assistidas
   (§32), o backfill da Busca Global cinco segundos depois do launch, o feed de
   notificações e o loop do relay começam sozinhos. Pergunta concreta:
   o que `autopilot_start` faz às 3 da manhã se ninguém está logado? Acoplar
   trabalho de agente a um login é o tipo de coisa que só aparece depois.
3. **A instalação do Alan já tem dados reais** — projetos e 42 diretórios de
   sessão. Uma atualização que exige cadastro antes de chegar neles é a coisa
   menos reversível desta tarefa inteira.

**Portanto:** a conta é **aditiva**. A máquina continua funcionando sem nenhuma
conta, exatamente como hoje. A tela aparece uma vez, oferece **Continuar sem
conta** com o mesmo peso visual de entrar, e depois vive no Rail/Settings. Nada
no núcleo — nem `autopilot`, nem `search::backfill`, nem `session::launch` —
pergunta quem está logado. Se alguma coisa um dia perguntar, isso é uma decisão
deliberada e vai estar escrita aqui.

## 3. Por que `identity` e não `accounts`

`src-tauri/src/accounts/`, `src/surfaces/accounts/`, o item do Rail e o
namespace `accounts.*` do i18n **já significam outra coisa**: as quatro
assinaturas Claude/Codex do M13/M16, onde uma conta *é* um diretório de
configuração. Duas coisas chamadas "Contas" na mesma interface é confusão real,
não implicância de nomenclatura. Aqui: `identity` no módulo Rust, na superfície,
no namespace do i18n e neste documento.

## 4. Esquema (migração 17)

`identity_accounts` e `identity_settings`. Duas regras que valem a pena
registrar:

- **`settings` não ganhou coluna de escopo.** O contrato daquela tabela é
  "não escolhido tem uma grafia só: nenhuma linha", e `mission::store`,
  `onboarding` e `settings::get/set` leem sem escopo. Acrescentar `account_id`
  mudaria em silêncio o que cada leitor existente enxerga. Preferência de
  pessoa mora em `identity_settings`; `settings` continua sendo da **máquina**.
- **Quem está logado é fato da máquina**, não da conta — mora em `settings`,
  chave `identity.signedIn`.

## 5. O que a conta carrega, e como

Tema, idioma, tamanho da fonte do terminal, scrollback, orçamento de turnos do
autopilot e as três chaves de notificação. **Espelho, não escopo**: entrar
aplica os valores da conta; mudar algo enquanto logado grava nos dois lugares.
Nenhum leitor existente muda de comportamento — é a mudança mais reversível que
entrega a frase "suas configurações te acompanham".

Sair **não** desfaz o que está aplicado. Sair não é motivo para a interface
mudar de cara na frente da pessoa; entrar de novo restaura o que é seu.

Criar conta **herda** o que a máquina já está usando, para que cadastrar-se
nunca zere o app de quem já o estava usando.

## 6. Google — presente e honestamente indisponível

O botão existe, com a marca certa, e diz o motivo de não funcionar ainda. É o
mesmo tratamento que notificação push recebe ("ausente de propósito, não
fingida" — §81). O que falta é uma credencial que só o Alan pode criar:
registrado em `docs/BLOCKERS.md` como **B7**.

Deliberadamente **não** construído: o fluxo loopback/PKCE inteiro. A forma do
redirect e se loopback é permitido dependem do tipo de client que ainda não
existe — um subsistema não testável é pior do que uma função claramente marcada.

### Atualização — Google e sincronização, 2026-08-27

B7 foi resolvido com um cliente Web e callback no `social-api` de produção.
O navegador do sistema faz o consentimento; o backend troca e verifica o token
do Google, entrega ao desktop uma sessão revogável de 90 dias e guarda no
Postgres as preferências carregadas e snapshots sanitizados das cotas. Tokens
dos provedores de IA e diretórios locais de configuração não atravessam essa
fronteira.

---

## 6.1 Argon2 é rápido o bastante aqui — medido, não suposto

O aviso valia: um build de debug pode tornar o hash lento a ponto de parecer
travado, e a tela seria desenhada em volta de uma espera que não existe. Medido:
26 testes com mais de quarenta operações de hash/verificação em **debug**
terminam em 2,47s — algo como 60ms por operação. Nada a ajustar, e o botão de
entrar não precisa de um estado de espera elaborado.

## 7. O que foi feito, e como foi verificado

- [x] Orientação, consulta ao revisor, decisão de arquitetura (seção 2)
- [x] Migração 17 — impressão digital registrada
- [x] Módulo `identity` + 27 testes
- [x] Strings en + pt-BR (tipadas contra o catálogo inglês)
- [x] Tela de login/cadastro
- [x] Perfil no Settings
- [x] Página **Conta** no site de documentação, nos dois idiomas
- [x] Suíte inteira: **609 testes** verdes (578 Rust, 9 i18n, 22 relay)
- [x] Verificação visual nos dois temas e nos dois idiomas, no binário real
- [x] DECISIONS (D48), ROADMAP (M20), HANDOFF, BLOCKERS (B7)

### O que foi realmente exercitado no app

Criar conta → sair → senha errada (contagem e plural corretos) → entrar → e o
ponto da funcionalidade inteira: entrar numa máquina em **escuro + pt-BR** e
vê-la virar **claro + inglês**, porque era isso que a conta carregava. Apagar
com a senha errada (recusado) e depois com a certa (apagada, e o app seguiu
igual). Cores amostradas com `GetPixel`, não julgadas a olho: fundo `#0C0C0D`
no escuro e `#F7F7F5` no claro — exatamente `--bg-base` — com as luzes de
ambiente deslocando no máximo duas unidades, que é a checagem de que a tela é
*iluminada* e não *colorida*. Âmbar continua significando trabalho de agente.

### Os três bugs que só o olhar encontrou

1. **A tela de login não fechava depois de um cadastro bem-sucedido.**
   `mark_prompted` rodava no comando, *depois* de `sign_up` já ter montado o
   relatório de resposta — então uma chamada que acabara de logar alguém
   devolvia `prompted: false`, e a tela que decide o que desenhar a partir
   exatamente desse par continuava desenhada por cima de uma conta criada
   corretamente. Nada deu erro. Agora é marcado em `seat`, onde estar logado
   *é* a oferta ter sido respondida. Tem teste com o nome disso.
2. **Dois olhos no campo de senha.** O WebView2 desenha o próprio controle de
   revelar dentro de um `input[type=password]`. Só aparece depois que uma senha
   é digitada de verdade, então ler o markup nunca mostraria.
3. **Setenta e dois pixels de cartão vazio.** `grid-template-rows: 0fr → 1fr`
   é a forma certa de animar uma altura que ninguém consegue medir antes, e
   `min-height: 0` no filho é só metade da receita. O mínimo automático de uma
   trilha de grid é a **margin box** do item, e `min-height` zera só a content
   box — então cada pixel de padding ou margem no elemento recortado sobrevive
   ao colapso. Resolvido medindo o markup real em Edge headless
   (`getBoundingClientRect`: 8px, 8px, 16px — exatamente os paddings) e movendo
   todo espaçamento para um **filho** da caixa recortada.

O quarto ponto é o que vale guardar: **nenhum dos três era visível no código.**
Cada arquivo lê corretamente sozinho, e tudo compilava e passava em todos os
testes durante os três.
