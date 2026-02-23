use serde::{Deserialize, Serialize};

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
