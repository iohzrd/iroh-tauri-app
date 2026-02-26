use crate::storage::Storage;
use iroh::{
    Endpoint, EndpointAddr, EndpointId,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use iroh_social_types::{SyncRequest, SyncResponse, short_id};
use std::collections::HashSet;
use std::sync::Arc;

pub use iroh_social_types::SYNC_ALPN;

#[derive(Debug, Clone)]
pub struct SyncHandler {
    storage: Arc<Storage>,
}

impl SyncHandler {
    pub fn new(storage: Arc<Storage>) -> Self {
        Self { storage }
    }
}

impl ProtocolHandler for SyncHandler {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let remote = conn.remote_id();
        println!(
            "[sync-server] incoming sync request from {}",
            short_id(&remote.to_string())
        );

        let (mut send, mut recv) = conn.accept_bi().await?;

        let req_bytes = recv
            .read_to_end(262_144)
            .await
            .map_err(AcceptError::from_err)?;

        let req: SyncRequest = serde_json::from_slice(&req_bytes).map_err(AcceptError::from_err)?;

        let map_err =
            |e: anyhow::Error| AcceptError::from_err(std::io::Error::other(e.to_string()));

        let total_count = self
            .storage
            .count_posts_by_author(&req.author)
            .map_err(map_err)?;

        // Get our local post IDs for this author
        let local_ids: Vec<String> = self
            .storage
            .get_post_ids_by_author(&req.author)
            .map_err(map_err)?;

        println!(
            "[sync-server] request: author={}, limit={}, known_ids={}, total={}\n  local_ids: {:?}\n  peer_known: {:?}",
            short_id(&req.author),
            req.limit,
            req.known_ids.len(),
            total_count,
            local_ids.iter().map(|id| short_id(id)).collect::<Vec<_>>(),
            req.known_ids
                .iter()
                .map(|id| short_id(id))
                .collect::<Vec<_>>(),
        );

        // Show which IDs differ
        let local_set: HashSet<&str> = local_ids.iter().map(|s| s.as_str()).collect();
        let peer_set: HashSet<&str> = req.known_ids.iter().map(|s| s.as_str()).collect();
        let only_local: Vec<_> = local_set.difference(&peer_set).collect();
        let only_peer: Vec<_> = peer_set.difference(&local_set).collect();
        if !only_local.is_empty() || !only_peer.is_empty() {
            println!(
                "[sync-server] diff: only_local={:?}, only_peer={:?}",
                only_local.iter().map(|id| short_id(id)).collect::<Vec<_>>(),
                only_peer.iter().map(|id| short_id(id)).collect::<Vec<_>>(),
            );
        }

        // Fetch all posts by this author, filter out ones the requester already has
        let all_posts = self
            .storage
            .get_posts_by_author(&req.author, req.limit as usize, None, None)
            .map_err(map_err)?;

        let mut posts = if req.known_ids.is_empty() {
            all_posts
        } else {
            let known: HashSet<&str> = req.known_ids.iter().map(|s| s.as_str()).collect();
            all_posts
                .into_iter()
                .filter(|p| !known.contains(p.id.as_str()))
                .collect()
        };
        posts.truncate(req.limit as usize);

        // Include interactions by this author
        let interactions = self
            .storage
            .get_interactions_by_author(&req.author, req.limit as usize, None)
            .map_err(map_err)?;

        // Include our own profile in the response
        let profile = self.storage.get_profile().ok().flatten();

        println!(
            "[sync-server] responding with {} posts, {} interactions (total={}, profile: {}) to {}",
            posts.len(),
            interactions.len(),
            total_count,
            profile.as_ref().map_or("none", |p| &p.display_name),
            short_id(&remote.to_string())
        );

        let resp = SyncResponse {
            posts,
            profile,
            total_count,
            interactions,
        };
        let resp_bytes = serde_json::to_vec(&resp).map_err(AcceptError::from_err)?;

        send.write_all(&resp_bytes)
            .await
            .map_err(AcceptError::from_err)?;
        send.finish().map_err(AcceptError::from_err)?;

        // Wait for the peer to close the connection.
        // This is the canonical iroh pattern to prevent the Connection drop from
        // sending ApplicationClose(error_code: 0) before the client reads all data.
        conn.closed().await;

        Ok(())
    }
}

/// Client: fetch posts from a remote peer, excluding posts we already have
pub async fn fetch_remote_posts(
    endpoint: &Endpoint,
    target: EndpointId,
    author: &str,
    limit: u32,
    known_ids: Vec<String>,
) -> anyhow::Result<SyncResponse> {
    let addr = EndpointAddr::from(target);
    println!(
        "[sync-client] connecting to {} for posts...",
        short_id(author)
    );
    let start = std::time::Instant::now();
    let conn = match endpoint.connect(addr, SYNC_ALPN).await {
        Ok(c) => {
            println!(
                "[sync-client] connected to {} in {:.1}s (remote: {})",
                short_id(author),
                start.elapsed().as_secs_f64(),
                c.remote_id()
            );
            c
        }
        Err(e) => {
            eprintln!(
                "[sync-client] failed to connect to {} after {:.1}s: {e:?}",
                short_id(author),
                start.elapsed().as_secs_f64()
            );
            return Err(e.into());
        }
    };

    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| {
        eprintln!(
            "[sync-client] failed to open bi-stream to {}: {e:?}",
            short_id(author)
        );
        e
    })?;

    let req = SyncRequest {
        author: author.to_string(),
        limit,
        known_ids,
    };
    let req_bytes = serde_json::to_vec(&req)?;

    send.write_all(&req_bytes).await?;
    send.finish()?;

    let resp_bytes = recv.read_to_end(1_048_576).await.map_err(|e| {
        eprintln!(
            "[sync-client] failed to read response from {}: {e:?}",
            short_id(author)
        );
        e
    })?; // 1MB max
    let resp: SyncResponse = serde_json::from_slice(&resp_bytes)?;

    println!(
        "[sync-client] received {} posts (total={}, profile: {}) from {} in {:.1}s",
        resp.posts.len(),
        resp.total_count,
        resp.profile.as_ref().map_or("none", |p| &p.display_name),
        short_id(author),
        start.elapsed().as_secs_f64()
    );

    // Explicitly close so the server's closed().await resolves promptly
    conn.close(0u32.into(), b"done");

    Ok(resp)
}
