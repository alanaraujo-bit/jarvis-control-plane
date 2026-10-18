/** M28: the public web pages — a person (`/u/<handle>`) and a post
 *  (`/p/<id>`), readable without the app. Only what the person made public
 *  arrives here; every value is escaped and the CSP allows no script. */
import { escapeHtml as e, type Stats } from "./community-rules.ts";

export interface PageProfile {
  handle: string;
  displayName: string;
  bio: string;
  location: string;
  website: string;
  github: string;
  stack: string[];
  building: string;
  createdAt: string;
}

export interface PagePost {
  id: string;
  kind: string;
  body: string;
  title: string;
  prompt: string;
  tags: string[];
  createdAt: string;
  likeCount: number;
  replyCount: number;
  saveCount: number;
  author: { handle: string; displayName: string };
}

export const PAGE_CSP = "default-src 'none'; style-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'";

const STYLE = `
:root{color-scheme:dark;--bg:#0c0c0d;--panel:#141415;--line:#232325;--text:#ededef;--muted:#9e9ea6;--faint:#6b6b72;--amber:#e0a33f;--h0:#1b1b1d;--h1:#34343a;--h2:#55555d;--h3:#8a8a93;--h4:#d6d6db}
@media (prefers-color-scheme:light){:root{color-scheme:light;--bg:#f7f7f5;--panel:#ffffff;--line:#e4e4e0;--text:#18181a;--muted:#5b5b62;--faint:#8a8a90;--amber:#96650e;--h0:#ececea;--h1:#cfcfcb;--h2:#a3a39f;--h3:#6e6e6a;--h4:#2a2a2a}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--text);font:14px/1.55 system-ui,-apple-system,"Segoe UI",sans-serif}
main{max-width:760px;margin:0 auto;padding:40px 20px 64px}a{color:inherit}
.card{background:var(--panel);border:1px solid var(--line);border-radius:10px;padding:20px;margin-top:16px}
.head{display:flex;gap:16px;align-items:center}.mono{width:56px;height:56px;border-radius:50%;display:grid;place-items:center;background:var(--h0);border:1px solid var(--line);font-weight:600;font-size:18px;color:var(--muted)}
h1{font-size:22px;margin:0}.handle{color:var(--muted)}.building{margin:12px 0 0;color:var(--text)}.bio{margin:8px 0 0;color:var(--muted);white-space:pre-wrap}
.meta{display:flex;flex-wrap:wrap;gap:6px 16px;margin-top:12px;color:var(--faint);font-size:13px}.chips{display:flex;flex-wrap:wrap;gap:6px;margin-top:12px}
.chip{border:1px solid var(--line);border-radius:999px;padding:1px 9px;font-size:12px;color:var(--muted)}
.stats{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:12px}.stat b{display:block;font-size:20px;font-variant-numeric:tabular-nums}.stat span{color:var(--faint);font-size:12px}
.badge{font-size:11px;color:var(--faint);border:1px solid var(--line);border-radius:4px;padding:0 5px;margin-left:6px}
.post{border-top:1px solid var(--line);padding:16px 0}.post:first-child{border-top:0;padding-top:0}.post .when{color:var(--faint);font-size:12px}
.post p{white-space:pre-wrap;margin:6px 0 0}.prompt{margin-top:8px;border:1px solid var(--line);border-radius:6px;padding:10px 12px;background:var(--bg)}
.prompt pre{margin:6px 0 0;white-space:pre-wrap;font:12.5px/1.5 ui-monospace,Consolas,monospace;color:var(--muted);max-height:240px;overflow:hidden}
.counts{color:var(--faint);font-size:12px;margin-top:8px}.working{color:var(--amber)}.foot{margin-top:32px;color:var(--faint);font-size:12px;text-align:center}
svg{display:block;max-width:100%}h2{font-size:13px;text-transform:uppercase;letter-spacing:.04em;color:var(--faint);margin:0 0 12px}
`;

function frame(title: string, description: string, body: string): string {
  return `<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>${e(title)}</title><meta name="description" content="${e(description)}"><meta property="og:title" content="${e(title)}"><meta property="og:description" content="${e(description)}">
<style>${STYLE}</style><main>${body}<p class="foot">J.A.R.V.I.S. community</p></main></html>`;
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
const day = (iso: string) => new Date(iso).toISOString().slice(0, 10);
const thousands = (n: number) => (n >= 1_000_000 ? `${(n / 1_000_000).toFixed(1)}M` : n >= 1_000 ? `${(n / 1_000).toFixed(1)}K` : String(n));
const initials = (name: string) => name.split(/\s+/).filter(Boolean).slice(0, 2).map((w) => [...w][0]?.toUpperCase() ?? "").join("") || "?";

/** A 26-week heatmap in neutral steps: colour reports activity, not brand. */
export function heatmapSvg(cells: { date: string; tokens: number }[]): string {
  if (cells.length === 0) return "";
  const max = Math.max(...cells.map((c) => c.tokens), 1);
  const level = (t: number) => (t === 0 ? 0 : Math.min(4, 1 + Math.floor((t / max) * 3.999)));
  const first = new Date(`${cells[0].date}T00:00:00Z`).getUTCDay();
  const rects = cells.map((c, i) => {
    const slot = i + first;
    const x = Math.floor(slot / 7) * 13, y = (slot % 7) * 13;
    return `<rect x="${x}" y="${y}" width="10" height="10" rx="2" fill="var(--h${level(c.tokens)})"><title>${e(c.date)}: ${c.tokens} tokens</title></rect>`;
  });
  const width = Math.ceil((cells.length + first) / 7) * 13;
  return `<svg width="${width}" height="91" viewBox="0 0 ${width} 91" role="img" aria-label="activity">${rects.join("")}</svg>`;
}

function postHtml(post: PagePost, linkSelf: boolean): string {
  const prompt = post.kind === "prompt"
    ? `<div class="prompt"><b>${e(post.title)}</b><pre>${e(post.prompt)}</pre></div>`
    : "";
  const tags = post.tags.length ? `<div class="chips">${post.tags.map((t) => `<span class="chip">#${e(t)}</span>`).join("")}</div>` : "";
  const when = linkSelf ? `<a class="when" href="/p/${e(post.id)}">${e(day(post.createdAt))}</a>` : `<span class="when">${e(day(post.createdAt))}</span>`;
  return `<article class="post"><div><b>${e(post.author.displayName)}</b> <span class="handle">@${e(post.author.handle)}</span> · ${when}</div>
${post.body ? `<p>${e(post.body)}</p>` : ""}${prompt}${tags}<div class="counts">${plural(post.likeCount, "like", "likes")} · ${plural(post.replyCount, "reply", "replies")}${post.kind === "prompt" ? ` · saved ${post.saveCount}×` : ""}</div></article>`;
}

export function renderProfilePage(input: {
  profile: PageProfile;
  counts: { followers: number; following: number; posts: number };
  stats: (Stats & { observedAt: string }) | null;
  working: boolean | null;
  posts: PagePost[];
}): string {
  const { profile, counts, stats, working, posts } = input;
  const links = [
    profile.location && `<span>${e(profile.location)}</span>`,
    profile.website && `<a href="${e(profile.website)}" rel="nofollow noopener">${e(profile.website.replace(/^https?:\/\//, ""))}</a>`,
    profile.github && `<a href="https://github.com/${e(profile.github)}" rel="nofollow noopener">github.com/${e(profile.github)}</a>`,
    `<span>joined ${e(day(profile.createdAt))}</span>`,
  ].filter(Boolean).join("");
  const statsHtml = stats
    ? `<section class="card"><h2>How they build <span class="badge">observed · ${e(day(stats.observedAt))}</span></h2><div class="stats">
<div class="stat"><b>${stats.streakCurrent}d</b><span>current streak · longest ${stats.streakLongest}d</span></div>
<div class="stat"><b>${stats.activeDays30}/30</b><span>active days</span></div>
<div class="stat"><b>${thousands(stats.tokens30)}</b><span>tokens, 30 days</span></div>
<div class="stat"><b>${Math.round(stats.agentMinutes30 / 60)}h</b><span>agent time · ${Math.round(stats.humanMinutes30 / 60)}h hands-on</span></div>
</div>${stats.topModels.length ? `<div class="chips">${stats.topModels.map((m) => `<span class="chip">${e(m.label)} ${m.share > 0 && m.share < 0.01 ? "<1" : Math.round(m.share * 100)}%</span>`).join("")}</div>` : ""}
<div style="margin-top:16px">${heatmapSvg(stats.heatmap)}</div></section>`
    : "";
  const body = `<section class="card"><div class="head"><div class="mono" aria-hidden="true">${e(initials(profile.displayName))}</div><div>
<h1>${e(profile.displayName)}</h1><div class="handle">@${e(profile.handle)}${working ? ` · <span class="working">● working now</span>` : ""}</div></div></div>
${profile.building ? `<p class="building">${e(profile.building)}</p>` : ""}${profile.bio ? `<p class="bio">${e(profile.bio)}</p>` : ""}
<div class="meta">${links}</div>${profile.stack.length ? `<div class="chips">${profile.stack.map((s) => `<span class="chip">${e(s)}</span>`).join("")}</div>` : ""}
<div class="meta"><span><b>${counts.posts}</b> ${counts.posts === 1 ? "post" : "posts"}</span><span><b>${counts.followers}</b> ${counts.followers === 1 ? "follower" : "followers"}</span><span><b>${counts.following}</b> following</span></div></section>
${statsHtml}<section class="card"><h2>Posts</h2>${posts.length ? posts.map((p) => postHtml(p, true)).join("") : `<p class="handle">Nothing posted yet.</p>`}</section>`;
  return frame(`${profile.displayName} (@${profile.handle})`, profile.building || profile.bio || "On the J.A.R.V.I.S. community", body);
}

export function renderPostPage(post: PagePost, replies: PagePost[]): string {
  const body = `<section class="card">${postHtml(post, false)}</section>${replies.length ? `<section class="card"><h2>Replies</h2>${replies.map((r) => postHtml(r, true)).join("")}</section>` : ""}`;
  const title = post.kind === "prompt" ? `${post.title} — prompt by @${post.author.handle}` : `@${post.author.handle} on J.A.R.V.I.S.`;
  return frame(title, (post.body || post.prompt).slice(0, 160), body);
}

export function renderNotFound(): string {
  return frame("Not found", "", `<section class="card"><h1>Not here</h1><p class="bio">This profile or post does not exist, or is not public.</p></section>`);
}
