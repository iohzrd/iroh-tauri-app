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
    pub limit: u32,
    /// Post IDs the requester already has for this author.
    /// The responder returns only posts not in this list.
    #[serde(default)]
    pub known_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncResponse {
    pub posts: Vec<Post>,
    #[serde(default)]
    pub profile: Option<Profile>,
    pub total_count: u64,
    #[serde(default)]
    pub interactions: Vec<Interaction>,
}
