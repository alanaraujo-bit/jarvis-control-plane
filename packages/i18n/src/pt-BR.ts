import type { MessageKey } from "./en";

/**
 * Português brasileiro.
 *
 * Typed against `MessageKey`, so adding a key to `en` without translating it
 * here fails the build.
 */
export const ptBR: Record<MessageKey, string> = {
  "app.name": "J.A.R.V.I.S.",
  "app.tagline": "O centro de controle do desenvolvimento com agentes de IA.",

  "nav.missionControl": "Central de Missões",
  "nav.projects": "Projetos",
  "nav.missions": "Missões",
  "nav.activity": "Atividade",
  "nav.analytics": "Métricas",
  "nav.settings": "Configurações",

  "window.minimize": "Minimizar",
  "window.maximize": "Maximizar",
  "window.restore": "Restaurar",
  "window.close": "Fechar",
  "window.search": "Buscar ou executar um comando",

  "state.working": "Trabalhando",
  "state.waiting": "Aguardando",
  "state.idle": "Ocioso",
  "state.completed": "Concluído",
  "state.blocked": "Bloqueado",
  "state.failed": "Falhou",
  "state.ready": "Pronto",
  "state.running": "Executando",
  "state.verifying": "Verificando",

  "missionControl.title": "Central de Missões",
  "missionControl.needsAttention": "Precisa de você",
  "missionControl.working": "Trabalhando agora",
  "missionControl.recentlyCompleted": "Concluídas recentemente",
  "missionControl.activeProjects": "Projetos ativos",
  "missionControl.empty.title": "Nada em execução",
  "missionControl.empty.body":
    "Quando os agentes estiverem trabalhando, você acompanha tudo por aqui. Comece abrindo um projeto.",
  "missionControl.empty.action": "Abrir um projeto",

  "env.title": "Ambiente",
  "env.rescan": "Verificar novamente",
  "env.scanning": "Analisando seu ambiente…",
  "env.ready": "Pronto",
  "env.missing": "Não encontrado",
  "env.degraded": "Requer atenção",
  "env.required": "Obrigatório",
  "env.recommended": "Recomendado",
  "env.optional": "Opcional",
  "env.signedIn": "Conectado",
  "env.signedOut": "Não conectado",
  "env.installHint": "Instale com",
  "env.copy": "Copiar",
  "env.copied": "Copiado",
  "env.learnMore": "Saiba mais",
  "env.allReady": "Seu ambiente está pronto.",
  "env.someMissing": "Algumas ferramentas estão faltando.",

  "settings.appearance": "Aparência",
  "settings.theme": "Tema",
  "settings.theme.dark": "Escuro",
  "settings.theme.light": "Claro",
  "settings.theme.system": "Sistema",
  "settings.language": "Idioma",


  "projects.openFolder": "Abrir pasta",
  "projects.empty.title": "Nenhum projeto ainda",
  "projects.empty.body":
    "Abra uma pasta deste computador para começar. Nada é enviado para lugar nenhum — o J.A.R.V.I.S. trabalha nos seus arquivos onde eles já estão.",
  "projects.missing": "Pasta não encontrada",
  "projects.back": "Todos os projetos",

  "terminal.title": "Terminal",
  "terminal.new": "Novo terminal",
  "terminal.shell": "Shell",
  "terminal.claudeCode": "Claude Code",
  "terminal.codex": "Codex",
  "terminal.close": "Fechar aba",
  "terminal.empty.title": "Nenhum terminal em execução",
  "terminal.empty.body": "Inicie um shell ou execute um agente neste projeto.",
  "terminal.notInstalled": "{name} não está instalado",


  "conversation.title": "Conversa",
  "conversation.you": "Você",
  "conversation.agent": "Agente",
  "conversation.thinking": "Raciocínio",
  "conversation.in": "entrada",
  "conversation.out": "saída",
  "conversation.cached": "cache",
  "conversation.usageOfficial": "Informado pelo provedor",
  "conversation.empty.title": "Nada para mostrar ainda",
  "conversation.empty.body":
    "Esta sessão ainda não produziu saída estruturada. O terminal mostra tudo em tempo real.",
  "view.terminal": "Terminal",
  "view.conversation": "Conversa",

  "common.cancel": "Cancelar",
  "common.confirm": "Confirmar",
  "common.retry": "Tentar novamente",
  "common.dismiss": "Dispensar",
  "common.loading": "Carregando…",
};
