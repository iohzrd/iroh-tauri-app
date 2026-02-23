use crate::storage::{Post, Profile, Storage};
use bytes::Bytes;
use futures_lite::StreamExt;
use iroh::{Endpoint, EndpointId};
use iroh_gossip::{
    Gossip, TopicId,
    api::{Event, GossipSender},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipMessage {
    NewPost(Post),
    DeletePost { id: String, author: String },
    ProfileUpdate(Profile),
}

pub fn user_feed_topic(pubkey: &str) -> TopicId {
    let mut hasher = Sha256::new();
    hasher.update(b"iroh-social-feed-v1:");
    hasher.update(pubkey.as_bytes());
    TopicId::from_bytes(hasher.finalize().into())
}

pub struct FeedManager {
    pub gossip: Gossip,
    pub endpoint: Endpoint,
    pub my_sender: Option<GossipSender>,
    pub subscriptions: HashMap<String, (GossipSender, JoinHandle<()>)>,
    pub storage: Arc<Storage>,
    pub app_handle: AppHandle,
}

impl FeedManager {
    pub fn new(
        gossip: Gossip,
        endpoint: Endpoint,
        storage: Arc<Storage>,
        app_handle: AppHandle,
    ) -> Self {
        Self {
            gossip,
            endpoint,
            my_sender: None,
            subscriptions: HashMap::new(),
            storage,
            app_handle,
        }
    }

    pub async fn start_own_feed(&mut self) -> anyhow::Result<()> {
        let my_id = self.endpoint.id().to_string();
        let topic = user_feed_topic(&my_id);
        println!("[gossip] starting own feed topic for {}", &my_id[..8]);

        let topic_handle = self.gossip.subscribe(topic, vec![]).await?;
        let (sender, receiver) = topic_handle.split();
        self.my_sender = Some(sender);

        // Listen for neighbors joining/leaving our own feed topic (followers)
        let storage = self.storage.clone();
        let app_handle = self.app_handle.clone();
        tokio::spawn(async move {
            println!("[gossip-own] listener started for own feed neighbors");
            let mut receiver = receiver;
            loop {
                match receiver.try_next().await {
                    Ok(Some(event)) => match &event {
                        Event::NeighborUp(endpoint_id) => {
                            let pubkey = endpoint_id.to_string();
                            println!("[gossip-own] new follower: {}", &pubkey[..8]);
                            let now = crate::now_millis();
                            match storage.upsert_follower(&pubkey, now) {
                                Ok(is_new) => {
                                    let _ = app_handle.emit("follower-changed", &pubkey);
                                    if is_new {
                                        let _ = app_handle.emit("new-follower", &pubkey);
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[gossip-own] failed to store follower: {e}");
                                }
                            }
                        }
                        Event::NeighborDown(endpoint_id) => {
                            let pubkey = endpoint_id.to_string();
                            println!("[gossip-own] follower left: {}", &pubkey[..8]);
                            if let Err(e) = storage.set_follower_offline(&pubkey) {
                                eprintln!("[gossip-own] failed to update follower: {e}");
                            }
                            let _ = app_handle.emit("follower-changed", &pubkey);
                        }
                        _ => {}
                    },
                    Ok(None) => {
                        println!("[gossip-own] own feed stream ended");
                        break;
                    }
                    Err(e) => {
                        eprintln!("[gossip-own] own feed receiver error: {e}");
                        break;
                    }
                }
            }
            println!("[gossip-own] own feed listener stopped");
        });

        Ok(())
    }

    pub async fn broadcast_profile(&self, profile: &Profile) -> anyhow::Result<()> {
        let sender = self
            .my_sender
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("own feed not started"))?;

        let msg = GossipMessage::ProfileUpdate(profile.clone());
        let payload = serde_json::to_vec(&msg)?;
        sender.broadcast(Bytes::from(payload)).await?;
        println!("[gossip] broadcast profile: {}", profile.display_name);

        Ok(())
    }

    pub async fn broadcast_post(&self, post: &Post) -> anyhow::Result<()> {
        let sender = self
            .my_sender
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("own feed not started"))?;

        let msg = GossipMessage::NewPost(post.clone());
        let payload = serde_json::to_vec(&msg)?;
        sender.broadcast(Bytes::from(payload)).await?;
        println!("[gossip] broadcast post {}", &post.id);

        Ok(())
    }

    pub async fn broadcast_delete(&self, id: &str, author: &str) -> anyhow::Result<()> {
        let sender = self
            .my_sender
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("own feed not started"))?;

        let msg = GossipMessage::DeletePost {
            id: id.to_string(),
            author: author.to_string(),
        };
        let payload = serde_json::to_vec(&msg)?;
        sender.broadcast(Bytes::from(payload)).await?;
        println!("[gossip] broadcast delete {id}");

        Ok(())
    }

    pub async fn follow_user(&mut self, pubkey: String) -> anyhow::Result<()> {
        if self.subscriptions.contains_key(&pubkey) {
            println!("[gossip] already subscribed to {}", &pubkey[..8]);
            return Ok(());
        }

        let topic = user_feed_topic(&pubkey);
        let bootstrap: EndpointId = pubkey.parse().map_err(|e| anyhow::anyhow!("{e}"))?;

        println!(
            "[gossip] subscribing to {} (topic: {})",
            &pubkey[..8],
            &format!("{:?}", topic)[..12]
        );
        let topic_handle = self.gossip.subscribe(topic, vec![bootstrap]).await?;
        let (sender, receiver) = topic_handle.split();
        println!("[gossip] subscribed to {}", &pubkey[..8]);

        let storage = self.storage.clone();
        let pk = pubkey.clone();
        let app_handle = self.app_handle.clone();
        let handle = tokio::spawn(async move {
            println!("[gossip-rx] listener started for {}", &pk[..8]);
            let mut receiver = receiver;
            loop {
                match receiver.try_next().await {
                    Ok(Some(event)) => match &event {
                        Event::Received(msg) => {
                            println!(
                                "[gossip-rx] received {} bytes from {}",
                                msg.content.len(),
                                &pk[..8]
                            );
                            match serde_json::from_slice(&msg.content) {
                                Ok(GossipMessage::NewPost(post)) => {
                                    if post.author == pk {
                                        if let Err(reason) = crate::validate_post(&post) {
                                            eprintln!(
                                                "[gossip-rx] rejected post {} from {}: {reason}",
                                                &post.id,
                                                &pk[..8]
                                            );
                                        } else {
                                            println!(
                                                "[gossip-rx] new post {} from {}",
                                                &post.id,
                                                &pk[..8]
                                            );
                                            if let Err(e) = storage.insert_post(&post) {
                                                eprintln!("[gossip-rx] failed to store post: {e}");
                                            }
                                            let _ = app_handle.emit("feed-updated", ());
                                        }
                                    } else {
                                        println!(
                                            "[gossip-rx] ignored post from {} (expected {})",
                                            &post.author[..8],
                                            &pk[..8]
                                        );
                                    }
                                }
                                Ok(GossipMessage::DeletePost { id, author }) => {
                                    if author == pk {
                                        // Verify the stored post belongs to this author
                                        match storage.get_post_by_id(&id) {
                                            Ok(Some(post)) if post.author == pk => {
                                                println!(
                                                    "[gossip-rx] delete post {id} from {}",
                                                    &pk[..8]
                                                );
                                                if let Err(e) = storage.delete_post(&id) {
                                                    eprintln!(
                                                        "[gossip-rx] failed to delete post: {e}"
                                                    );
                                                }
                                                let _ = app_handle.emit("feed-updated", ());
                                            }
                                            Ok(Some(_)) => {
                                                eprintln!(
                                                    "[gossip-rx] rejected delete for {id}: author mismatch"
                                                );
                                            }
                                            Ok(None) => {
                                                // Post not in our DB; ignore
                                            }
                                            Err(e) => {
                                                eprintln!(
                                                    "[gossip-rx] failed to look up post {id}: {e}"
                                                );
                                            }
                                        }
                                    }
                                }
                                Ok(GossipMessage::ProfileUpdate(profile)) => {
                                    println!(
                                        "[gossip-rx] profile update from {}: {}",
                                        &pk[..8],
                                        profile.display_name
                                    );
                                    if let Err(e) = storage.save_remote_profile(&pk, &profile) {
                                        eprintln!("[gossip-rx] failed to store profile: {e}");
                                    }
                                    let _ = app_handle.emit("profile-updated", &pk);
                                }
                                Err(e) => {
                                    eprintln!("[gossip-rx] failed to parse message: {e}");
                                }
                            }
                        }
                        other => {
                            println!("[gossip-rx] event from {}: {other:?}", &pk[..8]);
                        }
                    },
                    Ok(None) => {
                        println!("[gossip-rx] stream ended for {}", &pk[..8]);
                        break;
                    }
                    Err(e) => {
                        eprintln!("[gossip-rx] receiver error for {}: {e}", &pk[..8]);
                        break;
                    }
                }
            }
            println!("[gossip-rx] listener stopped for {}", &pk[..8]);
        });

        self.subscriptions.insert(pubkey, (sender, handle));
        Ok(())
    }

    pub fn unfollow_user(&mut self, pubkey: &str) {
        if let Some((_sender, handle)) = self.subscriptions.remove(pubkey) {
            println!("[gossip] unsubscribed from {}", &pubkey[..8]);
            handle.abort();
        }
    }
}
