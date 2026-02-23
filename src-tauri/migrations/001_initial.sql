CREATE TABLE IF NOT EXISTS profile (
    key TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    bio TEXT NOT NULL,
    avatar_hash TEXT
);

CREATE TABLE IF NOT EXISTS posts (
    id TEXT NOT NULL,
    author TEXT NOT NULL,
    content TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    media_json TEXT NOT NULL DEFAULT '[]',
    PRIMARY KEY (author, id)
);
CREATE INDEX IF NOT EXISTS idx_posts_timestamp ON posts(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_posts_author_timestamp ON posts(author, timestamp DESC);

CREATE TABLE IF NOT EXISTS follows (
    pubkey TEXT PRIMARY KEY,
    alias TEXT,
    followed_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS remote_profiles (
    pubkey TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    bio TEXT NOT NULL,
    avatar_hash TEXT
);

CREATE TABLE IF NOT EXISTS followers (
    pubkey TEXT PRIMARY KEY,
    first_seen INTEGER NOT NULL,
    last_seen INTEGER NOT NULL,
    is_online INTEGER NOT NULL DEFAULT 0
);
