CREATE TABLE IF NOT EXISTS profiles (
    pubkey TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    bio TEXT NOT NULL,
    avatar_hash TEXT,
    avatar_ticket TEXT,
    is_private INTEGER NOT NULL DEFAULT 0
);
