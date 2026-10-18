/** M28: the community's tables. Idempotent; runs on every boot. */
import type { Pool } from "pg";

export async function migrateCommunity(pool: Pool) {
  await pool.query(`
    CREATE TABLE IF NOT EXISTS community_profiles (
      user_id UUID PRIMARY KEY REFERENCES identity_users(id) ON DELETE CASCADE,
      handle TEXT UNIQUE NOT NULL CHECK (handle ~ '^[a-z0-9_]{3,24}$'),
      display_name TEXT NOT NULL, bio TEXT NOT NULL DEFAULT '', location TEXT NOT NULL DEFAULT '',
      website TEXT NOT NULL DEFAULT '', github TEXT NOT NULL DEFAULT '', stack TEXT[] NOT NULL DEFAULT '{}',
      building TEXT NOT NULL DEFAULT '',
      stats_visibility TEXT NOT NULL DEFAULT 'private' CHECK (stats_visibility IN ('public','followers','private')),
      presence_visibility TEXT NOT NULL DEFAULT 'private' CHECK (presence_visibility IN ('public','followers','private')),
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(), updated_at TIMESTAMPTZ NOT NULL DEFAULT now());
    CREATE TABLE IF NOT EXISTS community_follows (
      follower_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      followee_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY (follower_id, followee_id),
      CHECK (follower_id <> followee_id));
    CREATE INDEX IF NOT EXISTS community_follows_followee ON community_follows(followee_id);
    CREATE TABLE IF NOT EXISTS community_blocks (
      blocker_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      blocked_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY (blocker_id, blocked_id));
    CREATE TABLE IF NOT EXISTS community_posts (
      id UUID PRIMARY KEY, author_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      kind TEXT NOT NULL CHECK (kind IN ('post','prompt')), body TEXT NOT NULL DEFAULT '',
      title TEXT NOT NULL DEFAULT '', prompt TEXT NOT NULL DEFAULT '', tags TEXT[] NOT NULL DEFAULT '{}',
      reply_to UUID REFERENCES community_posts(id) ON DELETE SET NULL,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(), deleted_at TIMESTAMPTZ);
    CREATE INDEX IF NOT EXISTS community_posts_recent ON community_posts(created_at DESC) WHERE deleted_at IS NULL;
    CREATE INDEX IF NOT EXISTS community_posts_author ON community_posts(author_id, created_at DESC);
    CREATE INDEX IF NOT EXISTS community_posts_reply ON community_posts(reply_to);
    CREATE INDEX IF NOT EXISTS community_posts_tags ON community_posts USING GIN (tags);
    CREATE TABLE IF NOT EXISTS community_likes (
      post_id UUID NOT NULL REFERENCES community_posts(id) ON DELETE CASCADE,
      user_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY (post_id, user_id));
    CREATE TABLE IF NOT EXISTS community_saves (
      post_id UUID NOT NULL REFERENCES community_posts(id) ON DELETE CASCADE,
      user_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY (post_id, user_id));
    CREATE TABLE IF NOT EXISTS community_notifications (
      id BIGSERIAL PRIMARY KEY, user_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      kind TEXT NOT NULL CHECK (kind IN ('follow','like','reply','save')),
      actor_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      post_id UUID REFERENCES community_posts(id) ON DELETE CASCADE,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(), read_at TIMESTAMPTZ);
    CREATE INDEX IF NOT EXISTS community_notifications_user ON community_notifications(user_id, created_at DESC);
    CREATE TABLE IF NOT EXISTS community_stats (
      user_id UUID PRIMARY KEY REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      snapshot JSONB NOT NULL, observed_at TIMESTAMPTZ NOT NULL DEFAULT now());
    CREATE TABLE IF NOT EXISTS community_presence (
      user_id UUID PRIMARY KEY REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      active_sessions INTEGER NOT NULL CHECK (active_sessions BETWEEN 0 AND 100), last_seen_at TIMESTAMPTZ NOT NULL);
    CREATE TABLE IF NOT EXISTS community_reports (
      id BIGSERIAL PRIMARY KEY, reporter_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      post_id UUID NOT NULL REFERENCES community_posts(id) ON DELETE CASCADE,
      reason TEXT NOT NULL DEFAULT '', created_at TIMESTAMPTZ NOT NULL DEFAULT now());`);
}
