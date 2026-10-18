/** M28: the community's rules, as pure functions — what a profile, a post and
 *  a stats snapshot may contain, and who may see what. This module is the
 *  trust boundary's logic; community.ts only stores and fetches. */

export type Visibility = "public" | "followers" | "private";
export const VISIBILITIES = new Set<Visibility>(["public", "followers", "private"]);
export const HANDLE = /^[a-z0-9_]{3,24}$/;
const GITHUB = /^[A-Za-z0-9-]{1,39}$/;
const TAG = /^[a-z0-9-]{1,24}$/;
const DATE = /^\d{4}-\d{2}-\d{2}$/;

export type Result<K extends string, T> = { [key in K]: T } | { error: string };

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
}

const text = (value: unknown) => (typeof value === "string" ? value.trim() : "");

/** A whole profile, as sent by its owner. `creating` requires a handle; an
 *  update may omit it to keep the current one. */
export function parseProfile(input: unknown, creating: boolean): Result<"profile", Profile> {
  const raw = (input && typeof input === "object" ? input : {}) as Record<string, unknown>;
  const handle = text(raw.handle).toLowerCase();
  if ((creating || raw.handle !== undefined) && !HANDLE.test(handle)) return { error: "social.handleInvalid" };
  const displayName = text(raw.displayName);
  if (!displayName) return { error: "social.nameRequired" };
  if ([...displayName].length > 60) return { error: "social.nameTooLong" };
  const bio = text(raw.bio);
  if ([...bio].length > 280) return { error: "social.bioTooLong" };
  const location = text(raw.location);
  if ([...location].length > 60) return { error: "social.locationTooLong" };
  const building = text(raw.building);
  if ([...building].length > 140) return { error: "social.buildingTooLong" };
  const website = text(raw.website);
  if (website) {
    const ok = (() => {
      try {
        const url = new URL(website);
        return (url.protocol === "https:" || url.protocol === "http:") && website.length <= 200;
      } catch {
        return false;
      }
    })();
    if (!ok) return { error: "social.websiteInvalid" };
  }
  const github = text(raw.github).replace(/^@/, "");
  if (github && !GITHUB.test(github)) return { error: "social.githubInvalid" };
  const stack: string[] = [];
  for (const item of Array.isArray(raw.stack) ? raw.stack : []) {
    const value = text(item);
    if (!value || [...value].length > 24) return { error: "social.stackInvalid" };
    if (!stack.some((s) => s.toLowerCase() === value.toLowerCase())) stack.push(value);
  }
  if (stack.length > 12) return { error: "social.stackInvalid" };
  const statsVisibility = (raw.statsVisibility ?? "private") as Visibility;
  const presenceVisibility = (raw.presenceVisibility ?? "private") as Visibility;
  if (!VISIBILITIES.has(statsVisibility) || !VISIBILITIES.has(presenceVisibility)) return { error: "social.visibilityInvalid" };
  return { profile: { handle, displayName, bio, location, website, github, stack, building, statsVisibility, presenceVisibility } };
}

export interface NewPost {
  kind: "post" | "prompt";
  body: string;
  title: string;
  prompt: string;
  tags: string[];
  replyTo: string | null;
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function parsePost(input: unknown): Result<"post", NewPost> {
  const raw = (input && typeof input === "object" ? input : {}) as Record<string, unknown>;
  const kind = raw.kind;
  if (kind !== "post" && kind !== "prompt") return { error: "social.kindInvalid" };
  const body = text(raw.body);
  if ([...body].length > 2000) return { error: "social.bodyTooLong" };
  const title = text(raw.title);
  const prompt = typeof raw.prompt === "string" ? raw.prompt.replace(/\s+$/, "") : "";
  if (kind === "post" && !body) return { error: "social.bodyRequired" };
  if (kind === "prompt") {
    if (!title) return { error: "social.titleRequired" };
    if ([...title].length > 120) return { error: "social.titleTooLong" };
    if (!prompt.trim()) return { error: "social.promptRequired" };
    if (prompt.length > 20_000) return { error: "social.promptTooLong" };
  }
  const tags: string[] = [];
  for (const item of Array.isArray(raw.tags) ? raw.tags : []) {
    const tag = text(item).replace(/^#/, "").toLowerCase();
    if (!TAG.test(tag)) return { error: "social.tagInvalid" };
    if (!tags.includes(tag)) tags.push(tag);
  }
  if (tags.length > 5) return { error: "social.tooManyTags" };
  const replyTo = typeof raw.replyTo === "string" && UUID.test(raw.replyTo) ? raw.replyTo : null;
  if (raw.replyTo !== undefined && raw.replyTo !== null && !replyTo) return { error: "social.replyInvalid" };
  if (replyTo && kind !== "post") return { error: "social.replyInvalid" };
  return { post: { kind, body, title: kind === "prompt" ? title : "", prompt: kind === "prompt" ? prompt : "", tags, replyTo } };
}

/** The viewer's relation to a profile. */
export interface Relation {
  self: boolean;
  following: boolean;
}

export function canSee(visibility: Visibility, relation: Relation): boolean {
  return relation.self || visibility === "public" || (visibility === "followers" && relation.following);
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
}

const count = (value: unknown) => (value === undefined ? 0 : typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null);

/** A published snapshot. Only these aggregate fields are kept; anything else a
 *  client sends (a project name, a path) is dropped, never stored. */
export function parseStats(input: unknown): Stats | null {
  const raw = (input && typeof input === "object" ? input : {}) as Record<string, unknown>;
  const fields = ["streakCurrent", "streakLongest", "activeDays30", "activeDays90", "tokens7", "tokens30", "agentMinutes30", "humanMinutes30", "sessions30"] as const;
  const numbers: Record<string, number> = {};
  for (const field of fields) {
    const value = count(raw[field]);
    if (value === null) return null;
    numbers[field] = value;
  }
  const topModels: Stats["topModels"] = [];
  for (const item of Array.isArray(raw.topModels) ? raw.topModels.slice(0, 5) : []) {
    const label = text((item as Record<string, unknown>)?.label);
    const share = Number((item as Record<string, unknown>)?.share);
    if (!label || label.length > 60 || !(share >= 0 && share <= 1)) return null;
    topModels.push({ label, share: Math.round(share * 1000) / 1000 });
  }
  const heatmap: Stats["heatmap"] = [];
  for (const item of Array.isArray(raw.heatmap) ? raw.heatmap.slice(-182) : []) {
    const date = text((item as Record<string, unknown>)?.date);
    const tokens = count((item as Record<string, unknown>)?.tokens);
    if (!DATE.test(date) || tokens === null) return null;
    heatmap.push({ date, tokens });
  }
  return { ...(numbers as Omit<Stats, "topModels" | "heatmap">), topModels, heatmap };
}

export function escapeHtml(value: unknown): string {
  return String(value ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}
