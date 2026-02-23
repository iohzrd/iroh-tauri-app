use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub display_name: String,
    pub bio: String,
    pub avatar_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaAttachment {
    pub hash: String,
    pub ticket: String,
    pub mime_type: String,
    pub filename: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Post {
    pub id: String,
    pub author: String,
    pub content: String,
    pub timestamp: u64,
    #[serde(default)]
    pub media: Vec<MediaAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowEntry {
    pub pubkey: String,
    pub alias: Option<String>,
    pub followed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowerEntry {
    pub pubkey: String,
    pub first_seen: u64,
    pub last_seen: u64,
    pub is_online: bool,
}

pub struct Storage {
    db: Mutex<Connection>,
}

impl std::fmt::Debug for Storage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Storage").finish()
    }
}

impl Storage {
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let conn = Connection::open(path.as_ref())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(
            "
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
            ",
        )?;
        Ok(Self {
            db: Mutex::new(conn),
        })
    }

    // -- Profile --

    pub fn save_profile(&self, profile: &Profile) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO profile (key, display_name, bio, avatar_hash) VALUES ('self', ?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET display_name=?1, bio=?2, avatar_hash=?3",
            params![profile.display_name, profile.bio, profile.avatar_hash],
        )?;
        Ok(())
    }

    pub fn get_profile(&self) -> anyhow::Result<Option<Profile>> {
        let db = self.db.lock().unwrap();
        let mut stmt =
            db.prepare("SELECT display_name, bio, avatar_hash FROM profile WHERE key='self'")?;
        let mut rows = stmt.query([])?;
        match rows.next()? {
            Some(row) => Ok(Some(Profile {
                display_name: row.get(0)?,
                bio: row.get(1)?,
                avatar_hash: row.get(2)?,
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
                post.timestamp,
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
                let mut rows = stmt.query(params![b, limit])?;
                while let Some(row) = rows.next()? {
                    posts.push(Self::row_to_post(row)?);
                }
            }
            None => {
                let mut stmt = db.prepare(
                    "SELECT id, author, content, timestamp, media_json FROM posts
                     ORDER BY timestamp DESC LIMIT ?1",
                )?;
                let mut rows = stmt.query(params![limit])?;
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
                let mut rows = stmt.query(params![author, b, limit])?;
                while let Some(row) = rows.next()? {
                    posts.push(Self::row_to_post(row)?);
                }
            }
            None => {
                let mut stmt = db.prepare(
                    "SELECT id, author, content, timestamp, media_json FROM posts
                     WHERE author=?1 ORDER BY timestamp DESC LIMIT ?2",
                )?;
                let mut rows = stmt.query(params![author, limit])?;
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
            timestamp: row.get(3)?,
            media,
        })
    }

    // -- Remote Profiles --

    pub fn save_remote_profile(&self, pubkey: &str, profile: &Profile) -> anyhow::Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO remote_profiles (pubkey, display_name, bio, avatar_hash)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(pubkey) DO UPDATE SET display_name=?2, bio=?3, avatar_hash=?4",
            params![
                pubkey,
                profile.display_name,
                profile.bio,
                profile.avatar_hash
            ],
        )?;
        Ok(())
    }

    pub fn get_remote_profile(&self, pubkey: &str) -> anyhow::Result<Option<Profile>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT display_name, bio, avatar_hash FROM remote_profiles WHERE pubkey=?1",
        )?;
        let mut rows = stmt.query(params![pubkey])?;
        match rows.next()? {
            Some(row) => Ok(Some(Profile {
                display_name: row.get(0)?,
                bio: row.get(1)?,
                avatar_hash: row.get(2)?,
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
            params![entry.pubkey, entry.alias, entry.followed_at],
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
        let mut stmt = db.prepare("SELECT pubkey, alias, followed_at FROM follows")?;
        let mut rows = stmt.query([])?;
        let mut follows = Vec::new();
        while let Some(row) = rows.next()? {
            follows.push(FollowEntry {
                pubkey: row.get(0)?,
                alias: row.get(1)?,
                followed_at: row.get(2)?,
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
            params![pubkey, now],
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
            db.prepare("SELECT pubkey, first_seen, last_seen, is_online FROM followers")?;
        let mut rows = stmt.query([])?;
        let mut followers = Vec::new();
        while let Some(row) = rows.next()? {
            followers.push(FollowerEntry {
                pubkey: row.get(0)?,
                first_seen: row.get(1)?,
                last_seen: row.get(2)?,
                is_online: row.get::<_, i32>(3)? != 0,
            });
        }
        Ok(followers)
    }
}
