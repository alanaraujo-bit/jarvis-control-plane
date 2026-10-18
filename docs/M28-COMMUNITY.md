# M28 — The J.A.R.V.I.S. community

**State: built and verified (2026-10-18).** The server is deployed; the
desktop changes are local commits and reach the installed app with the next
release.

## Why

Alan asked for a real social network inside the product, "focused on our
ecosystem": every person has a public profile that says who they are and how
they build with agents, and there is a place to follow people, post, and share.
He left every design decision to me.

What existed was an MVP: a separate social account with its own token, friend
requests, and metrics nobody ever published. It has no feed, no public profile
and no way to share anything.

## What makes it ours

This is not a generic social feed bolted onto a desktop app. It is about
building with AI agents, and it uses data only this product has:

- **A profile that shows how you build.** It shows your streak, your active
  days, the tokens your agents spent, your hours of agent work against your
  own hands-on time, the models you lean on, and a 26-week activity heatmap.
  Every figure keeps its confidence (§28). All of it comes from the local
  analytics the product already computes.
- **Prompts as a first-class post.** You can publish a prompt from your
  notebook. Anyone can save it straight into their own notebook, and the post
  counts how many people did. A community prompt library grows out of normal
  use.
- **"Working now"** shows when someone's agents are running, in amber, the
  colour that already means agent work everywhere in the product.
- **A public web page per person** (`/u/<handle>`, `/p/<post>`) that can be
  shared outside the app. It shows only what that person made public.

## Non-negotiables, applied

- **Nothing leaves the machine unless the person publishes it.** That covers
  projects, project names, paths, sessions, transcripts and credentials. A
  prompt leaves only when someone presses Publish on it. Stats are aggregate
  numbers, never project names.
- **Opt-in, with real choices.** Creating a profile is the moment to choose.
  Stats and presence each have a visibility: Public, Followers or Private. The
  choice is shown at creation and can be changed later in Settings →
  Community.
- **Signing in is never a gate on work.** The community needs a J.A.R.V.I.S.
  account; nothing else does.
- **Metrics are information, not gamification** (D50). There are no
  leaderboards, no "keep your streak" nags and no badges.

## Decisions

1. **One identity.** A community profile belongs to the J.A.R.V.I.S. account:
   `social_profiles.user_id` references `identity_users`, and every call uses
   the identity session. The separate social account and token are retired.
   The old database was deleted, so there is nothing to migrate.
2. **Follow, not friend requests.** Following is asymmetric, like GitHub or
   X. When two people follow each other the app shows it ("follows you").
   Followers can see what is set to "Followers".
3. **Posts:** a text post (2,000 chars) or a prompt (title, prompt up to 20,000
   chars, and an optional note). Both take up to 5 tags. A reply is a post with
   `reply_to`. Reactions are likes. A save on a prompt also copies it into the
   saver's notebook on their machine. Authors can delete their own posts
   (soft delete).
4. **Feed:** two tabs. *Following* shows your posts and the people you follow.
   *Discover* shows recent posts from everyone, and can be filtered by tag.
   Pages load with a time cursor.
5. **Safety from day one:**
   - Block works both ways: the other person cannot follow, reply or like, and
     each stops seeing the other's posts.
   - Any post can be reported.
   - Limits: 20 posts per hour, 200 follows per day.
   - Every field is length- and shape-checked on the server.
   - The web pages escape everything, send a strict CSP, and never render HTML
     from a user.
6. **Notifications:** follow, like, reply and save, listed in the Community
   surface with an unread count on the rail.
7. **The desktop publishes**, from the core, while a profile exists:
   - a stats snapshot every 30 minutes, built from `analytics::report`, skipped
     when stats are private;
   - presence every 60 seconds, skipped when presence is private.

   Neither depends on a screen being open.
8. **Webview calls go through one core command.** `social_request(method,
   path, body)` attaches the identity session and allows only `/v1/social/*`.
   The server is the trust boundary, so twenty thin Rust wrappers would add
   nothing.
9. **Look:** desktop-dense, near-monochrome, colour only for meaning.
   - Amber for "working now".
   - Avatars are neutral monograms, never decorative colour.
   - Likes are a filled glyph, not a coloured one.
   - Dark and light are authored separately; en and pt-BR from the first
     string.

## Surface

`Community` replaces `Friends` on the rail.

- **Feed:** a composer at the top, then Following / Discover. The side column
  holds your profile card and people to follow.
- **Profile:** header (monogram, name, @handle, "building now" line, bio,
  location, links, joined, counts, Follow/Edit), stats strip, heatmap, and
  Posts / Prompts / Replies tabs.
- **Thread:** a post, its replies and a reply box.
- **People:** search and suggestions.
- **Notifications:** follow, like, reply, save. Named "Notifications", not
  "Activity", because the rail already has an Activity surface. An unread dot
  sits on the Community rail icon even when the surface is closed.
- **Create profile:** handle (live availability), name, bio, building, stack,
  and the two visibility choices.
- **Edit profile** (Community → Profile → Edit): every field, both
  visibilities, and deleting the community profile. (Settings has no separate
  Community page: one place to change it is enough.)
- **Notebook:** "Publish to community" on a note.

## Acceptance

1. Server: `node --test` covers validation, visibility rules and block rules.
   An e2e script against the deployment runs two accounts through: create
   profiles, follow, post, prompt, like, reply, save, notifications, block,
   the public profile with each visibility, `/u/<handle>` HTML escaping, and
   deleting a profile.
2. Desktop: unit tests for the stats snapshot mapping and for the
   `social_request` path guard.
3. App QA: two real accounts on two QA identifiers. Follow, post, share a
   prompt from the notebook, save it into the other notebook, see the
   notification, the profile with stats and heatmap, and the public web page.
   Both themes, both languages, pixels sampled.
4. `pnpm verify` green.

## Verified

- Server: `node --test` 12 pass (profile, post and stats validation,
  visibility, page escaping); `scripts/e2e-community.mjs` drives two accounts
  through profiles, a taken handle, visibility of stats and presence, follow,
  feed, tags, posts, prompts, likes, replies, saves, notifications and read
  state, search, the public pages (escaped, no followers-only stats), blocks
  (a blocked person gets 404) and deletion. It passes against
  `https://social-api-production-94d5.up.railway.app`.
- Desktop: `cargo test --lib` 730 pass, including the `social_request` path
  guard and "the snapshot never carries a project name or path".
- **The app** (`dev.jarvis.desktop.m28qa`; screenshots in `.tmp/m28-qa/`), with
  a second person driven through the real server:
  - created a profile through the form (live "Available"), both visibilities
    on screen;
  - Discover showed the other person's post and prompt; liked it, saved the
    prompt (it landed in the notebook with credit and a link), followed them;
  - their profile showed "Working now · 3 sessions" in amber (sampled
    `#CE7821`) with an amber ring, "How they build", models and the heatmap
    with month labels and a legend;
  - a notebook note went to the composer through "Publish to community" and
    was published as a prompt; a text post too;
  - when the other person followed, liked, saved and replied, the rail dot
    and a "4" appeared, and Notifications listed all four; the reply opened
    with the original post above it;
  - dark and light, pt-BR and en; the public page rendered in both schemes
    (page `#F7F7F5`, card `#FFFFFF` light).
  - The QA profiles were deleted afterwards. They briefly published this
    machine's real aggregate stats, read by the analytics backfill.
- `pnpm verify` green; lint at the 51-warning baseline.

## Found by running it

- Leftover profiles from interrupted e2e runs showed up as suggestions; the
  script deletes its profiles at the end, and the leftovers were removed.
- Stats were "waiting" right after creating a profile: the profile opened
  before the first snapshot landed. Creation now awaits it.
- A reply notification opened the reply alone; the thread now shows what it
  answers.
- "1 followers", "1 likes": singular forms (`_one`) in the app and on the
  public page.
- Saving your own prompt into your own notebook was offered; it no longer is.

## Not in scope, still

- A moderation queue for reports (they are stored, `community_reports`).
- Images and avatars uploads; monograms by design.
- Postgres integration tests in CI; the e2e script runs against a deployment.
