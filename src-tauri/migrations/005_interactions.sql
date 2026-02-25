CREATE TABLE IF NOT EXISTS interactions (
    id TEXT NOT NULL,
    author TEXT NOT NULL,
    kind TEXT NOT NULL,
    target_post_id TEXT NOT NULL,
    target_author TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    PRIMARY KEY (author, id)
);

CREATE INDEX IF NOT EXISTS idx_interactions_target
    ON interactions(target_post_id, kind);

CREATE INDEX IF NOT EXISTS idx_interactions_author_timestamp
    ON interactions(author, timestamp DESC);

CREATE UNIQUE INDEX IF NOT EXISTS idx_interactions_unique
    ON interactions(author, kind, target_post_id);
