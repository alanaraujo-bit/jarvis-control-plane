# M23 — Sync que sobrevive a uma reinstalação

## O problema, medido

Auditoria em 2026-08-27 contra o Postgres de produção (`jarvis-social`):

```
identity_users            1 linha
identity_quota_snapshots  1 linha (6770 bytes, escrita no mesmo dia)
identity_settings         0 linhas
```

O snapshot de quota chegou pelo mesmo token, mesma rota `PUT /v1/sync/*` e mesma
thread fire-and-forget que as preferências. Logo auth, rede e escrita funcionam:
o que falta é **conteúdo para enviar** e **um caminho de volta**.

Três falhas distintas:

1. **Bloco de notas nunca sincronizou.** Não há tabela, endpoint nem comando.
   `carriedKeys` no servidor é uma allowlist de 8 chaves de preferência.
2. **Conta por e-mail e senha não existe na nuvem.** `identity_sign_up` e
   `identity_sign_in` são Argon2 + SQLite puro; só o caminho do Google toca o
   Railway. Reinstalar perde a própria conta, não só os dados dela.
3. **As 8 preferências carregadas também estão vazias**, e a causa é precisa:
   `settings` local só ganha linha quando o valor sai do padrão, então
   `prefs::all()` devolve `{}` e o push inicial envia um objeto vazio. Tema e
   idioma vivem no `localStorage` e só chegam ao core quando *mudam*.

E, estruturalmente: `GET /v1/sync/state` não tinha um único chamador. Não havia
pull. O que subia era escrito e nunca lido de volta.

## O que sobe e o que fica

Fica local, deliberadamente — a fronteira que `identity/cloud.rs` já declara
("provider credentials and configuration directories never cross this
boundary") continua valendo:

- `provider_accounts` e os diretórios de configuração do provedor (M13/M16);
- `onboarding.seen` — é um fato sobre *esta* máquina;
- `guardrail_policies`, que são por pasta;
- caminhos absolutos de projeto, sessões, transcrições, `usage_samples`.

Sobe:

- as preferências carregadas, agora com um push inicial que envia valores reais;
- `performance.hudEnabled`, que a árvore de trabalho já tentava lembrar e que
  falhava silenciosamente por não estar em nenhuma das duas allowlists;
- o bloco de notas inteiro: pastas e notas, com exclusões.

## Conflito: last-write-wins por linha, com lápides

A biblioteca é de uma pessoa e tem centenas de linhas, não milhões, mas duas
máquinas ainda editam a mesma nota. Cada linha carrega `touched_at`; quem tem o
maior vence, dos dois lados. Exclusões viram linhas em `sync_tombstones`
localmente e `deleted_at` no servidor — sem isso um pull ressuscita o que a
outra máquina apagou.

`touched_at` é uma coluna nova em vez de reusar `updated_at` porque
`set_note_pinned` e `move_note` **deliberadamente não tocam** `updated_at`
(fixar não é editar, e a lista não deve se reordenar sob quem fixou). Um sync
que lesse `updated_at` deixaria o estado de fixação reverter no próximo pull.
`updated_at` continua sendo a ordem que a tela desenha; `touched_at` é o relógio
do sync, e nada além do sync o lê.

Uma máquina compartilhada continua mostrando a biblioteca de quem entrou antes
até o primeiro pull dar certo. É deliberado: uma biblioteca que nunca chegou ao
servidor existe num lugar só, e um login offline é exatamente o momento em que
não dá para verificar se ela está segura em outro. Fechar isso de verdade pede
uma coluna de conta em `notebooks` — a mudança que a migração 17 recusou para
`settings` — e merece ser discutida, não contrabandeada junto com esta.

## Conta: nuvem quando dá, local sempre

Local-first não muda: nada no núcleo pergunta quem está logado antes de
trabalhar, e uma máquina sem rede continua criando e abrindo contas.

- **Cadastro** valida local, tenta `POST /v1/auth/sign-up` e, dando certo, usa o
  id do servidor como id local e guarda a sessão. Sem rede, cria só local — e
  o próximo login com rede promove a conta.
- **Login** tenta o hash local primeiro (funciona offline). Sem conta local, ou
  com senha que o local recusa mas a nuvem aceita (a senha mudou em outra
  máquina), o servidor decide e o hash local é atualizado.
- A senha nunca cruza como texto em repouso: o servidor guarda scrypt com sal
  por linha; a máquina guarda Argon2id como já guardava.

## Pull no launch

Um thread em `setup`, fora do caminho de partida, faz `GET /v1/sync/state`,
funde preferências e bloco de notas e emite `sync://state` para a interface
recarregar o que estiver aberto. Falhar é silencioso por construção: sem rede o
produto é exatamente o que era.
