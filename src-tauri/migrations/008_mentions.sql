CREATE TABLE IF NOT EXISTS post_mentions (
    post_id TEXT NOT NULL,
    mentioned_pubkey TEXT NOT NULL,
    post_author TEXT NOT NULL,
    post_timestamp INTEGER NOT NULL,
    PRIMARY KEY (post_id, mentioned_pubkey)
);
CREATE INDEX IF NOT EXISTS idx_post_mentions_pubkey_ts ON post_mentions(mentioned_pubkey, post_timestamp DESC);

CREATE TABLE IF NOT EXISTS mention_read_marker (
    key TEXT PRIMARY KEY DEFAULT 'self',
    last_read_timestamp INTEGER NOT NULL DEFAULT 0
);
INSERT OR IGNORE INTO mention_read_marker (key, last_read_timestamp) VALUES ('self', 0);
