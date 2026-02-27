CREATE TABLE IF NOT EXISTS follows (
    pubkey TEXT PRIMARY KEY,
    alias TEXT,
    followed_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS followers (
    pubkey TEXT PRIMARY KEY,
    first_seen INTEGER NOT NULL,
    last_seen INTEGER NOT NULL,
    is_online INTEGER NOT NULL DEFAULT 0
);
