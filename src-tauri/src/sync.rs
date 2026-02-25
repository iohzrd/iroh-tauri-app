use crate::storage::Storage;
use iroh::{
    Endpoint, EndpointAddr, EndpointId,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use iroh_social_types::{SyncRequest, SyncResponse, short_id};
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
            .read_to_end(65536)
            .await
            .map_err(AcceptError::from_err)?;

        let req: SyncRequest = serde_json::from_slice(&req_bytes).map_err(AcceptError::from_err)?;
        println!(
            "[sync-server] request: author={}, before={:?}, after={:?}, limit={}",
            short_id(&req.author),
            req.before,
            req.after,
            req.limit
        );

        let map_err =
            |e: anyhow::Error| AcceptError::from_err(std::io::Error::other(e.to_string()));

        let posts = if let Some(after) = req.after {
            self.storage
                .get_posts_by_author_after(&req.author, after, req.limit as usize)
                .map_err(map_err)?
        } else {
            self.storage
                .get_posts_by_author(&req.author, req.limit as usize, req.before, None)
                .map_err(map_err)?
        };

        let total_count = self
            .storage
            .count_posts_by_author(&req.author)
            .map_err(map_err)?;
        let (oldest_ts, newest_ts) = self
            .storage
            .get_author_post_range(&req.author)
            .map_err(map_err)?;

        // Include our own profile in the response
        let profile = self.storage.get_profile().ok().flatten();

        println!(
            "[sync-server] responding with {} posts (total={}, profile: {}) to {}",
            posts.len(),
            total_count,
            profile.as_ref().map_or("none", |p| &p.display_name),
            short_id(&remote.to_string())
        );

        let resp = SyncResponse {
            posts,
            profile,
            total_count,
            newest_ts,
            oldest_ts,
        };
        let resp_bytes = serde_json::to_vec(&resp).map_err(AcceptError::from_err)?;

        send.write_all(&resp_bytes)
            .await
            .map_err(AcceptError::from_err)?;
        send.finish().map_err(AcceptError::from_err)?;

        println!(
            "[sync-server] sent response, waiting for peer to close connection {}",
            short_id(&remote.to_string())
        );

        // Wait for the peer to close the connection.
        // This is the canonical iroh pattern to prevent the Connection drop from
        // sending ApplicationClose(error_code: 0) before the client reads all data.
        conn.closed().await;

        println!(
            "[sync-server] completed sync for {}",
            short_id(&remote.to_string())
        );

        Ok(())
    }
}

/// Client: fetch paginated posts from a remote peer
pub async fn fetch_remote_posts(
    endpoint: &Endpoint,
    target: EndpointId,
    author: &str,
    before: Option<u64>,
    after: Option<u64>,
    limit: u32,
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
        before,
        after,
        limit,
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
