use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::Path;

fn to_bytes<T: Serialize>(val: &T) -> Vec<u8> {
    postcard::to_allocvec(val).expect("serialization failed")
}

fn from_bytes<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> T {
    postcard::from_bytes(bytes).expect("deserialization failed")
}

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

const PROFILE_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("profile");
const POSTS_TABLE: TableDefinition<(u64, &str), &[u8]> = TableDefinition::new("posts");
const FOLLOWS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("follows");

#[derive(Debug)]
pub struct Storage {
    db: Database,
}

impl Storage {
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let db = Database::create(path.as_ref())?;
        Ok(Self { db })
    }

    // -- Profile --

    pub fn save_profile(&self, profile: &Profile) -> anyhow::Result<()> {
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(PROFILE_TABLE)?;
            table.insert("self", to_bytes(profile).as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    pub fn get_profile(&self) -> anyhow::Result<Option<Profile>> {
        let txn = self.db.begin_read()?;
        match txn.open_table(PROFILE_TABLE) {
            Ok(table) => match table.get("self")? {
                Some(guard) => Ok(Some(from_bytes(guard.value()))),
                None => Ok(None),
            },
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // -- Posts --

    pub fn insert_post(&self, post: &Post) -> anyhow::Result<()> {
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(POSTS_TABLE)?;
            table.insert(
                (post.timestamp, post.id.as_str()),
                to_bytes(post).as_slice(),
            )?;
        }
        txn.commit()?;
        Ok(())
    }

    pub fn get_feed(&self, limit: usize, before: Option<u64>) -> anyhow::Result<Vec<Post>> {
        let txn = self.db.begin_read()?;
        match txn.open_table(POSTS_TABLE) {
            Ok(table) => {
                let upper = before.unwrap_or(u64::MAX);
                let mut posts = Vec::new();
                for entry in table.range((0u64, "")..=(upper, "\u{10FFFF}"))?.rev() {
                    if posts.len() >= limit {
                        break;
                    }
                    let (_k, v) = entry?;
                    posts.push(from_bytes(v.value()));
                }
                Ok(posts)
            }
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(vec![]),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_posts_by_author(&self, author: &str, limit: usize) -> anyhow::Result<Vec<Post>> {
        let txn = self.db.begin_read()?;
        match txn.open_table(POSTS_TABLE) {
            Ok(table) => {
                let mut posts = Vec::new();
                for entry in table.range((0u64, "")..=(u64::MAX, "\u{10FFFF}"))?.rev() {
                    if posts.len() >= limit {
                        break;
                    }
                    let (_k, v) = entry?;
                    let post: Post = from_bytes(v.value());
                    if post.author == author {
                        posts.push(post);
                    }
                }
                Ok(posts)
            }
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(vec![]),
            Err(e) => Err(e.into()),
        }
    }

    // -- Follows --

    pub fn follow(&self, entry: &FollowEntry) -> anyhow::Result<()> {
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(FOLLOWS_TABLE)?;
            table.insert(entry.pubkey.as_str(), to_bytes(entry).as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    pub fn unfollow(&self, pubkey: &str) -> anyhow::Result<()> {
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(FOLLOWS_TABLE)?;
            table.remove(pubkey)?;
        }
        txn.commit()?;
        Ok(())
    }

    pub fn get_follows(&self) -> anyhow::Result<Vec<FollowEntry>> {
        let txn = self.db.begin_read()?;
        match txn.open_table(FOLLOWS_TABLE) {
            Ok(table) => {
                let mut follows = Vec::new();
                for entry in table.iter()? {
                    let (_k, v) = entry?;
                    follows.push(from_bytes(v.value()));
                }
                Ok(follows)
            }
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(vec![]),
            Err(e) => Err(e.into()),
        }
    }
}
