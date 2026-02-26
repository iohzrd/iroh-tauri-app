CREATE TABLE IF NOT EXISTS mutes (
    pubkey TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS blocks (
    pubkey TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL
);
