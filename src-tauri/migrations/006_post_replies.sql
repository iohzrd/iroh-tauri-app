ALTER TABLE posts ADD COLUMN reply_to TEXT;
ALTER TABLE posts ADD COLUMN reply_to_author TEXT;

CREATE INDEX IF NOT EXISTS idx_posts_reply_to
    ON posts(reply_to) WHERE reply_to IS NOT NULL;
