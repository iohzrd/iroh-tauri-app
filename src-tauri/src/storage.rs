use iroh_social_types::{
    ConversationMeta, FollowEntry, FollowerEntry, Interaction, InteractionKind, MediaAttachment,
    Post, Profile, StoredMessage,
};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostCounts {
    pub likes: u32,
    pub replies: u32,
    pub reposts: u32,
    pub liked_by_me: bool,
    pub reposted_by_me: bool,
}
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Mutex;

pub struct Storage {
    db: Mutex<Connection>,
}

impl std::fmt::Debug for Storage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Storage").finish()
    }
}

impl Storage {
    const MIGRATIONS: &'static [(&'static str, &'static str)] = &[
        ("001_initial", include_str!("../migrations/001_initial.sql")),
        (
            "002_avatar_ticket",
            include_str!("../migrations/002_avatar_ticket.sql"),
        ),
        (
            "003_direct_messages",
            include_str!("../migrations/003_direct_messages.sql"),
        ),
        (
            "004_outbox_message_id",
            include_str!("../migrations/004_outbox_message_id.sql"),
        ),
        (
            "005_interactions",
            include_str!("../migrations/005_interactions.sql"),
        ),
        (
            "006_post_replies",
            include_str!("../migrations/006_post_replies.sql"),
        ),
        (
            "007_signatures",
            include_str!("../migrations/007_signatures.sql"),
        ),
    ];

    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let conn = Connection::open(path.as_ref())?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
        )?;
        Self::run_migrations(&conn)?;
        Ok(Self {
            db: Mutex::new(conn),
        })
    }

    fn run_migrations(conn: &Connection) -> anyhow::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                name TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL
            )",
        )?;
        for (name, sql) in Self::MIGRATIONS {
            let already_applied: bool = conn.query_row(
                "SELECT COUNT(*) > 0 FROM schema_migrations WHERE name=?1",
                params![name],
                |row| row.get(0),
            )?;
            if !already_applied {
                println!("[storage] applying migration: {name}");
                conn.execute_batch(sql)?;
                conn.execute(
                    "INSERT INTO schema_migrations (name, applied_at) VALUES (?1, strftime('%s', 'now'))",
                    params![name],
                )?;
            }
        }
        Ok(())
    }

    // -- Profile --

    pub fn save_profile(&self, profile: &Profile) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO profile (key, display_name, bio, avatar_hash, avatar_ticket)
             VALUES ('self', ?1, ?2, ?3, ?4)
             ON CONFLICT(key) DO UPDATE SET display_name=?1, bio=?2, avatar_hash=?3, avatar_ticket=?4",
            params![profile.display_name, profile.bio, profile.avatar_hash, profile.avatar_ticket],
        )?;
        Ok(())
    }

    pub fn get_profile(&self) -> anyhow::Result<Option<Profile>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT display_name, bio, avatar_hash, avatar_ticket FROM profile WHERE key='self'",
        )?;
        let mut rows = stmt.query([])?;
        match rows.next()? {
            Some(row) => Ok(Some(Profile {
                display_name: row.get(0)?,
                bio: row.get(1)?,
                avatar_hash: row.get(2)?,
                avatar_ticket: row.get(3)?,
            })),
            None => Ok(None),
        }
    }

    // -- Posts --

    pub fn insert_post(&self, post: &Post) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        let media_json = serde_json::to_string(&post.media)?;
        db.execute(
            "INSERT OR IGNORE INTO posts (id, author, content, timestamp, media_json, reply_to, reply_to_author, signature)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                post.id,
                post.author,
                post.content,
                post.timestamp as i64,
                media_json,
                post.reply_to,
                post.reply_to_author,
                post.signature,
            ],
        )?;
        Ok(())
    }

    pub fn get_post_by_id(&self, id: &str) -> anyhow::Result<Option<Post>> {
        let db = self.db.lock().unwrap();
        let mut stmt =
            db.prepare("SELECT id, author, content, timestamp, media_json, reply_to, reply_to_author, signature FROM posts WHERE id=?1")?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => Ok(Some(Self::row_to_post(row)?)),
            None => Ok(None),
        }
    }

    pub fn get_feed(&self, limit: usize, before: Option<u64>) -> anyhow::Result<Vec<Post>> {
        let db = self.db.lock().unwrap();
        let mut posts = Vec::new();
        match before {
            Some(b) => {
                let mut stmt = db.prepare(
                    "SELECT id, author, content, timestamp, media_json, reply_to, reply_to_author, signature FROM posts
                     WHERE timestamp < ?1 ORDER BY timestamp DESC LIMIT ?2",
                )?;
                let mut rows = stmt.query(params![b as i64, limit as i64])?;
                while let Some(row) = rows.next()? {
                    posts.push(Self::row_to_post(row)?);
                }
            }
            None => {
                let mut stmt = db.prepare(
                    "SELECT id, author, content, timestamp, media_json, reply_to, reply_to_author, signature FROM posts
                     ORDER BY timestamp DESC LIMIT ?1",
                )?;
                let mut rows = stmt.query(params![limit as i64])?;
                while let Some(row) = rows.next()? {
                    posts.push(Self::row_to_post(row)?);
                }
            }
        }
        Ok(posts)
    }

    pub fn delete_post(&self, id: &str) -> anyhow::Result<bool> {
        let db = self.db.lock().unwrap();
        let count = db.execute("DELETE FROM posts WHERE id=?1", params![id])?;
        Ok(count > 0)
    }

    pub fn get_posts_by_author(
        &self,
        author: &str,
        limit: usize,
        before: Option<u64>,
        media_filter: Option<&str>,
    ) -> anyhow::Result<Vec<Post>> {
        let filter_clause = match media_filter {
            Some("images") => " AND media_json LIKE '%image/%'",
            Some("videos") => " AND media_json LIKE '%video/%'",
            Some("audio") => " AND media_json LIKE '%audio/%'",
            Some("files") => {
                " AND media_json != '[]' AND media_json NOT LIKE '%image/%' AND media_json NOT LIKE '%video/%' AND media_json NOT LIKE '%audio/%'"
            }
            Some("text") => " AND media_json = '[]'",
            _ => "",
        };

        let db = self.db.lock().unwrap();
        let mut posts = Vec::new();
        match before {
            Some(b) => {
                let sql = format!(
                    "SELECT id, author, content, timestamp, media_json, reply_to, reply_to_author, signature FROM posts
                     WHERE author=?1 AND timestamp < ?2{filter_clause} ORDER BY timestamp DESC LIMIT ?3"
                );
                let mut stmt = db.prepare(&sql)?;
                let mut rows = stmt.query(params![author, b as i64, limit as i64])?;
                while let Some(row) = rows.next()? {
                    posts.push(Self::row_to_post(row)?);
                }
            }
            None => {
                let sql = format!(
                    "SELECT id, author, content, timestamp, media_json, reply_to, reply_to_author, signature FROM posts
                     WHERE author=?1{filter_clause} ORDER BY timestamp DESC LIMIT ?2"
                );
                let mut stmt = db.prepare(&sql)?;
                let mut rows = stmt.query(params![author, limit as i64])?;
                while let Some(row) = rows.next()? {
                    posts.push(Self::row_to_post(row)?);
                }
            }
        }
        Ok(posts)
    }

    pub fn get_post_ids_by_author(&self, author: &str) -> anyhow::Result<Vec<String>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT id FROM posts WHERE author=?1 ORDER BY timestamp ASC")?;
        let mut rows = stmt.query(params![author])?;
        let mut ids = Vec::new();
        while let Some(row) = rows.next()? {
            ids.push(row.get(0)?);
        }
        Ok(ids)
    }

    pub fn count_posts_by_author(&self, author: &str) -> anyhow::Result<u64> {
        let db = self.db.lock().unwrap();
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM posts WHERE author=?1",
            params![author],
            |row| row.get(0),
        )?;
        Ok(count as u64)
    }

    pub fn count_interactions_by_author(&self, author: &str) -> anyhow::Result<u64> {
        let db = self.db.lock().unwrap();
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM interactions WHERE author=?1",
            params![author],
            |row| row.get(0),
        )?;
        Ok(count as u64)
    }

    pub fn newest_interaction_timestamp(&self, author: &str) -> anyhow::Result<u64> {
        let db = self.db.lock().unwrap();
        let ts: Option<i64> = db.query_row(
            "SELECT MAX(timestamp) FROM interactions WHERE author=?1",
            params![author],
            |row| row.get(0),
        )?;
        Ok(ts.unwrap_or(0) as u64)
    }

    pub fn count_interactions_after(&self, author: &str, after_ts: u64) -> anyhow::Result<u64> {
        let db = self.db.lock().unwrap();
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM interactions WHERE author=?1 AND timestamp > ?2",
            params![author, after_ts as i64],
            |row| row.get(0),
        )?;
        Ok(count as u64)
    }

    /// Get interactions by author with timestamp > after_ts, paginated.
    /// Returns interactions in ascending timestamp order for streaming.
    pub fn get_interactions_after(
        &self,
        author: &str,
        after_ts: u64,
        limit: usize,
        offset: usize,
    ) -> anyhow::Result<Vec<Interaction>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, author, kind, target_post_id, target_author, timestamp, signature
             FROM interactions WHERE author=?1 AND timestamp > ?2
             ORDER BY timestamp ASC LIMIT ?3 OFFSET ?4",
        )?;
        let mut rows = stmt.query(params![
            author,
            after_ts as i64,
            limit as i64,
            offset as i64
        ])?;
        let mut interactions = Vec::new();
        while let Some(row) = rows.next()? {
            interactions.push(Self::row_to_interaction(row)?);
        }
        Ok(interactions)
    }

    pub fn newest_post_timestamp(&self, author: &str) -> anyhow::Result<u64> {
        let db = self.db.lock().unwrap();
        let ts: Option<i64> = db.query_row(
            "SELECT MAX(timestamp) FROM posts WHERE author=?1",
            params![author],
            |row| row.get(0),
        )?;
        Ok(ts.unwrap_or(0) as u64)
    }

    pub fn count_posts_after(&self, author: &str, after_ts: u64) -> anyhow::Result<u64> {
        let db = self.db.lock().unwrap();
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM posts WHERE author=?1 AND timestamp > ?2",
            params![author, after_ts as i64],
            |row| row.get(0),
        )?;
        Ok(count as u64)
    }

    /// Get posts by author with timestamp > after_ts, paginated by LIMIT/OFFSET.
    /// Returns posts in ascending timestamp order for streaming.
    pub fn get_posts_after(
        &self,
        author: &str,
        after_ts: u64,
        limit: usize,
        offset: usize,
    ) -> anyhow::Result<Vec<Post>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, author, content, timestamp, media_json, reply_to, reply_to_author, signature
             FROM posts WHERE author=?1 AND timestamp > ?2
             ORDER BY timestamp ASC LIMIT ?3 OFFSET ?4",
        )?;
        let mut rows = stmt.query(params![
            author,
            after_ts as i64,
            limit as i64,
            offset as i64
        ])?;
        let mut posts = Vec::new();
        while let Some(row) = rows.next()? {
            posts.push(Self::row_to_post(row)?);
        }
        Ok(posts)
    }

    /// Get posts by author whose IDs are NOT in the given set, paginated.
    pub fn get_posts_not_in(
        &self,
        author: &str,
        known_ids: &[String],
        limit: usize,
        offset: usize,
    ) -> anyhow::Result<Vec<Post>> {
        let db = self.db.lock().unwrap();
        if known_ids.is_empty() {
            // No known IDs = return all
            let mut stmt = db.prepare(
                "SELECT id, author, content, timestamp, media_json, reply_to, reply_to_author, signature
                 FROM posts WHERE author=?1
                 ORDER BY timestamp ASC LIMIT ?2 OFFSET ?3",
            )?;
            let mut rows = stmt.query(params![author, limit as i64, offset as i64])?;
            let mut posts = Vec::new();
            while let Some(row) = rows.next()? {
                posts.push(Self::row_to_post(row)?);
            }
            return Ok(posts);
        }

        // Build temp table for efficient NOT IN filtering
        db.execute_batch("CREATE TEMP TABLE IF NOT EXISTS _sync_known_ids (id TEXT PRIMARY KEY)")?;
        db.execute_batch("DELETE FROM _sync_known_ids")?;

        let mut insert = db.prepare("INSERT OR IGNORE INTO _sync_known_ids (id) VALUES (?1)")?;
        for id in known_ids {
            insert.execute(params![id])?;
        }
        drop(insert);

        let mut stmt = db.prepare(
            "SELECT p.id, p.author, p.content, p.timestamp, p.media_json, p.reply_to, p.reply_to_author, p.signature
             FROM posts p
             WHERE p.author=?1 AND p.id NOT IN (SELECT id FROM _sync_known_ids)
             ORDER BY p.timestamp ASC LIMIT ?2 OFFSET ?3",
        )?;
        let mut rows = stmt.query(params![author, limit as i64, offset as i64])?;
        let mut posts = Vec::new();
        while let Some(row) = rows.next()? {
            posts.push(Self::row_to_post(row)?);
        }

        db.execute_batch("DROP TABLE IF EXISTS _sync_known_ids")?;
        Ok(posts)
    }

    /// Get all interactions by author, paginated ascending.
    pub fn get_interactions_paged(
        &self,
        author: &str,
        limit: usize,
        offset: usize,
    ) -> anyhow::Result<Vec<Interaction>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, author, kind, target_post_id, target_author, timestamp, signature
             FROM interactions WHERE author=?1
             ORDER BY timestamp ASC LIMIT ?2 OFFSET ?3",
        )?;
        let mut rows = stmt.query(params![author, limit as i64, offset as i64])?;
        let mut interactions = Vec::new();
        while let Some(row) = rows.next()? {
            interactions.push(Self::row_to_interaction(row)?);
        }
        Ok(interactions)
    }

    fn row_to_post(row: &rusqlite::Row) -> anyhow::Result<Post> {
        let media_json: String = row.get(4)?;
        let media: Vec<MediaAttachment> = serde_json::from_str(&media_json)?;
        Ok(Post {
            id: row.get(0)?,
            author: row.get(1)?,
            content: row.get(2)?,
            timestamp: row.get::<_, i64>(3)? as u64,
            media,
            reply_to: row.get(5)?,
            reply_to_author: row.get(6)?,
            signature: row.get(7)?,
        })
    }

    // -- Interactions (likes/reposts) --

    pub fn save_interaction(&self, interaction: &Interaction) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        let kind_str = match interaction.kind {
            InteractionKind::Like => "Like",
            InteractionKind::Repost => "Repost",
        };
        db.execute(
            "INSERT OR IGNORE INTO interactions (id, author, kind, target_post_id, target_author, timestamp, signature)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                interaction.id,
                interaction.author,
                kind_str,
                interaction.target_post_id,
                interaction.target_author,
                interaction.timestamp as i64,
                interaction.signature,
            ],
        )?;
        Ok(())
    }

    pub fn delete_interaction(&self, id: &str, author: &str) -> anyhow::Result<bool> {
        let db = self.db.lock().unwrap();
        let count = db.execute(
            "DELETE FROM interactions WHERE id=?1 AND author=?2",
            params![id, author],
        )?;
        Ok(count > 0)
    }

    /// Delete a like/repost by target, returning the interaction ID if found.
    pub fn delete_interaction_by_target(
        &self,
        author: &str,
        kind: &str,
        target_post_id: &str,
    ) -> anyhow::Result<Option<String>> {
        let db = self.db.lock().unwrap();
        let id: Option<String> = db
            .query_row(
                "SELECT id FROM interactions WHERE author=?1 AND kind=?2 AND target_post_id=?3",
                params![author, kind, target_post_id],
                |row| row.get(0),
            )
            .ok();
        if let Some(ref id) = id {
            db.execute(
                "DELETE FROM interactions WHERE id=?1 AND author=?2",
                params![id, author],
            )?;
        }
        Ok(id)
    }

    pub fn get_post_counts(
        &self,
        my_pubkey: &str,
        target_post_id: &str,
    ) -> anyhow::Result<PostCounts> {
        let db = self.db.lock().unwrap();
        let likes: i64 = db.query_row(
            "SELECT COUNT(*) FROM interactions WHERE target_post_id=?1 AND kind='Like'",
            params![target_post_id],
            |row| row.get(0),
        )?;
        let reposts: i64 = db.query_row(
            "SELECT COUNT(*) FROM interactions WHERE target_post_id=?1 AND kind='Repost'",
            params![target_post_id],
            |row| row.get(0),
        )?;
        let replies: i64 = db.query_row(
            "SELECT COUNT(*) FROM posts WHERE reply_to=?1",
            params![target_post_id],
            |row| row.get(0),
        )?;
        let liked_by_me: bool = db.query_row(
            "SELECT COUNT(*) > 0 FROM interactions WHERE author=?1 AND kind='Like' AND target_post_id=?2",
            params![my_pubkey, target_post_id],
            |row| row.get(0),
        )?;
        let reposted_by_me: bool = db.query_row(
            "SELECT COUNT(*) > 0 FROM interactions WHERE author=?1 AND kind='Repost' AND target_post_id=?2",
            params![my_pubkey, target_post_id],
            |row| row.get(0),
        )?;
        Ok(PostCounts {
            likes: likes as u32,
            replies: replies as u32,
            reposts: reposts as u32,
            liked_by_me,
            reposted_by_me,
        })
    }

    pub fn get_interactions_by_author(
        &self,
        author: &str,
        limit: usize,
        before: Option<u64>,
    ) -> anyhow::Result<Vec<Interaction>> {
        let db = self.db.lock().unwrap();
        let mut interactions = Vec::new();
        match before {
            Some(b) => {
                let mut stmt = db.prepare(
                    "SELECT id, author, kind, target_post_id, target_author, timestamp, signature
                     FROM interactions WHERE author=?1 AND timestamp < ?2
                     ORDER BY timestamp DESC LIMIT ?3",
                )?;
                let mut rows = stmt.query(params![author, b as i64, limit as i64])?;
                while let Some(row) = rows.next()? {
                    interactions.push(Self::row_to_interaction(row)?);
                }
            }
            None => {
                let mut stmt = db.prepare(
                    "SELECT id, author, kind, target_post_id, target_author, timestamp, signature
                     FROM interactions WHERE author=?1
                     ORDER BY timestamp DESC LIMIT ?2",
                )?;
                let mut rows = stmt.query(params![author, limit as i64])?;
                while let Some(row) = rows.next()? {
                    interactions.push(Self::row_to_interaction(row)?);
                }
            }
        }
        Ok(interactions)
    }

    fn row_to_interaction(row: &rusqlite::Row) -> anyhow::Result<Interaction> {
        let kind_str: String = row.get(2)?;
        let kind = match kind_str.to_lowercase().as_str() {
            "like" => InteractionKind::Like,
            "repost" => InteractionKind::Repost,
            other => anyhow::bail!("unknown interaction kind: {other}"),
        };
        Ok(Interaction {
            id: row.get(0)?,
            author: row.get(1)?,
            kind,
            target_post_id: row.get(3)?,
            target_author: row.get(4)?,
            timestamp: row.get::<_, i64>(5)? as u64,
            signature: row.get(6)?,
        })
    }

    // -- Replies --

    pub fn get_replies(
        &self,
        parent_post_id: &str,
        limit: usize,
        before: Option<u64>,
    ) -> anyhow::Result<Vec<Post>> {
        let db = self.db.lock().unwrap();
        let mut posts = Vec::new();
        match before {
            Some(b) => {
                let mut stmt = db.prepare(
                    "SELECT id, author, content, timestamp, media_json, reply_to, reply_to_author, signature FROM posts
                     WHERE reply_to=?1 AND timestamp < ?2 ORDER BY timestamp ASC LIMIT ?3",
                )?;
                let mut rows = stmt.query(params![parent_post_id, b as i64, limit as i64])?;
                while let Some(row) = rows.next()? {
                    posts.push(Self::row_to_post(row)?);
                }
            }
            None => {
                let mut stmt = db.prepare(
                    "SELECT id, author, content, timestamp, media_json, reply_to, reply_to_author, signature FROM posts
                     WHERE reply_to=?1 ORDER BY timestamp ASC LIMIT ?2",
                )?;
                let mut rows = stmt.query(params![parent_post_id, limit as i64])?;
                while let Some(row) = rows.next()? {
                    posts.push(Self::row_to_post(row)?);
                }
            }
        }
        Ok(posts)
    }

    pub fn count_replies(&self, parent_post_id: &str) -> anyhow::Result<u64> {
        let db = self.db.lock().unwrap();
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM posts WHERE reply_to=?1",
            params![parent_post_id],
            |row| row.get(0),
        )?;
        Ok(count as u64)
    }

    // -- Remote Profiles --

    pub fn save_remote_profile(&self, pubkey: &str, profile: &Profile) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO remote_profiles (pubkey, display_name, bio, avatar_hash, avatar_ticket)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(pubkey) DO UPDATE SET display_name=?2, bio=?3, avatar_hash=?4, avatar_ticket=?5",
            params![
                pubkey,
                profile.display_name,
                profile.bio,
                profile.avatar_hash,
                profile.avatar_ticket
            ],
        )?;
        Ok(())
    }

    pub fn get_remote_profile(&self, pubkey: &str) -> anyhow::Result<Option<Profile>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT display_name, bio, avatar_hash, avatar_ticket FROM remote_profiles WHERE pubkey=?1",
        )?;
        let mut rows = stmt.query(params![pubkey])?;
        match rows.next()? {
            Some(row) => Ok(Some(Profile {
                display_name: row.get(0)?,
                bio: row.get(1)?,
                avatar_hash: row.get(2)?,
                avatar_ticket: row.get(3)?,
            })),
            None => Ok(None),
        }
    }

    // -- Follows --

    pub fn follow(&self, entry: &FollowEntry) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO follows (pubkey, alias, followed_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(pubkey) DO UPDATE SET alias=?2, followed_at=?3",
            params![entry.pubkey, entry.alias, entry.followed_at as i64],
        )?;
        Ok(())
    }

    pub fn update_follow_alias(&self, pubkey: &str, alias: Option<&str>) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE follows SET alias=?2 WHERE pubkey=?1",
            params![pubkey, alias],
        )?;
        Ok(())
    }

    pub fn unfollow(&self, pubkey: &str) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute("DELETE FROM follows WHERE pubkey=?1", params![pubkey])?;
        Ok(())
    }

    pub fn get_follows(&self) -> anyhow::Result<Vec<FollowEntry>> {
        let db = self.db.lock().unwrap();
        let mut stmt =
            db.prepare("SELECT pubkey, alias, followed_at FROM follows ORDER BY followed_at DESC")?;
        let mut rows = stmt.query([])?;
        let mut follows = Vec::new();
        while let Some(row) = rows.next()? {
            follows.push(FollowEntry {
                pubkey: row.get(0)?,
                alias: row.get(1)?,
                followed_at: row.get::<_, i64>(2)? as u64,
            });
        }
        Ok(follows)
    }

    // -- Followers (people following us) --

    pub fn upsert_follower(&self, pubkey: &str, now: u64) -> anyhow::Result<bool> {
        let db = self.db.lock().unwrap();
        let existing: bool = db.query_row(
            "SELECT COUNT(*) > 0 FROM followers WHERE pubkey=?1",
            params![pubkey],
            |row| row.get(0),
        )?;
        db.execute(
            "INSERT INTO followers (pubkey, first_seen, last_seen, is_online)
             VALUES (?1, ?2, ?2, 1)
             ON CONFLICT(pubkey) DO UPDATE SET last_seen=?2, is_online=1",
            params![pubkey, now as i64],
        )?;
        Ok(!existing)
    }

    pub fn set_follower_offline(&self, pubkey: &str) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE followers SET is_online=0 WHERE pubkey=?1",
            params![pubkey],
        )?;
        Ok(())
    }

    pub fn get_followers(&self) -> anyhow::Result<Vec<FollowerEntry>> {
        let db = self.db.lock().unwrap();
        let mut stmt =
            db.prepare("SELECT pubkey, first_seen, last_seen, is_online FROM followers ORDER BY last_seen DESC")?;
        let mut rows = stmt.query([])?;
        let mut followers = Vec::new();
        while let Some(row) = rows.next()? {
            followers.push(FollowerEntry {
                pubkey: row.get(0)?,
                first_seen: row.get::<_, i64>(1)? as u64,
                last_seen: row.get::<_, i64>(2)? as u64,
                is_online: row.get::<_, i32>(3)? != 0,
            });
        }
        Ok(followers)
    }

    // -- DM Conversations --

    pub fn conversation_id(pubkey_a: &str, pubkey_b: &str) -> String {
        let mut keys = [pubkey_a, pubkey_b];
        keys.sort();
        let mut hasher = Sha256::new();
        hasher.update(b"iroh-social-dm-v1:");
        hasher.update(keys[0].as_bytes());
        hasher.update(b":");
        hasher.update(keys[1].as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub fn upsert_conversation(
        &self,
        peer_pubkey: &str,
        my_pubkey: &str,
        last_message_at: u64,
        preview: &str,
    ) -> anyhow::Result<()> {
        let conv_id = Self::conversation_id(my_pubkey, peer_pubkey);
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO dm_conversations (conversation_id, peer_pubkey, last_message_at, last_message_preview, created_at)
             VALUES (?1, ?2, ?3, ?4, ?3)
             ON CONFLICT(conversation_id) DO UPDATE SET last_message_at=?3, last_message_preview=?4",
            params![conv_id, peer_pubkey, last_message_at as i64, preview],
        )?;
        Ok(())
    }

    pub fn get_conversations(&self) -> anyhow::Result<Vec<ConversationMeta>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT peer_pubkey, last_message_at, last_message_preview, unread_count
             FROM dm_conversations ORDER BY last_message_at DESC",
        )?;
        let mut rows = stmt.query([])?;
        let mut convos = Vec::new();
        while let Some(row) = rows.next()? {
            convos.push(ConversationMeta {
                peer_pubkey: row.get(0)?,
                last_message_at: row.get::<_, i64>(1)? as u64,
                last_message_preview: row.get(2)?,
                unread_count: row.get::<_, i32>(3)? as u32,
            });
        }
        Ok(convos)
    }

    pub fn increment_unread(&self, conversation_id: &str) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE dm_conversations SET unread_count = unread_count + 1 WHERE conversation_id=?1",
            params![conversation_id],
        )?;
        Ok(())
    }

    pub fn mark_conversation_read(&self, peer_pubkey: &str, my_pubkey: &str) -> anyhow::Result<()> {
        let conv_id = Self::conversation_id(my_pubkey, peer_pubkey);
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE dm_conversations SET unread_count = 0 WHERE conversation_id=?1",
            params![conv_id],
        )?;
        db.execute(
            "UPDATE dm_messages SET read = 1 WHERE conversation_id=?1 AND read = 0",
            params![conv_id],
        )?;
        Ok(())
    }

    // -- DM Messages --

    pub fn insert_dm_message(&self, msg: &StoredMessage) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        let media_json = serde_json::to_string(&msg.media)?;
        db.execute(
            "INSERT OR IGNORE INTO dm_messages (id, conversation_id, from_pubkey, to_pubkey, content, timestamp, media_json, read, delivered, reply_to)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                msg.id,
                msg.conversation_id,
                msg.from_pubkey,
                msg.to_pubkey,
                msg.content,
                msg.timestamp as i64,
                media_json,
                msg.read as i32,
                msg.delivered as i32,
                msg.reply_to,
            ],
        )?;
        Ok(())
    }

    pub fn get_dm_messages(
        &self,
        conversation_id: &str,
        limit: usize,
        before: Option<u64>,
    ) -> anyhow::Result<Vec<StoredMessage>> {
        let db = self.db.lock().unwrap();
        let mut messages = Vec::new();
        match before {
            Some(b) => {
                let mut stmt = db.prepare(
                    "SELECT id, conversation_id, from_pubkey, to_pubkey, content, timestamp, media_json, read, delivered, reply_to
                     FROM dm_messages WHERE conversation_id=?1 AND timestamp < ?2
                     ORDER BY timestamp DESC LIMIT ?3",
                )?;
                let mut rows = stmt.query(params![conversation_id, b as i64, limit as i64])?;
                while let Some(row) = rows.next()? {
                    messages.push(Self::row_to_stored_message(row)?);
                }
            }
            None => {
                let mut stmt = db.prepare(
                    "SELECT id, conversation_id, from_pubkey, to_pubkey, content, timestamp, media_json, read, delivered, reply_to
                     FROM dm_messages WHERE conversation_id=?1
                     ORDER BY timestamp DESC LIMIT ?2",
                )?;
                let mut rows = stmt.query(params![conversation_id, limit as i64])?;
                while let Some(row) = rows.next()? {
                    messages.push(Self::row_to_stored_message(row)?);
                }
            }
        }
        // Reverse so messages are in chronological order
        messages.reverse();
        Ok(messages)
    }

    fn row_to_stored_message(row: &rusqlite::Row) -> anyhow::Result<StoredMessage> {
        let media_json: String = row.get(6)?;
        let media: Vec<MediaAttachment> = serde_json::from_str(&media_json)?;
        Ok(StoredMessage {
            id: row.get(0)?,
            conversation_id: row.get(1)?,
            from_pubkey: row.get(2)?,
            to_pubkey: row.get(3)?,
            content: row.get(4)?,
            timestamp: row.get::<_, i64>(5)? as u64,
            media,
            read: row.get::<_, i32>(7)? != 0,
            delivered: row.get::<_, i32>(8)? != 0,
            reply_to: row.get(9)?,
        })
    }

    pub fn mark_dm_delivered(&self, message_id: &str) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE dm_messages SET delivered = 1 WHERE id=?1",
            params![message_id],
        )?;
        Ok(())
    }

    pub fn mark_dm_read_by_id(&self, message_id: &str) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE dm_messages SET read = 1 WHERE id=?1",
            params![message_id],
        )?;
        Ok(())
    }

    pub fn delete_dm_message(&self, message_id: &str) -> anyhow::Result<bool> {
        let db = self.db.lock().unwrap();
        let count = db.execute("DELETE FROM dm_messages WHERE id=?1", params![message_id])?;
        Ok(count > 0)
    }

    pub fn get_total_unread_count(&self) -> anyhow::Result<u32> {
        let db = self.db.lock().unwrap();
        let count: i64 = db.query_row(
            "SELECT COALESCE(SUM(unread_count), 0) FROM dm_conversations",
            [],
            |row| row.get(0),
        )?;
        Ok(count as u32)
    }

    // -- DM Outbox --

    pub fn insert_outbox_message(
        &self,
        id: &str,
        peer_pubkey: &str,
        envelope_json: &str,
        created_at: u64,
        message_id: &str,
    ) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO dm_outbox (id, peer_pubkey, envelope_json, created_at, message_id)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id,
                peer_pubkey,
                envelope_json,
                created_at as i64,
                message_id
            ],
        )?;
        Ok(())
    }

    /// Returns (outbox_id, envelope_json, message_id) tuples.
    pub fn get_outbox_for_peer(
        &self,
        peer_pubkey: &str,
    ) -> anyhow::Result<Vec<(String, String, String)>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, envelope_json, message_id FROM dm_outbox WHERE peer_pubkey=?1 ORDER BY created_at ASC",
        )?;
        let mut rows = stmt.query(params![peer_pubkey])?;
        let mut entries = Vec::new();
        while let Some(row) = rows.next()? {
            entries.push((row.get(0)?, row.get(1)?, row.get(2)?));
        }
        Ok(entries)
    }

    pub fn get_all_outbox_peers(&self) -> anyhow::Result<Vec<String>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT DISTINCT peer_pubkey FROM dm_outbox")?;
        let mut rows = stmt.query([])?;
        let mut peers = Vec::new();
        while let Some(row) = rows.next()? {
            peers.push(row.get(0)?);
        }
        Ok(peers)
    }

    pub fn remove_outbox_message(&self, id: &str) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute("DELETE FROM dm_outbox WHERE id=?1", params![id])?;
        Ok(())
    }

    // -- Ratchet Sessions --

    pub fn save_ratchet_session(
        &self,
        peer_pubkey: &str,
        state_json: &str,
        updated_at: u64,
    ) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO dm_ratchet_sessions (peer_pubkey, state_json, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(peer_pubkey) DO UPDATE SET state_json=?2, updated_at=?3",
            params![peer_pubkey, state_json, updated_at as i64],
        )?;
        Ok(())
    }

    pub fn get_ratchet_session(&self, peer_pubkey: &str) -> anyhow::Result<Option<String>> {
        let db = self.db.lock().unwrap();
        let mut stmt =
            db.prepare("SELECT state_json FROM dm_ratchet_sessions WHERE peer_pubkey=?1")?;
        let mut rows = stmt.query(params![peer_pubkey])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn delete_ratchet_session(&self, peer_pubkey: &str) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "DELETE FROM dm_ratchet_sessions WHERE peer_pubkey=?1",
            params![peer_pubkey],
        )?;
        Ok(())
    }
}
