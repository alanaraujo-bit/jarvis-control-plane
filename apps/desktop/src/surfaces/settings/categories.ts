import {
  Bell,
  CircleUser,
  Download,
  Gauge,
  Palette,
  ShieldCheck,
  Smartphone,
  SquareTerminal,
  Wrench,
  type LucideIcon,
} from "lucide-react";
import type { MessageKey } from "@jarvis/i18n";

/**
 * The sections of Settings (§64).
 *
 * Settings used to be one long scroll of every group at once: theme, terminal,
 * autonomy, guardrails, notifications, the phone, the environment scan and the
 * updater, stacked. Everything was visible, which sounds like a virtue and is
 * not — nothing was *findable*, because a page where every group is equally
 * present gives the eye nowhere to start.
 *
 * So the groups became destinations. One is on screen at a time, chosen from a
 * column on the left, and the column is the map: eight named places rather than
 * a scrollbar you have to explore. This reverses the note the old file carried
 * ("flat sections rather than a nested menu tree") on purpose — see D45.
 *
 * The list is data for the same reason the rail is (§87 in `Rail.tsx`): a new
 * section is an entry here, not a component that has to be restructured.
 */
export const CATEGORIES = [
  // Account first: it is the only section that says who the product thinks you
  // are, and the one somebody looks for when they have just been offered an
  // account and want to know what happened to that choice (M20).
  "account",
  "appearance",
  "terminal",
  "autonomy",
  "guardrails",
  "notifications",
  "companion",
  "environment",
  "updates",
] as const;

export type CategoryId = (typeof CATEGORIES)[number];

export interface Category {
  id: CategoryId;
  icon: LucideIcon;
  /**
   * The name in the left column — deliberately the key the thing already had
   * rather than a nav-only duplicate of it, so "Guardrails" is spelled once in
   * the catalogue and cannot drift from the heading it names.
   */
  label: MessageKey;
  /**
   * The one line under the heading, for sections whose panel does not already
   * introduce itself. Where a panel carries its own title and subtitle —
   * Autonomy, Guardrails, the phone, Environment, Updates — the pane renders
   * no heading at all rather than saying the same thing twice, which is half
   * of what made the old screen feel cluttered.
   */
  blurb?: MessageKey;
  /** Words someone might type in the command palette to get here. */
  keywords: string;
}

export const CATEGORY: Record<CategoryId, Category> = {
  account: {
    id: "account",
    icon: CircleUser,
    label: "identity.settings.title",
    blurb: "identity.settings.blurb",
    keywords:
      "account sign in out profile password login conta entrar sair perfil senha cadastro login",
  },
  // Ordered by how often somebody actually comes here. Appearance is the first
  // thing anyone changes in a new app; Updates is the thing you visit twice a
  // year. Frequency of use decides the order, as it did before (§64).
  appearance: {
    id: "appearance",
    icon: Palette,
    label: "settings.appearance",
    blurb: "settings.appearance.blurb",
    keywords: "appearance theme language aparência tema idioma dark light escuro claro",
  },
  terminal: {
    id: "terminal",
    icon: SquareTerminal,
    label: "settings.terminal",
    blurb: "settings.terminal.blurb",
    keywords: "terminal font size scrollback fonte tamanho histórico historico",
  },
  autonomy: {
    id: "autonomy",
    icon: Gauge,
    label: "autonomy.title",
    keywords: "autonomy agents turns budget autonomia agentes turnos limite",
  },
  guardrails: {
    id: "guardrails",
    icon: ShieldCheck,
    label: "guardrail.title",
    keywords: "guardrails policy permissions proteções protecoes permissões regras",
  },
  notifications: {
    id: "notifications",
    icon: Bell,
    label: "settings.notifications",
    blurb: "settings.notifications.blurb",
    keywords: "notifications sound toast notificações notificacoes som aviso",
  },
  companion: {
    id: "companion",
    icon: Smartphone,
    label: "settings.companion",
    keywords: "phone mobile companion pair celular telefone parear",
  },
  environment: {
    id: "environment",
    icon: Wrench,
    label: "env.title",
    blurb: "settings.environment.blurb",
    keywords: "environment tools scan doctor ambiente ferramentas verificar",
  },
  updates: {
    id: "updates",
    icon: Download,
    label: "update.title",
    blurb: "settings.updates.blurb",
    keywords: "updates version upgrade atualizações atualizacoes versão versao",
  },
};

export const DEFAULT_CATEGORY: CategoryId = "appearance";
