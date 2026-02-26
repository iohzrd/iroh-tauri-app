use crate::types::{Interaction, Post, Profile};
use iroh_gossip::TopicId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipMessage {
    NewPost(Post),
    DeletePost { id: String, author: String },
    ProfileUpdate(Profile),
    NewInteraction(Interaction),
    DeleteInteraction { id: String, author: String },
}

pub fn user_feed_topic(pubkey: &str) -> TopicId {
    let mut hasher = Sha256::new();
    hasher.update(b"iroh-social-feed-v1:");
    hasher.update(pubkey.as_bytes());
    TopicId::from_bytes(hasher.finalize().into())
}

pub const SYNC_ALPN: &[u8] = b"iroh-social/sync/1";

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncRequest {
    pub author: String,
    pub before: Option<u64>,
    pub after: Option<u64>,
    pub limit: u32,
    /// How many posts the requester already has for this author.
    /// When present, the responder can detect gaps and fall back to a full sync.
    #[serde(default)]
    pub local_count: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncResponse {
    pub posts: Vec<Post>,
    #[serde(default)]
    pub profile: Option<Profile>,
    pub total_count: u64,
    pub newest_ts: Option<u64>,
    pub oldest_ts: Option<u64>,
    #[serde(default)]
    pub interactions: Vec<Interaction>,
}
