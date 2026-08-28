import { createHash, randomBytes, randomUUID, scrypt, timingSafeEqual } from "node:crypto";
import { promisify } from "node:util";
import { createServer } from "node:http";
import { OAuth2Client } from "google-auth-library";
import { Pool } from "pg";

const pool = new Pool({ connectionString: process.env.DATABASE_URL, ssl: process.env.NODE_ENV === "production" ? { rejectUnauthorized: false } : undefined });
const port = Number(process.env.PORT ?? 3000);
const json = (res: import("node:http").ServerResponse, status: number, body: unknown) => { res.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" }); res.end(JSON.stringify(body)); };
const hash = (value: string) => createHash("sha256").update(value).digest("hex");
const token = () => randomBytes(32).toString("hex");
const googleClientId = process.env.GOOGLE_CLIENT_ID ?? "";
const googleClientSecret = process.env.GOOGLE_CLIENT_SECRET ?? "";
const publicOrigin = (process.env.PUBLIC_ORIGIN ?? "https://social-api-production-edb6.up.railway.app").replace(/\/$/, "");
const googleRedirectUri = `${publicOrigin}/v1/auth/google/callback`;
const carriedKeys = new Set([
  "appearance.theme", "appearance.locale", "terminal.fontSize", "terminal.scrollback",
  "autopilot.turnBudget", "notifications.enabled", "notifications.system", "notifications.sound",
  // The performance HUD. The desktop already called `identity_remember` for it
  // and the call died in a swallowed `identity.notCarried` -- an allowlist is
  // only closed if both halves are closed at the same time.
  "performance.hudEnabled",
]);
const MIN_PASSWORD = 8;
const MAX_ATTEMPTS = 5;
// Matches the desktop's own lockout (`identity::LOCKOUT_MS`). Short rather than
// punitive: the realistic attacker against a password is somebody guessing at a
// keyboard, and somebody holding the database does not need to guess at all.
const LOCKOUT_MS = 60_000;

// scrypt, where the desktop uses Argon2id locally. Not an oversight: `node:crypto`
// ships scrypt, and a native Argon2 binding is a build-time dependency on a
// service whose whole job is to be boring. The two hashes never meet -- the
// machine verifies its own, the server verifies this one -- so they are free to
// differ, and `identity/prefs.rs`'s boundary note still holds: a password
// crosses in a request body and is never stored as one.
const scryptAsync = promisify(scrypt) as (secret: string, salt: string, keylen: number) => Promise<Buffer>;
async function hashPassword(password: string) {
  const salt = randomBytes(16).toString("hex");
  return ["scrypt", salt, (await scryptAsync(password, salt, 64)).toString("hex")].join("$");
}
/** The same rule as `identity::looks_like_email` on the desktop, character for
 *  character: one `@`, something on each side, a dot inside the domain but not
 *  at either end, no whitespace. Written out rather than as a regex because the
 *  regex that lived here shipped with its backslashes eaten -- `[^@s]` instead
 *  of `[^@\s]` -- and quietly rejected every address containing the letter s. */
function looksLikeEmail(email: string) {
  const parts = email.split("@");
  if (parts.length !== 2) return false;
  const [local, domain] = parts;
  return (
    local.length > 0 &&
    domain.length > 0 &&
    domain.includes(".") &&
    !domain.startsWith(".") &&
    !domain.endsWith(".") &&
    !/\s/.test(email)
  );
}
async function verifyPassword(password: string, stored: string | null) {
  if (!stored) return false;
  const [scheme, salt, digest] = stored.split("$");
  if (scheme !== "scrypt" || !salt || !digest) return false;
  const expected = Buffer.from(digest, "hex");
  const actual = await scryptAsync(password, salt, expected.length);
  return expected.length === actual.length && timingSafeEqual(expected, actual);
}

async function migrate() {
  await pool.query(`CREATE TABLE IF NOT EXISTS social_accounts (
    id UUID PRIMARY KEY, handle TEXT UNIQUE NOT NULL CHECK (handle ~ '^[a-z0-9_]{3,24}$'), display_name TEXT NOT NULL CHECK (char_length(display_name) BETWEEN 1 AND 60), token_hash TEXT NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), updated_at TIMESTAMPTZ NOT NULL DEFAULT now());
    CREATE TABLE IF NOT EXISTS social_presence (account_id UUID PRIMARY KEY REFERENCES social_accounts(id) ON DELETE CASCADE, state TEXT NOT NULL CHECK (state IN ('working','idle')), last_seen_at TIMESTAMPTZ NOT NULL, active_sessions INTEGER NOT NULL DEFAULT 0 CHECK (active_sessions >= 0));
    CREATE TABLE IF NOT EXISTS social_metrics (account_id UUID PRIMARY KEY REFERENCES social_accounts(id) ON DELETE CASCADE, visibility TEXT NOT NULL DEFAULT 'friends' CHECK (visibility IN ('private','friends','public')), tokens_today BIGINT NOT NULL DEFAULT 0, tokens_week BIGINT NOT NULL DEFAULT 0, focus_minutes_week INTEGER NOT NULL DEFAULT 0, streak_days INTEGER NOT NULL DEFAULT 0, calendar JSONB NOT NULL DEFAULT '[]', observed_at TIMESTAMPTZ NOT NULL DEFAULT now());
    CREATE TABLE IF NOT EXISTS friendships (requester_id UUID REFERENCES social_accounts(id) ON DELETE CASCADE, recipient_id UUID REFERENCES social_accounts(id) ON DELETE CASCADE, status TEXT NOT NULL CHECK (status IN ('pending','accepted','declined')), created_at TIMESTAMPTZ NOT NULL DEFAULT now(), decided_at TIMESTAMPTZ, PRIMARY KEY (requester_id, recipient_id), CHECK (requester_id <> recipient_id));
    CREATE INDEX IF NOT EXISTS friendships_recipient_pending ON friendships(recipient_id, status);
    CREATE TABLE IF NOT EXISTS social_activity (id BIGSERIAL PRIMARY KEY, account_id UUID REFERENCES social_accounts(id) ON DELETE CASCADE, kind TEXT NOT NULL CHECK (kind IN ('started','updatedMetrics')), created_at TIMESTAMPTZ NOT NULL DEFAULT now());
    CREATE INDEX IF NOT EXISTS social_activity_account_time ON social_activity(account_id, created_at DESC);

    CREATE TABLE IF NOT EXISTS identity_users (
      id UUID PRIMARY KEY, google_sub TEXT UNIQUE NOT NULL, email TEXT UNIQUE NOT NULL,
      display_name TEXT NOT NULL, avatar_url TEXT, created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      updated_at TIMESTAMPTZ NOT NULL DEFAULT now(), last_signed_in_at TIMESTAMPTZ);
    CREATE TABLE IF NOT EXISTS identity_sessions (
      id UUID PRIMARY KEY, user_id UUID NOT NULL REFERENCES identity_users(id) ON DELETE CASCADE,
      token_hash TEXT UNIQUE NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      expires_at TIMESTAMPTZ NOT NULL, revoked_at TIMESTAMPTZ);
    CREATE INDEX IF NOT EXISTS identity_sessions_user ON identity_sessions(user_id, expires_at DESC);
    CREATE TABLE IF NOT EXISTS identity_oauth_flows (
      id UUID PRIMARY KEY, poll_secret_hash TEXT NOT NULL, state_hash TEXT UNIQUE NOT NULL,
      user_id UUID REFERENCES identity_users(id) ON DELETE CASCADE, status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending','complete','consumed','failed')),
      error TEXT, created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      expires_at TIMESTAMPTZ NOT NULL);
    CREATE INDEX IF NOT EXISTS identity_oauth_flows_expiry ON identity_oauth_flows(expires_at);
    CREATE TABLE IF NOT EXISTS identity_settings (
      user_id UUID NOT NULL REFERENCES identity_users(id) ON DELETE CASCADE,
      key TEXT NOT NULL, value JSONB NOT NULL, updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      PRIMARY KEY(user_id, key));
    CREATE TABLE IF NOT EXISTS identity_quota_snapshots (
      user_id UUID PRIMARY KEY REFERENCES identity_users(id) ON DELETE CASCADE,
      snapshot JSONB NOT NULL, observed_at TIMESTAMPTZ NOT NULL DEFAULT now());

    -- ---- M23: an account you can sign into without Google -------------------
    --
    -- Every statement below re-runs on every boot, because migrate() does. That
    -- is why they are all IF NOT EXISTS or idempotent ALTERs and why none of
    -- them is a bare ADD CONSTRAINT.
    --
    -- google_sub loses NOT NULL rather than gaining a placeholder: a password
    -- account genuinely has no Google subject, and a sentinel string would be a
    -- value the UNIQUE index has to keep unique.
    ALTER TABLE identity_users ALTER COLUMN google_sub DROP NOT NULL;
    ALTER TABLE identity_users ADD COLUMN IF NOT EXISTS password_hash TEXT;
    ALTER TABLE identity_users ADD COLUMN IF NOT EXISTS failed_attempts INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE identity_users ADD COLUMN IF NOT EXISTS locked_until TIMESTAMPTZ;

    -- ---- M23: the notebook, which is the thing he actually asked for --------
    --
    -- Soft delete, not DELETE. A row removed outright is a row the next pull on
    -- another machine cannot tell from one it has never seen, so it resurrects
    -- what somebody deleted. deleted_at is the tombstone, and touched_at is the
    -- clock both sides compare -- see docs/M23-CLOUD-SYNC.md for why that is a
    -- separate column from updated_at.
    CREATE TABLE IF NOT EXISTS identity_notebooks (
      user_id UUID NOT NULL REFERENCES identity_users(id) ON DELETE CASCADE,
      id TEXT NOT NULL, name TEXT NOT NULL, position BIGINT NOT NULL,
      created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL,
      touched_at BIGINT NOT NULL, deleted_at BIGINT,
      PRIMARY KEY (user_id, id));
    CREATE TABLE IF NOT EXISTS identity_notes (
      user_id UUID NOT NULL REFERENCES identity_users(id) ON DELETE CASCADE,
      id TEXT NOT NULL, notebook_id TEXT, title TEXT NOT NULL, body TEXT NOT NULL,
      pinned BOOLEAN NOT NULL DEFAULT false,
      created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL,
      touched_at BIGINT NOT NULL, deleted_at BIGINT,
      PRIMARY KEY (user_id, id));
    CREATE INDEX IF NOT EXISTS identity_notes_user ON identity_notes(user_id);`);
  await pool.query("DELETE FROM identity_oauth_flows WHERE expires_at < now() - interval '1 day'");
  await pool.query("DELETE FROM identity_sessions WHERE expires_at < now() - interval '1 day'");
}
/** The cap is per route: a whole prompt library is two orders of magnitude
 *  larger than any other request this service takes, and one shared 32KB limit
 *  meant the notebook push failed at around forty notes -- as a 500, because
 *  the throw landed in the outer catch rather than saying what was wrong. */
class TooLarge extends Error {}
async function body(req: import("node:http").IncomingMessage, limit = 32_768) { let raw=""; for await (const chunk of req) { raw += chunk; if (raw.length > limit) throw new TooLarge(); } return raw ? JSON.parse(raw) as Record<string, unknown> : {}; }
const NOTEBOOK_LIMIT = 8 * 1024 * 1024;
async function me(req: import("node:http").IncomingMessage) { const raw = req.headers.authorization?.replace(/^Bearer\s+/i, ""); if (!raw) return null; const row = await pool.query("SELECT id, handle, display_name FROM social_accounts WHERE token_hash = $1", [hash(raw)]); return row.rows[0] as {id:string;handle:string;display_name:string}|undefined; }
async function identityMe(req: import("node:http").IncomingMessage) {
  const raw = req.headers.authorization?.replace(/^Bearer\s+/i, "");
  if (!raw) return null;
  const row = await pool.query(
    `SELECT u.id,u.email,u.display_name,u.avatar_url,u.created_at,u.updated_at,u.last_signed_in_at
       FROM identity_sessions s JOIN identity_users u ON u.id=s.user_id
      WHERE s.token_hash=$1 AND s.revoked_at IS NULL AND s.expires_at>now()`, [hash(raw)]);
  return row.rows[0] as Record<string, unknown> | undefined;
}
/** Everything an account carries, in one shape, used by every entry point --
 *  Google poll, password sign-up, password sign-in and the launch pull. Four
 *  copies of this assembly is four chances for one of them to forget a field. */
async function stateFor(userId: string) {
  const settings = (await pool.query("SELECT key,value FROM identity_settings WHERE user_id=$1", [userId])).rows;
  const notebooks = (await pool.query(
    "SELECT id,name,position,created_at,updated_at,touched_at,deleted_at FROM identity_notebooks WHERE user_id=$1", [userId])).rows;
  const notes = (await pool.query(
    "SELECT id,notebook_id,title,body,pinned,created_at,updated_at,touched_at,deleted_at FROM identity_notes WHERE user_id=$1", [userId])).rows;
  return {
    settings: Object.fromEntries(settings.map((row) => [row.key, row.value])),
    notebook: {
      notebooks: notebooks.map((row) => ({ id: row.id, name: row.name, position: Number(row.position),
        createdAt: Number(row.created_at), updatedAt: Number(row.updated_at),
        touchedAt: Number(row.touched_at), deletedAt: row.deleted_at === null ? null : Number(row.deleted_at) })),
      notes: notes.map((row) => ({ id: row.id, notebookId: row.notebook_id, title: row.title, body: row.body,
        pinned: row.pinned, createdAt: Number(row.created_at), updatedAt: Number(row.updated_at),
        touchedAt: Number(row.touched_at), deletedAt: row.deleted_at === null ? null : Number(row.deleted_at) })),
    },
  };
}
async function issueSession(userId: string) {
  const sessionToken = token();
  await pool.query("INSERT INTO identity_sessions(id,user_id,token_hash,expires_at) VALUES($1,$2,$3,now()+interval '90 days')", [randomUUID(), userId, hash(sessionToken)]);
  await pool.query("UPDATE identity_users SET last_signed_in_at=now(),failed_attempts=0,locked_until=NULL WHERE id=$1", [userId]);
  const user = (await pool.query("SELECT * FROM identity_users WHERE id=$1", [userId])).rows[0];
  return { token: sessionToken, account: identityAccount(user), ...(await stateFor(userId)) };
}
function identityAccount(row: Record<string, unknown>) {
  return { id: row.id, email: row.email, displayName: row.display_name, avatarUrl: row.avatar_url,
    createdAt: new Date(String(row.created_at)).getTime(), updatedAt: new Date(String(row.updated_at)).getTime(),
    lastSignedInAt: row.last_signed_in_at ? new Date(String(row.last_signed_in_at)).getTime() : null };
}
function html(res: import("node:http").ServerResponse, status: number, title: string, message: string) {
  res.writeHead(status, { "content-type": "text/html; charset=utf-8", "cache-control": "no-store", "content-security-policy": "default-src 'none'; style-src 'unsafe-inline'" });
  res.end(`<!doctype html><meta charset="utf-8"><title>${title}</title><style>body{font:16px system-ui;background:#0c0c0d;color:#f4f4f2;display:grid;place-items:center;min-height:100vh;margin:0}main{max-width:34rem;padding:2rem}h1{font-size:1.5rem}</style><main><h1>${title}</h1><p>${message}</p></main>`);
}
function profile(row: Record<string, unknown>) { const seen=row.last_seen_at?new Date(String(row.last_seen_at)).getTime():0; const fresh=seen>0&&Date.now()-seen<120_000; return { id: row.id, handle: row.handle, displayName: row.display_name, presence: fresh ? { state: row.state, lastSeenAt: row.last_seen_at, activeSessions: row.active_sessions } : (row.last_seen_at ? { state: "idle", lastSeenAt: row.last_seen_at, activeSessions: 0 } : null), metrics: !row.visibility || row.visibility === "private" ? null : { visibility: row.visibility, tokensToday: row.tokens_today, tokensWeek: row.tokens_week, focusMinutesWeek: row.focus_minutes_week, streakDays: row.streak_days, calendar: row.calendar, observedAt: row.observed_at } }; }

await migrate();
createServer(async (req, res) => { try {
  const url = new URL(req.url ?? "/", `http://${req.headers.host}`);
  if (req.method === "GET" && url.pathname === "/health") return json(res, 200, { ok: true });
  if (req.method === "POST" && url.pathname === "/v1/auth/google/start") {
    if (!googleClientId || !googleClientSecret) return json(res, 503, { error: "identity.googleUnavailable" });
    const id=randomUUID(), pollSecret=token(), state=token();
    await pool.query("INSERT INTO identity_oauth_flows(id,poll_secret_hash,state_hash,expires_at) VALUES($1,$2,$3,now()+interval '10 minutes')",[id,hash(pollSecret),hash(state)]);
    const authUrl=new URL("https://accounts.google.com/o/oauth2/v2/auth");
    authUrl.search=new URLSearchParams({client_id:googleClientId,redirect_uri:googleRedirectUri,response_type:"code",scope:"openid email profile",state,prompt:"select_account"}).toString();
    return json(res,201,{flowId:id,pollSecret,authorizationUrl:authUrl.toString(),expiresInMs:600_000});
  }
  if (req.method === "GET" && url.pathname === "/v1/auth/google/callback") {
    const state=url.searchParams.get("state")??"", code=url.searchParams.get("code")??"", oauthError=url.searchParams.get("error");
    const flow=(await pool.query("SELECT id,status FROM identity_oauth_flows WHERE state_hash=$1 AND expires_at>now()",[hash(state)])).rows[0];
    if(!flow) return html(res,400,"Link expirado","Volte ao J.A.R.V.I.S. e tente entrar novamente.");
    if(oauthError||!code){await pool.query("UPDATE identity_oauth_flows SET status='failed',error=$2 WHERE id=$1",[flow.id,oauthError??"missing_code"]);return html(res,400,"Login cancelado","Nenhuma alteração foi feita. Você pode fechar esta aba.");}
    try {
      const client=new OAuth2Client(googleClientId,googleClientSecret,googleRedirectUri);
      const {tokens}=await client.getToken(code);
      if(!tokens.id_token) throw Error("missing_id_token");
      const ticket=await client.verifyIdToken({idToken:tokens.id_token,audience:googleClientId});
      const payload=ticket.getPayload();
      if(!payload?.sub||!payload.email||payload.email_verified!==true) throw Error("unverified_google_account");
      // Linked by e-mail as well as by subject, and this is not a nicety: once
      // password accounts exist, signing up with an address and *then* using
      // Google with the same one has no google_sub conflict to absorb, so the
      // insert would collide on the UNIQUE e-mail and take the whole flow down
      // with a 500. The rule is deliberately the same one `upsert_google` uses
      // on the desktop -- two rules for who owns an address is two answers.
      const email=payload.email.toLowerCase();
      const existing=(await pool.query(
        "SELECT id FROM identity_users WHERE google_sub=$1 OR email=$2 ORDER BY (google_sub=$1) DESC LIMIT 1",
        [payload.sub,email])).rows[0];
      const user=existing
        ? (await pool.query(
            `UPDATE identity_users SET google_sub=$2,email=$3,display_name=$4,avatar_url=$5,
               updated_at=now(),last_signed_in_at=now() WHERE id=$1 RETURNING id`,
            [existing.id,payload.sub,email,payload.name??email,payload.picture??null])).rows[0]
        : (await pool.query(
            `INSERT INTO identity_users(id,google_sub,email,display_name,avatar_url,last_signed_in_at)
             VALUES($1,$2,$3,$4,$5,now()) RETURNING id`,
            [randomUUID(),payload.sub,email,payload.name??email,payload.picture??null])).rows[0];
      await pool.query("UPDATE identity_oauth_flows SET status='complete',user_id=$2,error=NULL WHERE id=$1 AND status='pending'",[flow.id,user.id]);
      return html(res,200,"Tudo certo","Sua conta foi conectada. Pode fechar esta aba e voltar ao J.A.R.V.I.S.");
    } catch(error) { console.error(error); await pool.query("UPDATE identity_oauth_flows SET status='failed',error='google_exchange_failed' WHERE id=$1",[flow.id]); return html(res,500,"Não foi possível entrar","Volte ao J.A.R.V.I.S. e tente novamente."); }
  }
  if (req.method === "POST" && url.pathname === "/v1/auth/google/poll") {
    const b=await body(req), flowId=typeof b.flowId==="string"?b.flowId:"", pollSecret=typeof b.pollSecret==="string"?b.pollSecret:"";
    const flow=(await pool.query("SELECT status,error,expires_at FROM identity_oauth_flows WHERE id=$1 AND poll_secret_hash=$2",[flowId,hash(pollSecret)])).rows[0];
    if(!flow||new Date(flow.expires_at).getTime()<Date.now())return json(res,410,{status:"expired"});
    if(flow.status==="pending")return json(res,202,{status:"pending"});
    if(flow.status==="failed")return json(res,400,{status:"failed",error:flow.error??"identity.googleFailed"});
    const claimed=await pool.query("UPDATE identity_oauth_flows SET status='consumed' WHERE id=$1 AND status='complete' RETURNING user_id",[flowId]);
    if(!claimed.rowCount)return json(res,410,{status:"consumed"});
    return json(res,200,{status:"complete",...(await issueSession(claimed.rows[0].user_id))});
  }
  if (req.method === "POST" && url.pathname === "/v1/auth/sign-up") {
    const b=await body(req);
    const email=typeof b.email==="string"?b.email.trim().toLowerCase():"";
    const displayName=typeof b.displayName==="string"?b.displayName.trim():"";
    const password=typeof b.password==="string"?b.password:"";
    if(!displayName)return json(res,400,{status:"nameRequired"});
    if(!looksLikeEmail(email))return json(res,400,{status:"invalidEmail"});
    if([...password].length<MIN_PASSWORD)return json(res,400,{status:"passwordTooShort",minimum:MIN_PASSWORD});
    const taken=(await pool.query("SELECT id FROM identity_users WHERE email=$1",[email])).rowCount;
    if(taken)return json(res,409,{status:"emailTaken"});
    const id=randomUUID();
    try {
      await pool.query("INSERT INTO identity_users(id,email,display_name,password_hash,last_signed_in_at) VALUES($1,$2,$3,$4,now())",
        [id,email,displayName,await hashPassword(password)]);
    } catch { return json(res,409,{status:"emailTaken"}); }
    return json(res,201,{status:"ok",...(await issueSession(id))});
  }
  if (req.method === "POST" && url.pathname === "/v1/auth/sign-in") {
    const b=await body(req);
    const email=typeof b.email==="string"?b.email.trim().toLowerCase():"";
    const password=typeof b.password==="string"?b.password:"";
    const user=(await pool.query("SELECT id,password_hash,failed_attempts,locked_until FROM identity_users WHERE email=$1",[email])).rows[0];
    // One verdict for "no such address" and for "wrong password", deliberately.
    // This endpoint faces the internet, where the desktop's version faced a
    // person at their own keyboard: there, naming an unknown e-mail is a
    // kindness; here it is an oracle that says which addresses have accounts.
    if(!user||!user.password_hash)return json(res,401,{status:"wrongPassword"});
    if(user.locked_until&&new Date(user.locked_until).getTime()>Date.now())
      return json(res,429,{status:"lockedOut",retryInMs:new Date(user.locked_until).getTime()-Date.now()});
    if(!(await verifyPassword(password,user.password_hash))) {
      const attempts=Number(user.failed_attempts)+1;
      await pool.query("UPDATE identity_users SET failed_attempts=$2,locked_until=$3 WHERE id=$1",
        [user.id,attempts,attempts>=MAX_ATTEMPTS?new Date(Date.now()+LOCKOUT_MS):null]);
      return json(res,401,{status:"wrongPassword",attemptsLeft:Math.max(0,MAX_ATTEMPTS-attempts)});
    }
    return json(res,200,{status:"ok",...(await issueSession(user.id))});
  }
  if (req.method === "POST" && url.pathname === "/v1/auth/sign-out") {
    const raw=req.headers.authorization?.replace(/^Bearer\s+/i,""); if(raw)await pool.query("UPDATE identity_sessions SET revoked_at=now() WHERE token_hash=$1",[hash(raw)]); return json(res,200,{ok:true});
  }
  if (url.pathname.startsWith("/v1/sync/")) {
    const user=await identityMe(req); if(!user)return json(res,401,{error:"identity.unauthorised"});
    if(req.method==="GET"&&url.pathname==="/v1/sync/state") { const quota=(await pool.query("SELECT snapshot,observed_at FROM identity_quota_snapshots WHERE user_id=$1",[user.id])).rows[0]; return json(res,200,{account:identityAccount(user),...(await stateFor(String(user.id))),quota:quota?{snapshot:quota.snapshot,observedAt:quota.observed_at}:null}); }
    // The whole library, merged row by row rather than replaced. Replacing
    // would mean the last machine to speak owns everything, which loses a note
    // written on the other one between two pushes; last-write-wins per row by
    // touchedAt loses only the older of two edits to the *same* note.
    if(req.method==="PUT"&&url.pathname==="/v1/sync/notebook") {
      const b=await body(req,NOTEBOOK_LIMIT);
      const notebooks=Array.isArray(b.notebooks)?b.notebooks:null, notes=Array.isArray(b.notes)?b.notes:null;
      if(!notebooks||!notes||notebooks.length>1_000||notes.length>10_000)return json(res,400,{error:"identity.invalidNotebook"});
      const num=(value:unknown)=>Number.isFinite(Number(value))?Math.trunc(Number(value)):null;
      const client=await pool.connect();
      try {
        await client.query("BEGIN");
        for(const raw of notebooks as Record<string,unknown>[]) {
          const id=typeof raw.id==="string"?raw.id:"", touched=num(raw.touchedAt);
          if(!id||id.length>64||touched===null)continue;
          await client.query(
            `INSERT INTO identity_notebooks(user_id,id,name,position,created_at,updated_at,touched_at,deleted_at)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8)
             ON CONFLICT(user_id,id) DO UPDATE SET name=excluded.name,position=excluded.position,
               created_at=excluded.created_at,updated_at=excluded.updated_at,touched_at=excluded.touched_at,
               deleted_at=excluded.deleted_at
             WHERE excluded.touched_at >= identity_notebooks.touched_at`,
            [user.id,id,String(raw.name??"").slice(0,200),num(raw.position)??0,num(raw.createdAt)??touched,
             num(raw.updatedAt)??touched,touched,num(raw.deletedAt)]);
        }
        for(const raw of notes as Record<string,unknown>[]) {
          const id=typeof raw.id==="string"?raw.id:"", touched=num(raw.touchedAt);
          if(!id||id.length>64||touched===null)continue;
          await client.query(
            `INSERT INTO identity_notes(user_id,id,notebook_id,title,body,pinned,created_at,updated_at,touched_at,deleted_at)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
             ON CONFLICT(user_id,id) DO UPDATE SET notebook_id=excluded.notebook_id,title=excluded.title,
               body=excluded.body,pinned=excluded.pinned,created_at=excluded.created_at,
               updated_at=excluded.updated_at,touched_at=excluded.touched_at,deleted_at=excluded.deleted_at
             WHERE excluded.touched_at >= identity_notes.touched_at`,
            [user.id,id,typeof raw.notebookId==="string"?raw.notebookId:null,String(raw.title??"").slice(0,500),
             String(raw.body??"").slice(0,200_000),raw.pinned===true,num(raw.createdAt)??touched,
             num(raw.updatedAt)??touched,touched,num(raw.deletedAt)]);
        }
        await client.query("COMMIT");
      } catch(error){await client.query("ROLLBACK");throw error;} finally {client.release();}
      return json(res,200,{notebook:(await stateFor(String(user.id))).notebook});
    }
    if(req.method==="PUT"&&url.pathname==="/v1/sync/settings") { const b=await body(req), settings=b.settings&&typeof b.settings==="object"&&!Array.isArray(b.settings)?b.settings as Record<string,unknown>:null; if(!settings)return json(res,400,{error:"identity.invalidSettings"}); const entries=Object.entries(settings); if(entries.some(([key,value])=>!carriedKeys.has(key)||JSON.stringify(value).length>4096))return json(res,400,{error:"identity.invalidSettings"}); const client=await pool.connect(); try{await client.query("BEGIN");for(const [key,value] of entries)await client.query("INSERT INTO identity_settings(user_id,key,value) VALUES($1,$2,$3::jsonb) ON CONFLICT(user_id,key) DO UPDATE SET value=excluded.value,updated_at=now()",[user.id,key,JSON.stringify(value)]);await client.query("COMMIT");}catch(error){await client.query("ROLLBACK");throw error;}finally{client.release();} return json(res,200,{ok:true}); }
    if(req.method==="PUT"&&url.pathname==="/v1/sync/quota") { const b=await body(req); if(!b.snapshot||JSON.stringify(b.snapshot).length>28_000)return json(res,400,{error:"identity.invalidQuota"}); await pool.query("INSERT INTO identity_quota_snapshots(user_id,snapshot) VALUES($1,$2::jsonb) ON CONFLICT(user_id) DO UPDATE SET snapshot=excluded.snapshot,observed_at=now()",[user.id,JSON.stringify(b.snapshot)]); return json(res,200,{ok:true}); }
  }
  if (req.method === "POST" && url.pathname === "/v1/accounts") { const b=await body(req); const handle=typeof b.handle==="string"?b.handle.trim().toLowerCase():""; const displayName=typeof b.displayName==="string"?b.displayName.trim():""; if(!/^[a-z0-9_]{3,24}$/.test(handle)||!displayName||displayName.length>60)return json(res,400,{error:"social.invalidProfile"}); const id=randomUUID(), secret=token(); try { await pool.query("INSERT INTO social_accounts(id,handle,display_name,token_hash) VALUES($1,$2,$3,$4)",[id,handle,displayName,hash(secret)]); return json(res,201,{account:{id,handle,displayName},token:secret}); } catch { return json(res,409,{error:"social.handleTaken"}); } }
  const account=await me(req); if(!account)return json(res,401,{error:"social.unauthorised"});
  if(req.method==="POST"&&url.pathname==="/v1/heartbeat"){const b=await body(req), active=Number(b.activeSessions); if(!Number.isInteger(active)||active<0||active>100)return json(res,400,{error:"social.invalidPresence"}); await pool.query("INSERT INTO social_presence(account_id,state,last_seen_at,active_sessions) VALUES($1,$2,now(),$3) ON CONFLICT(account_id) DO UPDATE SET state=excluded.state,last_seen_at=excluded.last_seen_at,active_sessions=excluded.active_sessions",[account.id,active>0?"working":"idle",active]); return json(res,200,{ok:true});}
  if(req.method==="GET"&&url.pathname==="/v1/me") { const result=await pool.query("SELECT a.id,a.handle,a.display_name,p.state,p.last_seen_at,p.active_sessions,m.* FROM social_accounts a LEFT JOIN social_presence p ON p.account_id=a.id LEFT JOIN social_metrics m ON m.account_id=a.id WHERE a.id=$1",[account.id]); return json(res,200,{profile:profile(result.rows[0])}); }
  if(req.method==="POST"&&url.pathname==="/v1/metrics") { const b=await body(req), visibility=String(b.visibility); const numbers=[b.tokensToday,b.tokensWeek,b.focusMinutesWeek,b.streakDays].map(Number); if(!["private","friends","public"].includes(visibility)||numbers.some(n=>!Number.isInteger(n)||n<0))return json(res,400,{error:"social.invalidMetrics"}); const calendar=Array.isArray(b.calendar)?b.calendar.slice(-91):[]; await pool.query("INSERT INTO social_metrics(account_id,visibility,tokens_today,tokens_week,focus_minutes_week,streak_days,calendar,observed_at) VALUES($1,$2,$3,$4,$5,$6,$7,now()) ON CONFLICT(account_id) DO UPDATE SET visibility=excluded.visibility,tokens_today=excluded.tokens_today,tokens_week=excluded.tokens_week,focus_minutes_week=excluded.focus_minutes_week,streak_days=excluded.streak_days,calendar=excluded.calendar,observed_at=excluded.observed_at",[account.id,visibility,...numbers,JSON.stringify(calendar)]); await pool.query("INSERT INTO social_activity(account_id,kind) VALUES($1,'updatedMetrics')",[account.id]); return json(res,200,{ok:true}); }
  if(req.method==="POST"&&url.pathname.startsWith("/v1/friends/")) { const handle=url.pathname.slice("/v1/friends/".length).toLowerCase(); const target=(await pool.query("SELECT id,handle,display_name FROM social_accounts WHERE handle=$1",[handle])).rows[0]; if(!target||target.id===account.id)return json(res,404,{error:"social.profileNotFound"}); const old=(await pool.query("SELECT * FROM friendships WHERE (requester_id=$1 AND recipient_id=$2) OR (requester_id=$2 AND recipient_id=$1)",[account.id,target.id])).rows[0]; if(old?.status==="accepted")return json(res,200,{status:"accepted"}); if(old?.requester_id===target.id&&old.status==="pending"){await pool.query("UPDATE friendships SET status='accepted',decided_at=now() WHERE requester_id=$1 AND recipient_id=$2",[target.id,account.id]); return json(res,200,{status:"accepted"});} if(!old)await pool.query("INSERT INTO friendships(requester_id,recipient_id,status) VALUES($1,$2,'pending')",[account.id,target.id]); return json(res,201,{status:"pending"}); }
  if(req.method==="POST"&&url.pathname.startsWith("/v1/friend-requests/")&&url.pathname.endsWith("/accept")) { const requester=url.pathname.slice(20,-7); const changed=await pool.query("UPDATE friendships SET status='accepted',decided_at=now() WHERE requester_id=$1 AND recipient_id=$2 AND status='pending'",[requester,account.id]); return changed.rowCount?json(res,200,{status:"accepted"}):json(res,404,{error:"social.requestNotFound"}); }
  if(req.method==="GET"&&url.pathname==="/v1/friends") { const result=await pool.query("SELECT a.id,a.handle,a.display_name,p.state,p.last_seen_at,p.active_sessions,m.visibility,m.tokens_today,m.tokens_week,m.focus_minutes_week,m.streak_days,m.calendar,m.observed_at,f.status, f.requester_id FROM friendships f JOIN social_accounts a ON a.id=CASE WHEN f.requester_id=$1 THEN f.recipient_id ELSE f.requester_id END LEFT JOIN social_presence p ON p.account_id=a.id LEFT JOIN social_metrics m ON m.account_id=a.id WHERE f.requester_id=$1 OR f.recipient_id=$1 ORDER BY COALESCE(p.last_seen_at,a.updated_at) DESC",[account.id]); return json(res,200,{friends:result.rows.filter(row=>row.status==='accepted').map(profile),requests:result.rows.filter(row=>row.status==='pending'&&row.requester_id!==account.id).map(profile)}); }
  if(req.method==="GET"&&url.pathname.startsWith("/v1/profiles/")) { const handle=url.pathname.slice(13).toLowerCase(); const row=(await pool.query("SELECT a.id,a.handle,a.display_name,p.state,p.last_seen_at,p.active_sessions,m.* FROM social_accounts a LEFT JOIN social_presence p ON p.account_id=a.id LEFT JOIN social_metrics m ON m.account_id=a.id WHERE a.handle=$1",[handle])).rows[0]; if(!row)return json(res,404,{error:"social.profileNotFound"}); const linked=((await pool.query("SELECT status FROM friendships WHERE status='accepted' AND ((requester_id=$1 AND recipient_id=$2) OR (requester_id=$2 AND recipient_id=$1))",[account.id,row.id])).rowCount ?? 0)>0; if(row.visibility==='private'||(row.visibility==='friends'&&!linked))row.visibility='private'; return json(res,200,{profile:profile(row),friend:linked}); }
  return json(res,404,{error:"social.notFound"});
} catch (error) { if (error instanceof TooLarge) return json(res,413,{error:"social.tooLarge"}); console.error(error); json(res,500,{error:"social.internal"}); } }).listen(port, () => console.log(`social api on ${port}`));
