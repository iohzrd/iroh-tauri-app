use crate::storage::{Post, Storage};
use iroh::{
    Endpoint, EndpointAddr, EndpointId,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const SYNC_ALPN: &[u8] = b"iroh-social/sync/1";

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncRequest {
    pub author: String,
    pub before: Option<u64>,
    pub limit: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncResponse {
    pub posts: Vec<Post>,
}

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
            &remote.to_string()[..8]
        );

        let (mut send, mut recv) = conn.accept_bi().await?;

        let req_bytes = recv
            .read_to_end(65536)
            .await
            .map_err(AcceptError::from_err)?;

        let req: SyncRequest = serde_json::from_slice(&req_bytes).map_err(AcceptError::from_err)?;
        println!(
            "[sync-server] request: author={}, before={:?}, limit={}",
            &req.author[..8],
            req.before,
            req.limit
        );

        let posts = self
            .storage
            .get_posts_by_author(&req.author, req.limit as usize)
            .map_err(|e| {
                AcceptError::from_err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;

        // Filter by `before` if specified
        let posts: Vec<Post> = match req.before {
            Some(before) => posts.into_iter().filter(|p| p.timestamp < before).collect(),
            None => posts,
        };

        println!(
            "[sync-server] responding with {} posts to {}",
            posts.len(),
            &remote.to_string()[..8]
        );

        let resp = SyncResponse { posts };
        let resp_bytes = serde_json::to_vec(&resp).map_err(AcceptError::from_err)?;

        send.write_all(&resp_bytes)
            .await
            .map_err(AcceptError::from_err)?;
        send.finish().map_err(AcceptError::from_err)?;

        Ok(())
    }
}

/// Client: fetch paginated posts from a remote peer
pub async fn fetch_remote_posts(
    endpoint: &Endpoint,
    target: EndpointId,
    author: &str,
    before: Option<u64>,
    limit: u32,
) -> anyhow::Result<Vec<Post>> {
    let addr = EndpointAddr::from(target);
    println!("[sync-client] connecting to {} for posts...", &author[..8]);
    let start = std::time::Instant::now();
    let conn = match endpoint.connect(addr, SYNC_ALPN).await {
        Ok(c) => {
            println!(
                "[sync-client] connected to {} in {:.1}s (remote: {})",
                &author[..8],
                start.elapsed().as_secs_f64(),
                c.remote_id()
            );
            c
        }
        Err(e) => {
            eprintln!(
                "[sync-client] failed to connect to {} after {:.1}s: {e:?}",
                &author[..8],
                start.elapsed().as_secs_f64()
            );
            return Err(e.into());
        }
    };

    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| {
        eprintln!(
            "[sync-client] failed to open bi-stream to {}: {e:?}",
            &author[..8]
        );
        e
    })?;

    let req = SyncRequest {
        author: author.to_string(),
        before,
        limit,
    };
    let req_bytes = serde_json::to_vec(&req)?;

    send.write_all(&req_bytes).await?;
    send.finish()?;

    let resp_bytes = recv.read_to_end(1_048_576).await.map_err(|e| {
        eprintln!(
            "[sync-client] failed to read response from {}: {e:?}",
            &author[..8]
        );
        e
    })?; // 1MB max
    let resp: SyncResponse = serde_json::from_slice(&resp_bytes)?;

    println!(
        "[sync-client] received {} posts from {} in {:.1}s",
        resp.posts.len(),
        &author[..8],
        start.elapsed().as_secs_f64()
    );

    Ok(resp.posts)
}
