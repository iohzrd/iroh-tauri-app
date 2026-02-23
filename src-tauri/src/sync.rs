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
        let (mut send, mut recv) = conn.accept_bi().await?;

        let req_bytes = recv
            .read_to_end(65536)
            .await
            .map_err(AcceptError::from_err)?;

        let req: SyncRequest = serde_json::from_slice(&req_bytes).map_err(AcceptError::from_err)?;

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
    let conn = endpoint.connect(addr, SYNC_ALPN).await?;

    let (mut send, mut recv) = conn.open_bi().await?;

    let req = SyncRequest {
        author: author.to_string(),
        before,
        limit,
    };
    let req_bytes = serde_json::to_vec(&req)?;

    send.write_all(&req_bytes).await?;
    send.finish()?;

    let resp_bytes = recv.read_to_end(1_048_576).await?; // 1MB max
    let resp: SyncResponse = serde_json::from_slice(&resp_bytes)?;

    Ok(resp.posts)
}
