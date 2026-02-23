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
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipMessage {
    NewPost(Post),
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
}

impl FeedManager {
    pub fn new(gossip: Gossip, endpoint: Endpoint, storage: Arc<Storage>) -> Self {
        Self {
            gossip,
            endpoint,
            my_sender: None,
            subscriptions: HashMap::new(),
            storage,
        }
    }

    pub async fn start_own_feed(&mut self) -> anyhow::Result<()> {
        let my_id = self.endpoint.id().to_string();
        let topic = user_feed_topic(&my_id);

        let topic_handle = self.gossip.subscribe(topic, vec![]).await?;
        let (sender, _receiver) = topic_handle.split();
        self.my_sender = Some(sender);

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

        Ok(())
    }

    pub async fn follow_user(&mut self, pubkey: String) -> anyhow::Result<()> {
        if self.subscriptions.contains_key(&pubkey) {
            return Ok(());
        }

        let topic = user_feed_topic(&pubkey);
        let bootstrap: EndpointId = pubkey.parse().map_err(|e| anyhow::anyhow!("{e}"))?;

        let topic_handle = self.gossip.subscribe(topic, vec![bootstrap]).await?;
        let (sender, receiver) = topic_handle.split();

        let storage = self.storage.clone();
        let pk = pubkey.clone();
        let handle = tokio::spawn(async move {
            let mut receiver = receiver;
            loop {
                match receiver.try_next().await {
                    Ok(Some(event)) => {
                        if let Event::Received(msg) = event {
                            match serde_json::from_slice(&msg.content) {
                                Ok(GossipMessage::NewPost(post)) => {
                                    if post.author == pk {
                                        if let Err(e) = storage.insert_post(&post) {
                                            eprintln!("[gossip] failed to store post: {e}");
                                        }
                                    }
                                }
                                Ok(GossipMessage::ProfileUpdate(profile)) => {
                                    if let Err(e) = storage.save_remote_profile(&pk, &profile) {
                                        eprintln!("[gossip] failed to store profile: {e}");
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[gossip] failed to parse message: {e}");
                                }
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        eprintln!("[gossip] receiver error: {e}");
                        break;
                    }
                }
            }
        });

        self.subscriptions.insert(pubkey, (sender, handle));
        Ok(())
    }

    pub fn unfollow_user(&mut self, pubkey: &str) {
        if let Some((_sender, handle)) = self.subscriptions.remove(pubkey) {
            handle.abort();
        }
    }
}
