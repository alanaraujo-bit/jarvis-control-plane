import { Channel } from "@tauri-apps/api/core";
import { invoke, isTauri } from "./platform";

/**
 * The chat bridge (M29). Mirrors `chat::model` and `chat::Snapshot` in the
 * core; the shapes are the core's, never a provider's.
 */

export interface ChatModel {
  id: string;
  name: string;
  description: string;
  efforts: string[];
  defaultEffort: string | null;
}

export interface ChatOption {
  label: string;
  description: string;
}

export interface ChatQuestion {
  id: string;
  question: string;
  header: string;
  options: ChatOption[];
  multiSelect: boolean;
}

export type ChatAction = "command" | "edit" | "create" | "web" | "question" | "other";

export interface ChatRequest {
  id: string;
  kind: "permission" | "question";
  tool: string;
  action: ChatAction;
  detail: string;
  canAlways: boolean;
  questions: ChatQuestion[];
}

export interface ChatSnapshot {
  ready: boolean;
  models: ChatModel[];
  model: string | null;
  effort: string | null;
  busy: boolean;
  draft: string | null;
  pending: ChatRequest[];
  quickReplies: string[];
  ended: boolean;
}

export type ChatEvent =
  | { type: "ready"; models: ChatModel[]; model: string | null; effort: string | null }
  | { type: "settings"; model: string | null; effort: string | null }
  | { type: "busy"; busy: boolean }
  | { type: "draft"; text: string }
  | { type: "draftDone" }
  | { type: "request"; request: ChatRequest }
  | { type: "resolved"; id: string }
  | { type: "quickReplies"; options: string[] }
  | { type: "exited"; code: number | null };

export type ChatAnswer =
  | { type: "allow"; always: boolean }
  | { type: "deny" }
  | { type: "answers"; answers: Record<string, string[]> };

export const EMPTY_CHAT: ChatSnapshot = {
  ready: false,
  models: [],
  model: null,
  effort: null,
  busy: false,
  draft: null,
  pending: [],
  quickReplies: [],
  ended: false,
};

/** The same fold the core applies (`chat::fold`), for the live stream. */
export function foldChat(snapshot: ChatSnapshot, event: ChatEvent): ChatSnapshot {
  switch (event.type) {
    case "ready":
      return { ...snapshot, ready: true, models: event.models, model: event.model, effort: event.effort };
    case "settings":
      return { ...snapshot, model: event.model, effort: event.effort };
    case "busy":
      return { ...snapshot, busy: event.busy, quickReplies: event.busy ? [] : snapshot.quickReplies };
    case "draft":
      return { ...snapshot, draft: event.text };
    case "draftDone":
      return { ...snapshot, draft: null };
    case "request":
      return { ...snapshot, pending: [...snapshot.pending, event.request] };
    case "resolved":
      return { ...snapshot, pending: snapshot.pending.filter((request) => request.id !== event.id) };
    case "quickReplies":
      return { ...snapshot, quickReplies: event.options };
    case "exited":
      return { ...snapshot, ended: true, busy: false, draft: null, pending: [], quickReplies: [] };
  }
}

/** Start hearing a chat. Detaching stops the stream, never the agent. */
export async function attachChat(
  sessionId: string,
  onEvent: (event: ChatEvent) => void,
): Promise<{ snapshot: ChatSnapshot; detach: () => void }> {
  if (!isTauri()) return { snapshot: { ...EMPTY_CHAT, ended: true }, detach: () => {} };
  const attachmentId = crypto.randomUUID();
  const channel = new Channel<ChatEvent>();
  channel.onmessage = onEvent;
  const snapshot = await invoke<ChatSnapshot>("chat_attach", { sessionId, attachmentId, channel });
  return {
    snapshot,
    detach: () => {
      void invoke("chat_detach", { sessionId, attachmentId }).catch(() => {
        // The chat may already be gone; detaching is best-effort.
      });
    },
  };
}

export const sendChat = (sessionId: string, text: string) => invoke<void>("chat_send", { sessionId, text });
export const answerChat = (sessionId: string, requestId: string, answer: ChatAnswer) =>
  invoke<void>("chat_answer", { sessionId, requestId, answer });
export const setChatModel = (sessionId: string, model: string) => invoke<void>("chat_set_model", { sessionId, model });
export const setChatEffort = (sessionId: string, effort: string) =>
  invoke<void>("chat_set_effort", { sessionId, effort });
export const interruptChat = (sessionId: string) => invoke<void>("chat_interrupt", { sessionId });

/** Codes the core refuses with that have their own words (§65). */
const CHAT_ERRORS: Record<string, "chat.error.ended" | "chat.error.requestGone" | "chat.error.unsupported" | "chat.error.toolMissing"> = {
  "chat.ended": "chat.error.ended",
  "chat.requestGone": "chat.error.requestGone",
  "session.chatUnsupported": "chat.error.unsupported",
  "chat.toolMissing": "chat.error.toolMissing",
};

export function chatErrorKey(code: unknown) {
  return CHAT_ERRORS[String(code)] ?? "chat.error.generic";
}
