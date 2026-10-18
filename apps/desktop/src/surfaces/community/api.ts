import { create } from "zustand";
import { invoke, isTauri } from "../../app/platform";

/** The community's shapes, as `apps/social-api/src/community.ts` returns them. */
export type Visibility = "public" | "followers" | "private";

export interface Profile {
  handle: string;
  displayName: string;
  bio: string;
  location: string;
  website: string;
  github: string;
  stack: string[];
  building: string;
  statsVisibility: Visibility;
  presenceVisibility: Visibility;
  createdAt: string;
}

export interface Person {
  handle: string;
  displayName: string;
  building: string;
  followers?: number;
}

export interface Stats {
  streakCurrent: number;
  streakLongest: number;
  activeDays30: number;
  activeDays90: number;
  tokens7: number;
  tokens30: number;
  agentMinutes30: number;
  humanMinutes30: number;
  sessions30: number;
  topModels: { label: string; share: number }[];
  heatmap: { date: string; tokens: number }[];
  observedAt: string;
}

export interface ProfileView {
  profile: Profile;
  counts: { followers: number; following: number; posts: number };
  relation: { self: boolean; following: boolean; followsYou: boolean; blocked: boolean };
  stats: Stats | null;
  presence: { working: boolean; activeSessions: number; lastSeenAt: string } | null;
}

export interface Post {
  id: string;
  kind: "post" | "prompt";
  body: string;
  title: string;
  prompt: string;
  tags: string[];
  replyTo: string | null;
  createdAt: string;
  likeCount: number;
  replyCount: number;
  saveCount: number;
  likedByMe: boolean;
  savedByMe: boolean;
  author: Person;
  parent: { handle: string } | null;
}

export interface Notification {
  id: number;
  kind: "follow" | "like" | "reply" | "save";
  createdAt: string;
  read: boolean;
  actor: Person;
  postId: string | null;
  snippet: string;
}

export interface Me {
  profile: Profile | null;
  counts: { followers: number; following: number; posts: number } | null;
  unread: number;
  suggestedName: string;
}

/** The public web origin, for "copy link". Mirrors `identity::cloud::ORIGIN`. */
export const WEB_ORIGIN = "https://social-api-production-94d5.up.railway.app";

export function social<T>(method: "GET" | "POST" | "PUT" | "DELETE", path: string, body?: unknown): Promise<T> {
  return invoke<T>("social_request", { method, path: `/v1/social/${path}`, body: body ?? null });
}

export type View =
  | { kind: "feed" }
  | { kind: "profile"; handle: string }
  | { kind: "thread"; id: string }
  | { kind: "people" }
  | { kind: "activity" }
  | { kind: "edit" };

export interface Draft {
  kind: "post" | "prompt";
  title: string;
  prompt: string;
  body: string;
}

interface CommunityState {
  me: Me | null;
  /** `signedOut` when there is no J.A.R.V.I.S. account session on this machine. */
  status: "loading" | "ready" | "signedOut" | "error";
  view: View;
  /** A prompt handed over from the notebook, waiting in the composer. */
  draft: Draft | null;
  load: () => Promise<void>;
  go: (view: View) => void;
  setDraft: (draft: Draft | null) => void;
  setMe: (me: Me) => void;
}

export const useCommunity = create<CommunityState>((set) => ({
  me: null,
  status: "loading",
  view: { kind: "feed" },
  draft: null,
  load: async () => {
    // The community lives behind the desktop core; a browser preview has none.
    if (!isTauri()) return set({ status: "signedOut" });
    try {
      set({ me: await social<Me>("GET", "me"), status: "ready" });
    } catch (cause) {
      const code = String(cause);
      set({ status: code === "social.signedOut" || code === "identity.unauthorised" ? "signedOut" : "error" });
    }
  },
  go: (view) => set({ view }),
  setDraft: (draft) => set({ draft }),
  setMe: (me) => set({ me, status: "ready" }),
}));

/** "3m", "2h", "5d", or a date — how long ago, compactly. */
export function ago(iso: string, locale: string): string {
  const seconds = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000);
  if (seconds < 60) return locale === "pt-BR" ? "agora" : "now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)}min`;
  if (seconds < 86_400) return `${Math.floor(seconds / 3600)}h`;
  if (seconds < 7 * 86_400) return `${Math.floor(seconds / 86_400)}d`;
  return new Date(iso).toLocaleDateString(locale, { day: "numeric", month: "short" });
}

export const initials = (name: string) =>
  name.split(/\s+/).filter(Boolean).slice(0, 2).map((w) => [...w][0]?.toUpperCase() ?? "").join("") || "?";
