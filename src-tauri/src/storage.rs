use iroh_social_types::{FollowEntry, FollowerEntry, MediaAttachment, Post, Profile};
use rusqlite::{Connection, params};
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
            "INSERT OR IGNORE INTO posts (id, author, content, timestamp, media_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                post.id,
                post.author,
                post.content,
                post.timestamp as i64,
                media_json
            ],
        )?;
        Ok(())
    }

    pub fn get_post_by_id(&self, id: &str) -> anyhow::Result<Option<Post>> {
        let db = self.db.lock().unwrap();
        let mut stmt =
            db.prepare("SELECT id, author, content, timestamp, media_json FROM posts WHERE id=?1")?;
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
                    "SELECT id, author, content, timestamp, media_json FROM posts
                     WHERE timestamp < ?1 ORDER BY timestamp DESC LIMIT ?2",
                )?;
                let mut rows = stmt.query(params![b as i64, limit as i64])?;
                while let Some(row) = rows.next()? {
                    posts.push(Self::row_to_post(row)?);
                }
            }
            None => {
                let mut stmt = db.prepare(
                    "SELECT id, author, content, timestamp, media_json FROM posts
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
    ) -> anyhow::Result<Vec<Post>> {
        let db = self.db.lock().unwrap();
        let mut posts = Vec::new();
        match before {
            Some(b) => {
                let mut stmt = db.prepare(
                    "SELECT id, author, content, timestamp, media_json FROM posts
                     WHERE author=?1 AND timestamp < ?2 ORDER BY timestamp DESC LIMIT ?3",
                )?;
                let mut rows = stmt.query(params![author, b as i64, limit as i64])?;
                while let Some(row) = rows.next()? {
                    posts.push(Self::row_to_post(row)?);
                }
            }
            None => {
                let mut stmt = db.prepare(
                    "SELECT id, author, content, timestamp, media_json FROM posts
                     WHERE author=?1 ORDER BY timestamp DESC LIMIT ?2",
                )?;
                let mut rows = stmt.query(params![author, limit as i64])?;
                while let Some(row) = rows.next()? {
                    posts.push(Self::row_to_post(row)?);
                }
            }
        }
        Ok(posts)
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
        })
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
}
