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

pub const SYNC_ALPN: &[u8] = b"iroh-social/sync/3";

/// Phase 1: Client sends summary of what it has for an author.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncRequest {
    pub author: String,
    pub post_count: u64,
    pub interaction_count: u64,
    /// Newest post timestamp the client has for this author (0 = no posts).
    pub newest_timestamp: u64,
    /// Newest interaction timestamp the client has for this author (0 = no interactions).
    pub newest_interaction_timestamp: u64,
}

/// Phase 1: Server responds with its counts and whether timestamp catch-up suffices.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncSummary {
    pub server_post_count: u64,
    pub server_interaction_count: u64,
    /// Number of posts the server has with timestamp > client's newest_timestamp.
    pub posts_after_count: u64,
    /// Number of interactions the server has with timestamp > client's newest_interaction_timestamp.
    pub interactions_after_count: u64,
    /// The sync mode the server will use for streaming.
    pub mode: SyncMode,
    pub profile: Option<Profile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncMode {
    /// Counts match, nothing to send.
    UpToDate,
    /// Pure timestamp catch-up: stream posts with ts > client newest.
    TimestampCatchUp,
    /// ID diff required: client must send known IDs.
    NeedIdDiff,
}

/// Streamed frame over the QUIC bi-stream.
/// Length-prefixed: [4-byte big-endian len][JSON payload].
/// A zero-length frame signals end of stream.
#[derive(Debug, Serialize, Deserialize)]
pub enum SyncFrame {
    Posts(Vec<Post>),
    Interactions(Vec<Interaction>),
}
