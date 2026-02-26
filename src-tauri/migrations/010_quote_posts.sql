ALTER TABLE posts ADD COLUMN quote_of TEXT;
ALTER TABLE posts ADD COLUMN quote_of_author TEXT;
CREATE INDEX idx_posts_quote_of ON posts(quote_of) WHERE quote_of IS NOT NULL;
