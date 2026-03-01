mod crypto;
mod dm;
mod gossip;
mod storage;
mod sync;

use crate::dm::DmHandler;
use crate::gossip::FeedManager;
use crate::storage::Storage;
use iroh::{Endpoint, SecretKey, protocol::Router};
use iroh_blobs::{BlobsProtocol, HashAndFormat, store::fs::FsStore, ticket::BlobTicket};
use iroh_gossip::Gossip;
use iroh_social_types::{
    ConversationMeta, DM_ALPN, DirectMessage, FollowEntry, FollowerEntry, Interaction,
    InteractionKind, MAX_BLOB_SIZE, MediaAttachment, Post, Profile, StoredMessage, now_millis,
    parse_mentions, short_id, sign_interaction, sign_post, validate_interaction, validate_post,
    validate_profile, verify_interaction_signature, verify_post_signature,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

pub struct AppState {
    pub endpoint: Endpoint,
    pub router: Router,
    pub blobs: BlobsProtocol,
    pub store: FsStore,
    pub storage: Arc<Storage>,
    pub feed: Arc<Mutex<FeedManager>>,
    pub dm: DmHandler,
    pub secret_key_bytes: [u8; 32],
}

// -- Identity --

#[tauri::command]
async fn get_node_id(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    Ok(state.endpoint.id().to_string())
}

// -- Profile --

#[tauri::command]
async fn get_my_profile(state: State<'_, Arc<AppState>>) -> Result<Option<Profile>, String> {
    let node_id = state.endpoint.id().to_string();
    state
        .storage
        .get_profile(&node_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn save_my_profile(
    state: State<'_, Arc<AppState>>,
    display_name: String,
    bio: String,
    avatar_hash: Option<String>,
    avatar_ticket: Option<String>,
    is_private: bool,
) -> Result<(), String> {
    let node_id = state.endpoint.id().to_string();
    let profile = Profile {
        display_name: display_name.clone(),
        bio: bio.clone(),
        avatar_hash,
        avatar_ticket,
        is_private,
    };
    validate_profile(&profile)?;
    state
        .storage
        .save_profile(&node_id, &profile)
        .map_err(|e| e.to_string())?;
    log::info!("[profile] saved profile: {display_name} (private={is_private})");
    let feed = state.feed.lock().await;
    feed.broadcast_profile(&profile)
        .await
        .map_err(|e| e.to_string())?;
    log::info!("[profile] broadcast profile update");
    Ok(())
}

#[tauri::command]
async fn get_remote_profile(
    state: State<'_, Arc<AppState>>,
    pubkey: String,
) -> Result<Option<Profile>, String> {
    state
        .storage
        .get_profile(&pubkey)
        .map_err(|e| e.to_string())
}

// -- Posts --

fn generate_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("failed to generate random bytes");
    let (a, b) = bytes.split_at(8);
    format!(
        "{:016x}{:016x}",
        u64::from_le_bytes(a.try_into().unwrap()),
        u64::from_le_bytes(b.try_into().unwrap())
    )
}

#[tauri::command]
async fn create_post(
    state: State<'_, Arc<AppState>>,
    content: String,
    media: Option<Vec<MediaAttachment>>,
    reply_to: Option<String>,
    reply_to_author: Option<String>,
    quote_of: Option<String>,
    quote_of_author: Option<String>,
) -> Result<Post, String> {
    let author = state.endpoint.id().to_string();
    let media_count = media.as_ref().map_or(0, |m| m.len());
    let mut post = Post {
        id: generate_id(),
        author,
        content,
        timestamp: now_millis(),
        media: media.unwrap_or_default(),
        reply_to,
        reply_to_author,
        quote_of,
        quote_of_author,
        signature: String::new(),
    };

    validate_post(&post)?;

    let sk = SecretKey::from_bytes(&state.secret_key_bytes);
    sign_post(&mut post, &sk);

    state
        .storage
        .insert_post(&post)
        .map_err(|e| e.to_string())?;
    log::info!(
        "[post] created post {} ({} media attachments)",
        &post.id,
        media_count
    );
    let feed = state.feed.lock().await;
    feed.broadcast_post(&post)
        .await
        .map_err(|e| e.to_string())?;
    log::info!("[post] broadcast post {}", &post.id);

    Ok(post)
}

#[tauri::command]
async fn delete_post(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    let my_id = state.endpoint.id().to_string();

    // Verify the post exists and belongs to us
    let post = state
        .storage
        .get_post_by_id(&id)
        .map_err(|e| e.to_string())?;
    match post {
        Some(post) if post.author == my_id => {}
        Some(_) => {
            return Err("cannot delete posts authored by other users".to_string());
        }
        None => {
            return Err(format!("post {id} not found"));
        }
    }

    let removed = state.storage.delete_post(&id).map_err(|e| e.to_string())?;
    log::info!("[post] delete post {id}: removed={removed}");
    let feed = state.feed.lock().await;
    feed.broadcast_delete(&id, &my_id)
        .await
        .map_err(|e| e.to_string())?;
    log::info!("[post] broadcast delete {id}");

    Ok(())
}

#[tauri::command]
async fn get_feed(
    state: State<'_, Arc<AppState>>,
    limit: Option<usize>,
    before: Option<u64>,
) -> Result<Vec<Post>, String> {
    let q = crate::storage::FeedQuery {
        limit: limit.unwrap_or(20),
        before,
    };
    let posts = state.storage.get_feed(&q).map_err(|e| e.to_string())?;
    log::info!("[feed] loaded {} posts", posts.len());
    Ok(posts)
}

#[tauri::command]
async fn get_notifications(
    state: State<'_, Arc<AppState>>,
    limit: Option<usize>,
    before: Option<u64>,
) -> Result<Vec<storage::Notification>, String> {
    state
        .storage
        .get_notifications(limit.unwrap_or(30), before)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_unread_notification_count(state: State<'_, Arc<AppState>>) -> Result<u32, String> {
    state
        .storage
        .get_unread_notification_count()
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn mark_notifications_read(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state
        .storage
        .mark_notifications_read()
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_user_posts(
    state: State<'_, Arc<AppState>>,
    pubkey: String,
    limit: Option<usize>,
    before: Option<u64>,
    media_filter: Option<String>,
) -> Result<Vec<Post>, String> {
    state
        .storage
        .get_posts_by_author(
            &pubkey,
            limit.unwrap_or(20),
            before,
            media_filter.as_deref(),
        )
        .map_err(|e| e.to_string())
}

// -- Sync (pull history from peers) --

/// Validate and store posts/interactions/profile from a sync result.
/// Returns the number of posts actually stored.
fn process_sync_result(
    storage: &Storage,
    pubkey: &str,
    result: &sync::SyncResult,
    label: &str,
    my_id: &str,
    app_handle: &AppHandle,
) -> usize {
    let mut stored = 0;
    for post in &result.posts {
        if let Err(reason) = validate_post(post) {
            log::error!("[{label}] rejected post {}: {reason}", &post.id);
            continue;
        }
        if let Err(reason) = verify_post_signature(post) {
            log::error!("[{label}] rejected post {} (bad sig): {reason}", &post.id);
            continue;
        }
        if let Err(e) = storage.insert_post(post) {
            log::error!("[{label}] failed to store post: {e}");
            continue;
        }
        if post.author != my_id {
            if parse_mentions(&post.content).contains(&my_id.to_string()) {
                let _ = storage.insert_notification(
                    "mention",
                    &post.author,
                    None,
                    Some(&post.id),
                    post.timestamp,
                );
                let _ = app_handle.emit("mentioned-in-post", post);
                let _ = app_handle.emit("notification-received", ());
            }
            if post.reply_to_author.as_deref() == Some(my_id) {
                let _ = storage.insert_notification(
                    "reply",
                    &post.author,
                    post.reply_to.as_deref(),
                    Some(&post.id),
                    post.timestamp,
                );
                let _ = app_handle.emit("notification-received", ());
            }
            if post.quote_of_author.as_deref() == Some(my_id) {
                let _ = storage.insert_notification(
                    "quote",
                    &post.author,
                    post.quote_of.as_deref(),
                    Some(&post.id),
                    post.timestamp,
                );
                let _ = app_handle.emit("notification-received", ());
            }
        }
        stored += 1;
    }
    if let Some(profile) = &result.profile
        && let Err(e) = storage.save_profile(pubkey, profile)
    {
        log::error!("[{label}] failed to store profile: {e}");
    }
    for interaction in &result.interactions {
        if interaction.author == pubkey
            && validate_interaction(interaction).is_ok()
            && verify_interaction_signature(interaction).is_ok()
        {
            let _ = storage.save_interaction(interaction);
            if interaction.target_author == my_id && interaction.author != my_id {
                let _ = storage.insert_notification(
                    "like",
                    &interaction.author,
                    Some(&interaction.target_post_id),
                    None,
                    interaction.timestamp,
                );
                let _ = app_handle.emit("notification-received", ());
            }
        }
    }
    stored
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FrontendSyncResult {
    posts: Vec<Post>,
    remote_total: u64,
}

#[tauri::command]
async fn sync_posts(
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
    pubkey: String,
) -> Result<FrontendSyncResult, String> {
    let endpoint = state.endpoint.clone();
    let storage = state.storage.clone();
    let target: iroh::EndpointId = pubkey.parse().map_err(|e| format!("{e}"))?;

    let my_id = state.endpoint.id().to_string();
    let result = sync::sync_from_peer(&endpoint, &storage, target, &pubkey)
        .await
        .map_err(|e| e.to_string())?;

    let stored = process_sync_result(&storage, &pubkey, &result, "sync", &my_id, &app_handle);
    log::info!(
        "[sync] stored {stored}/{} posts from {} (mode={:?})",
        result.posts.len(),
        short_id(&pubkey),
        result.mode,
    );

    Ok(FrontendSyncResult {
        posts: result.posts,
        remote_total: result.remote_post_count,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SyncStatus {
    local_count: u64,
}

#[tauri::command]
async fn get_sync_status(
    state: State<'_, Arc<AppState>>,
    pubkey: String,
) -> Result<SyncStatus, String> {
    let local_count = state
        .storage
        .count_posts_by_author(&pubkey)
        .map_err(|e| e.to_string())?;
    Ok(SyncStatus { local_count })
}

#[tauri::command]
async fn fetch_older_posts(
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
    pubkey: String,
) -> Result<FrontendSyncResult, String> {
    sync_posts(app_handle, state, pubkey).await
}

// -- Interactions (likes/reposts) --

#[tauri::command]
async fn like_post(
    state: State<'_, Arc<AppState>>,
    target_post_id: String,
    target_author: String,
) -> Result<Interaction, String> {
    let my_id = state.endpoint.id().to_string();
    let mut interaction = Interaction {
        id: generate_id(),
        author: my_id,
        kind: InteractionKind::Like,
        target_post_id,
        target_author,
        timestamp: now_millis(),
        signature: String::new(),
    };
    let sk = SecretKey::from_bytes(&state.secret_key_bytes);
    sign_interaction(&mut interaction, &sk);
    state
        .storage
        .save_interaction(&interaction)
        .map_err(|e| e.to_string())?;
    let feed = state.feed.lock().await;
    feed.broadcast_interaction(&interaction)
        .await
        .map_err(|e| e.to_string())?;
    Ok(interaction)
}

#[tauri::command]
async fn unlike_post(
    state: State<'_, Arc<AppState>>,
    target_post_id: String,
) -> Result<(), String> {
    let my_id = state.endpoint.id().to_string();
    let id = state
        .storage
        .delete_interaction_by_target(&my_id, "Like", &target_post_id)
        .map_err(|e| e.to_string())?;
    if let Some(id) = id {
        let feed = state.feed.lock().await;
        feed.broadcast_delete_interaction(&id, &my_id)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn repost(
    state: State<'_, Arc<AppState>>,
    target_post_id: String,
    target_author: String,
) -> Result<Post, String> {
    let author = state.endpoint.id().to_string();
    let mut post = Post {
        id: generate_id(),
        author,
        content: String::new(),
        timestamp: now_millis(),
        media: vec![],
        reply_to: None,
        reply_to_author: None,
        quote_of: Some(target_post_id),
        quote_of_author: Some(target_author),
        signature: String::new(),
    };

    validate_post(&post)?;

    let sk = SecretKey::from_bytes(&state.secret_key_bytes);
    sign_post(&mut post, &sk);

    state
        .storage
        .insert_post(&post)
        .map_err(|e| e.to_string())?;
    let feed = state.feed.lock().await;
    feed.broadcast_post(&post)
        .await
        .map_err(|e| e.to_string())?;
    Ok(post)
}

#[tauri::command]
async fn unrepost(state: State<'_, Arc<AppState>>, target_post_id: String) -> Result<(), String> {
    let my_id = state.endpoint.id().to_string();
    let id = state
        .storage
        .delete_repost_by_target(&my_id, &target_post_id)
        .map_err(|e| e.to_string())?;
    if let Some(id) = id {
        let feed = state.feed.lock().await;
        feed.broadcast_delete(&id, &my_id)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn get_post_counts(
    state: State<'_, Arc<AppState>>,
    target_post_id: String,
) -> Result<storage::PostCounts, String> {
    let my_id = state.endpoint.id().to_string();
    state
        .storage
        .get_post_counts(&my_id, &target_post_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_replies(
    state: State<'_, Arc<AppState>>,
    target_post_id: String,
    limit: Option<u32>,
    before: Option<u64>,
) -> Result<Vec<Post>, String> {
    state
        .storage
        .get_replies(&target_post_id, limit.unwrap_or(50) as usize, before)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_post(state: State<'_, Arc<AppState>>, id: String) -> Result<Option<Post>, String> {
    state.storage.get_post_by_id(&id).map_err(|e| e.to_string())
}

// -- Follows --

#[tauri::command]
async fn follow_user(
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
    pubkey: String,
) -> Result<(), String> {
    let my_id = state.endpoint.id().to_string();
    if pubkey == my_id {
        return Err("cannot follow yourself".to_string());
    }
    log::info!("[follow] following {}...", short_id(&pubkey));
    let entry = FollowEntry {
        pubkey: pubkey.clone(),
        alias: None,
        followed_at: now_millis(),
    };
    state.storage.follow(&entry).map_err(|e| e.to_string())?;

    {
        let mut feed = state.feed.lock().await;
        feed.follow_user(pubkey.clone())
            .await
            .map_err(|e| e.to_string())?;
    }
    log::info!("[follow] subscribed to gossip for {}", short_id(&pubkey));

    // Sync existing posts from the followed user (lock dropped, no blocking)
    log::info!("[follow] syncing posts from {}...", short_id(&pubkey));
    let endpoint = state.endpoint.clone();
    let storage = state.storage.clone();
    let target: iroh::EndpointId = pubkey.parse().map_err(|e| format!("{e}"))?;
    match sync::sync_from_peer(&endpoint, &storage, target, &pubkey).await {
        Ok(result) => {
            let stored = process_sync_result(
                &storage,
                &pubkey,
                &result,
                "follow-sync",
                &my_id,
                &app_handle,
            );
            log::info!(
                "[follow-sync] stored {stored}/{} posts, {} interactions from {} (mode={:?})",
                result.posts.len(),
                result.interactions.len(),
                short_id(&pubkey),
                result.mode,
            );
        }
        Err(e) => {
            log::error!(
                "[follow-sync] failed to sync from {}: {e}",
                short_id(&pubkey)
            );
        }
    }

    Ok(())
}

#[tauri::command]
async fn unfollow_user(state: State<'_, Arc<AppState>>, pubkey: String) -> Result<(), String> {
    log::info!("[follow] unfollowing {}...", short_id(&pubkey));
    state.storage.unfollow(&pubkey).map_err(|e| e.to_string())?;
    let mut feed = state.feed.lock().await;
    feed.unfollow_user(&pubkey);
    log::info!("[follow] unfollowed {}", short_id(&pubkey));
    Ok(())
}

#[tauri::command]
async fn update_follow_alias(
    state: State<'_, Arc<AppState>>,
    pubkey: String,
    alias: Option<String>,
) -> Result<(), String> {
    state
        .storage
        .update_follow_alias(&pubkey, alias.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_follows(state: State<'_, Arc<AppState>>) -> Result<Vec<FollowEntry>, String> {
    state.storage.get_follows().map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_followers(state: State<'_, Arc<AppState>>) -> Result<Vec<FollowerEntry>, String> {
    state.storage.get_followers().map_err(|e| e.to_string())
}

// -- Blobs (media) --

#[tauri::command]
async fn add_blob(
    state: State<'_, Arc<AppState>>,
    content: String,
) -> Result<serde_json::Value, String> {
    if content.len() > MAX_BLOB_SIZE {
        return Err(format!(
            "blob too large: {} bytes (max {} bytes)",
            content.len(),
            MAX_BLOB_SIZE
        ));
    }

    let tag = state
        .store
        .add_slice(content.as_bytes())
        .await
        .map_err(|e| e.to_string())?;

    let addr = state.endpoint.addr();
    let ticket = BlobTicket::new(addr, tag.hash, tag.format);
    log::info!("[blob] added text blob {}", tag.hash);

    Ok(serde_json::json!({
        "hash": tag.hash.to_string(),
        "ticket": ticket.to_string(),
    }))
}

#[tauri::command]
async fn fetch_blob(state: State<'_, Arc<AppState>>, ticket: String) -> Result<String, String> {
    let ticket: BlobTicket = ticket.parse().map_err(|e| format!("{e}"))?;
    let store = state.store.clone();
    let endpoint = state.endpoint.clone();
    let blobs = state.blobs.clone();

    log::info!("[blob] fetching text blob {}...", ticket.hash());
    let conn = endpoint
        .connect(ticket.addr().clone(), iroh_blobs::ALPN)
        .await
        .map_err(|e| e.to_string())?;

    let hash_and_format: HashAndFormat = ticket.hash_and_format();
    blobs
        .remote()
        .fetch(conn, hash_and_format)
        .await
        .map_err(|e| e.to_string())?;

    let bytes = store
        .get_bytes(ticket.hash())
        .await
        .map_err(|e| e.to_string())?;

    log::info!(
        "[blob] fetched text blob {} ({} bytes)",
        ticket.hash(),
        bytes.len()
    );
    String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string())
}

#[tauri::command]
async fn add_blob_bytes(
    state: State<'_, Arc<AppState>>,
    data: Vec<u8>,
) -> Result<serde_json::Value, String> {
    if data.len() > MAX_BLOB_SIZE {
        return Err(format!(
            "blob too large: {} bytes (max {} bytes)",
            data.len(),
            MAX_BLOB_SIZE
        ));
    }

    let size = data.len();
    let tag = state
        .store
        .add_slice(&data)
        .await
        .map_err(|e| e.to_string())?;

    let addr = state.endpoint.addr();
    let ticket = BlobTicket::new(addr, tag.hash, tag.format);
    log::info!("[blob] added blob {} ({size} bytes)", tag.hash);

    Ok(serde_json::json!({
        "hash": tag.hash.to_string(),
        "ticket": ticket.to_string(),
    }))
}

#[tauri::command]
async fn fetch_blob_bytes(
    state: State<'_, Arc<AppState>>,
    ticket: String,
) -> Result<Vec<u8>, String> {
    let ticket: BlobTicket = ticket.parse().map_err(|e| format!("{e}"))?;
    let store = state.store.clone();
    let endpoint = state.endpoint.clone();
    let blobs = state.blobs.clone();

    // Try local store first -- no lock held
    if let Ok(bytes) = store.get_bytes(ticket.hash()).await {
        return Ok(bytes.to_vec());
    }

    // Fetch from remote peer -- no lock held
    log::info!("[blob] fetching {} from remote...", ticket.hash());
    let conn = endpoint
        .connect(ticket.addr().clone(), iroh_blobs::ALPN)
        .await
        .map_err(|e| e.to_string())?;

    let hash_and_format: HashAndFormat = ticket.hash_and_format();
    blobs
        .remote()
        .fetch(conn, hash_and_format)
        .await
        .map_err(|e| e.to_string())?;

    let bytes = store
        .get_bytes(ticket.hash())
        .await
        .map_err(|e| e.to_string())?;

    log::info!(
        "[blob] fetched {} from remote ({} bytes)",
        ticket.hash(),
        bytes.len()
    );
    Ok(bytes.to_vec())
}

// -- Connection status --

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeStatus {
    node_id: String,
    has_relay: bool,
    relay_url: Option<String>,
    follow_count: usize,
    follower_count: usize,
}

#[tauri::command]
async fn get_node_status(state: State<'_, Arc<AppState>>) -> Result<NodeStatus, String> {
    let addr = state.endpoint.addr();
    let relay_url = addr.relay_urls().next().map(|u| u.to_string());
    let has_relay = relay_url.is_some();
    let feed = state.feed.lock().await;
    let follow_count = feed.subscriptions.len();
    let follower_count = state.storage.get_followers().map(|f| f.len()).unwrap_or(0);

    Ok(NodeStatus {
        node_id: state.endpoint.id().to_string(),
        has_relay,
        relay_url,
        follow_count,
        follower_count,
    })
}

// -- Direct Messages --

#[tauri::command]
async fn send_dm(
    state: State<'_, Arc<AppState>>,
    to: String,
    content: String,
    media: Option<Vec<MediaAttachment>>,
    reply_to: Option<String>,
) -> Result<StoredMessage, String> {
    log::info!(
        "[dm-cmd] send_dm called: to={}, content_len={}, media={:?}, reply_to={:?}",
        short_id(&to),
        content.len(),
        media.as_ref().map(|m| m.len()),
        reply_to
    );

    let my_id = state.endpoint.id().to_string();
    let msg_id = uuid::Uuid::new_v4().to_string();
    let timestamp = now_millis();

    let dm_msg = DirectMessage {
        id: msg_id.clone(),
        content: content.clone(),
        timestamp,
        media: media.clone().unwrap_or_default(),
        reply_to: reply_to.clone(),
    };

    let conv_id = Storage::conversation_id(&my_id, &to);
    let preview = if content.len() > 80 {
        format!("{}...", &content[..77])
    } else {
        content.clone()
    };

    let stored = StoredMessage {
        id: msg_id.clone(),
        conversation_id: conv_id,
        from_pubkey: my_id.clone(),
        to_pubkey: to.clone(),
        content,
        timestamp,
        media: media.unwrap_or_default(),
        read: false,
        delivered: false,
        reply_to,
    };

    // Store locally -- conversation first (FK constraint), then message
    state
        .storage
        .upsert_conversation(&to, &my_id, timestamp, &preview)
        .map_err(|e| {
            log::error!("[dm-cmd] upsert_conversation error: {e}");
            e.to_string()
        })?;
    state.storage.insert_dm_message(&stored).map_err(|e| {
        log::error!("[dm-cmd] insert_dm_message error: {e}");
        e.to_string()
    })?;

    log::info!("[dm-cmd] stored message {} locally", short_id(&msg_id));

    // Send async
    let endpoint = state.endpoint.clone();
    let dm_handler = state.dm.clone();
    let to_clone = to.clone();
    tokio::spawn(async move {
        log::info!("[dm-cmd] async send starting to {}", short_id(&to_clone));
        match dm_handler.send_dm(&endpoint, &to_clone, dm_msg).await {
            Ok(()) => log::info!("[dm-cmd] async send completed to {}", short_id(&to_clone)),
            Err(e) => log::error!("[dm-cmd] async send failed to {}: {e}", short_id(&to_clone)),
        }
    });

    Ok(stored)
}

#[tauri::command]
async fn get_conversations(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<ConversationMeta>, String> {
    let convos = state
        .storage
        .get_conversations()
        .map_err(|e| e.to_string())?;
    log::info!("[dm-cmd] get_conversations: {} conversations", convos.len());
    Ok(convos)
}

#[tauri::command]
async fn get_dm_messages(
    state: State<'_, Arc<AppState>>,
    peer_pubkey: String,
    limit: Option<usize>,
    before: Option<u64>,
) -> Result<Vec<StoredMessage>, String> {
    let my_id = state.endpoint.id().to_string();
    let conv_id = Storage::conversation_id(&my_id, &peer_pubkey);
    let msgs = state
        .storage
        .get_dm_messages(&conv_id, limit.unwrap_or(50), before)
        .map_err(|e| e.to_string())?;
    log::info!(
        "[dm-cmd] get_dm_messages: peer={}, conv={}, {} messages",
        short_id(&peer_pubkey),
        short_id(&conv_id),
        msgs.len()
    );
    Ok(msgs)
}

#[tauri::command]
async fn mark_dm_read(state: State<'_, Arc<AppState>>, peer_pubkey: String) -> Result<(), String> {
    let my_id = state.endpoint.id().to_string();
    state
        .storage
        .mark_conversation_read(&peer_pubkey, &my_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_dm_message(
    state: State<'_, Arc<AppState>>,
    message_id: String,
) -> Result<(), String> {
    state
        .storage
        .delete_dm_message(&message_id)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn flush_dm_outbox(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let peers = state
        .storage
        .get_all_outbox_peers()
        .map_err(|e| e.to_string())?;
    let endpoint = state.endpoint.clone();
    let dm_handler = state.dm.clone();

    let mut total_sent = 0u32;
    let mut total_failed = 0u32;
    for peer in peers {
        match dm_handler.flush_outbox_for_peer(&endpoint, &peer).await {
            Ok((sent, failed)) => {
                total_sent += sent;
                total_failed += failed;
            }
            Err(e) => {
                log::error!("[dm-outbox] flush error for {}: {e}", short_id(&peer));
                total_failed += 1;
            }
        }
    }

    Ok(serde_json::json!({
        "sent": total_sent,
        "failed": total_failed,
    }))
}

#[tauri::command]
async fn get_unread_dm_count(state: State<'_, Arc<AppState>>) -> Result<u32, String> {
    state
        .storage
        .get_total_unread_count()
        .map_err(|e| e.to_string())
}

/// Send a lightweight DM signal (typing indicator or read receipt).
/// Does not create a stored message -- just encrypts and sends over QUIC.
#[tauri::command]
async fn send_dm_signal(
    state: State<'_, Arc<AppState>>,
    to: String,
    signal_type: String,
    message_id: Option<String>,
) -> Result<(), String> {
    use iroh_social_types::DmPayload;

    let payload = match signal_type.as_str() {
        "typing" => DmPayload::Typing,
        "read" => {
            let id = message_id.ok_or("message_id required for read signal")?;
            DmPayload::Read { message_id: id }
        }
        other => return Err(format!("unknown signal type: {other}")),
    };

    let dm_handler = state.dm.clone();
    let endpoint = state.endpoint.clone();

    // Best-effort: don't fail if peer is offline
    tokio::spawn(async move {
        if let Err(e) = dm_handler.send_signal(&endpoint, &to, payload).await {
            log::info!(
                "[dm-signal] failed to send {signal_type} to {}: {e}",
                short_id(&to)
            );
        }
    });

    Ok(())
}

// -- Bookmarks --

#[tauri::command]
async fn toggle_bookmark(state: State<'_, Arc<AppState>>, post_id: String) -> Result<bool, String> {
    state
        .storage
        .toggle_bookmark(&post_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn is_bookmarked(state: State<'_, Arc<AppState>>, post_id: String) -> Result<bool, String> {
    state
        .storage
        .is_bookmarked(&post_id)
        .map_err(|e| e.to_string())
}

// -- Mute / Block --

#[tauri::command]
async fn mute_user(state: State<'_, Arc<AppState>>, pubkey: String) -> Result<(), String> {
    state.storage.mute_user(&pubkey).map_err(|e| e.to_string())
}

#[tauri::command]
async fn unmute_user(state: State<'_, Arc<AppState>>, pubkey: String) -> Result<(), String> {
    state
        .storage
        .unmute_user(&pubkey)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn is_muted(state: State<'_, Arc<AppState>>, pubkey: String) -> Result<bool, String> {
    state.storage.is_muted(&pubkey).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_muted_pubkeys(state: State<'_, Arc<AppState>>) -> Result<Vec<String>, String> {
    state.storage.get_muted_pubkeys().map_err(|e| e.to_string())
}

#[tauri::command]
async fn block_user(state: State<'_, Arc<AppState>>, pubkey: String) -> Result<(), String> {
    // Block also unfollows
    let is_following = state
        .storage
        .get_follows()
        .map_err(|e| e.to_string())?
        .iter()
        .any(|f| f.pubkey == pubkey);

    if is_following {
        state.storage.unfollow(&pubkey).map_err(|e| e.to_string())?;
        let mut feed = state.feed.lock().await;
        feed.unfollow_user(&pubkey);
    }

    state.storage.block_user(&pubkey).map_err(|e| e.to_string())
}

#[tauri::command]
async fn unblock_user(state: State<'_, Arc<AppState>>, pubkey: String) -> Result<(), String> {
    state
        .storage
        .unblock_user(&pubkey)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn is_blocked(state: State<'_, Arc<AppState>>, pubkey: String) -> Result<bool, String> {
    state.storage.is_blocked(&pubkey).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_blocked_pubkeys(state: State<'_, Arc<AppState>>) -> Result<Vec<String>, String> {
    state
        .storage
        .get_blocked_pubkeys()
        .map_err(|e| e.to_string())
}

// -- Setup --

fn load_or_create_key(path: &std::path::Path) -> SecretKey {
    if path.exists() {
        let bytes = std::fs::read(path).expect("failed to read identity key");
        let bytes: [u8; 32] = bytes.try_into().expect("invalid key length");
        SecretKey::from_bytes(&bytes)
    } else {
        let mut key_bytes = [0u8; 32];
        getrandom::fill(&mut key_bytes).expect("failed to generate random key");
        let key = SecretKey::from_bytes(&key_bytes);
        std::fs::write(path, key.to_bytes()).expect("failed to write identity key");
        key
    }
}

/// Startup sync: sync all posts/interactions from a followed peer.
async fn sync_peer_posts(
    endpoint: &Endpoint,
    storage: &Arc<Storage>,
    pubkey: &str,
    my_id: &str,
    handle: &AppHandle,
) {
    let target: iroh::EndpointId = match pubkey.parse() {
        Ok(t) => t,
        Err(_) => return,
    };

    for attempt in 1..=3 {
        log::info!(
            "[startup-sync] syncing from {} (attempt {}/3)...",
            short_id(pubkey),
            attempt,
        );
        let start = std::time::Instant::now();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            sync::sync_from_peer(endpoint, storage, target, pubkey),
        )
        .await;
        let elapsed = start.elapsed();

        match result {
            Ok(Ok(sync_result)) => {
                let stored = process_sync_result(
                    storage,
                    pubkey,
                    &sync_result,
                    "startup-sync",
                    my_id,
                    handle,
                );

                if stored > 0 || sync_result.profile.is_some() {
                    let _ = handle.emit("feed-updated", ());
                }
                log::info!(
                    "[startup-sync] stored {stored}/{} posts from {} in {:.1}s (mode={:?})",
                    sync_result.posts.len(),
                    short_id(pubkey),
                    elapsed.as_secs_f64(),
                    sync_result.mode,
                );
                return;
            }
            Ok(Err(e)) => {
                log::error!(
                    "[startup-sync] attempt {attempt} failed for {} after {:.1}s: {e:?}",
                    short_id(pubkey),
                    elapsed.as_secs_f64()
                );
            }
            Err(_) => {
                log::error!(
                    "[startup-sync] attempt {attempt} timed out for {} after {:.1}s",
                    short_id(pubkey),
                    elapsed.as_secs_f64()
                );
            }
        }

        if attempt < 3 {
            let delay = attempt as u64 * 5;
            log::info!(
                "[startup-sync] retrying {} in {delay}s...",
                short_id(pubkey)
            );
            tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    // Desktop-only plugins
    #[cfg(not(mobile))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            for arg in &argv {
                if arg.starts_with("iroh-social://") {
                    let _ = app.emit("deep-link-received", vec![arg.clone()]);
                    break;
                }
            }
        }));
    }

    // Mobile-only plugins
    #[cfg(mobile)]
    {
        builder = builder
            .plugin(tauri_plugin_barcode_scanner::init())
            .plugin(tauri_plugin_haptics::init());
    }

    builder
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: None,
                    }),
                ])
                .build(),
        )
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_os::init())
        .setup(|app| {
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                if let Err(e) = app.deep_link().register_all() {
                    log::error!("[setup] failed to register deep link schemes: {e}");
                }
            }

            let handle = app.handle().clone();

            let data_dir = handle
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            std::fs::create_dir_all(&data_dir).expect("failed to create app data dir");
            log::info!("[setup] data dir: {}", data_dir.display());

            let secret_key = load_or_create_key(&data_dir.join("identity.key"));
            let db_path = data_dir.join("social.db");
            let storage = Arc::new(Storage::open(&db_path).expect("failed to open database"));
            log::info!("[setup] database opened");

            let follows = storage.get_follows().unwrap_or_default();
            log::info!("[setup] loaded {} follows", follows.len());

            let secret_key_bytes = secret_key.to_bytes();
            let storage_clone = storage.clone();
            tauri::async_runtime::spawn(async move {
                log::info!("[setup] binding iroh endpoint...");
                let endpoint = Endpoint::builder()
                    .secret_key(secret_key)
                    .alpns(vec![
                        iroh_blobs::ALPN.to_vec(),
                        iroh_gossip::ALPN.to_vec(),
                        sync::SYNC_ALPN.to_vec(),
                        DM_ALPN.to_vec(),
                    ])
                    .bind()
                    .await
                    .expect("failed to bind iroh endpoint");

                log::info!("[setup] Node ID: {}", endpoint.id());
                log::info!("[setup] addr (immediate): {:?}", endpoint.addr());

                // Log relay address after it has time to connect
                let ep_clone = endpoint.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    log::info!("[setup] addr (after 3s): {:?}", ep_clone.addr());
                });

                // On Android, iroh cannot detect network changes natively.
                // Periodically notify the endpoint so it re-discovers interfaces and relay.
                #[cfg(target_os = "android")]
                {
                    let ep_net = endpoint.clone();
                    tokio::spawn(async move {
                        // Initial kick to discover the network right away
                        ep_net.network_change().await;
                        log::info!("[android-net] initial network_change() sent");
                        loop {
                            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                            ep_net.network_change().await;
                        }
                    });
                }

                let blobs_dir = data_dir.join("blobs");
                let store = FsStore::load(&blobs_dir)
                    .await
                    .expect("failed to open blob store");
                log::info!("[setup] blob store opened at {}", blobs_dir.display());

                let blobs = BlobsProtocol::new(&store, None);
                let gossip = Gossip::builder().spawn(endpoint.clone());
                log::info!("[setup] gossip started");

                let node_id_str = endpoint.id().to_string();
                let sync_handler =
                    sync::SyncHandler::new(storage_clone.clone(), node_id_str.clone());
                let dm_handler = DmHandler::new(
                    storage_clone.clone(),
                    handle.clone(),
                    secret_key_bytes,
                    endpoint.id().to_string(),
                );

                let router = Router::builder(endpoint.clone())
                    .accept(iroh_blobs::ALPN, blobs.clone())
                    .accept(iroh_gossip::ALPN, gossip.clone())
                    .accept(sync::SYNC_ALPN, sync_handler)
                    .accept(DM_ALPN, dm_handler.clone())
                    .spawn();
                log::info!("[setup] router spawned");

                let mut feed = FeedManager::new(
                    gossip,
                    endpoint.clone(),
                    storage_clone.clone(),
                    handle.clone(),
                );

                if let Err(e) = feed.start_own_feed().await {
                    log::error!("[setup] failed to start own feed: {e}");
                } else {
                    log::info!("[setup] own gossip feed started");
                }

                if let Ok(Some(profile)) = storage_clone.get_profile(&node_id_str) {
                    if let Err(e) = feed.broadcast_profile(&profile).await {
                        log::error!("[setup] failed to broadcast profile: {e}");
                    } else {
                        log::info!("[setup] broadcast profile: {}", profile.display_name);
                    }
                }

                for f in &follows {
                    log::info!("[setup] resubscribing to {}...", short_id(&f.pubkey));
                    if let Err(e) = feed.follow_user(f.pubkey.clone()).await {
                        log::error!(
                            "[setup] failed to resubscribe to {}: {e}",
                            short_id(&f.pubkey)
                        );
                    } else {
                        log::info!("[setup] resubscribed to {}", short_id(&f.pubkey));
                    }
                }

                // Concurrent startup sync with semaphore for bounded parallelism
                let sync_endpoint = endpoint.clone();
                let sync_storage = storage_clone.clone();
                let sync_follows = follows.clone();
                let sync_handle = handle.clone();
                let sync_my_id = endpoint.id().to_string();
                tokio::spawn(async move {
                    // Wait for relay to connect before attempting sync
                    log::info!("[startup-sync] waiting for relay connectivity...");
                    let mut has_relay = false;
                    for i in 0..10 {
                        let addr = sync_endpoint.addr();
                        if addr.relay_urls().next().is_some() {
                            log::info!("[startup-sync] relay connected after {}s", i);
                            has_relay = true;
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                    if !has_relay {
                        log::error!("[startup-sync] no relay after 10s, attempting sync anyway");
                    }

                    // Additional delay to let remote peers finish their own startup
                    log::info!("[startup-sync] waiting 5s for peers to be ready...");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                    let semaphore = Arc::new(tokio::sync::Semaphore::new(5));
                    let mut join_set = tokio::task::JoinSet::new();

                    for f in sync_follows {
                        let ep = sync_endpoint.clone();
                        let st = sync_storage.clone();
                        let hdl = sync_handle.clone();
                        let sem = semaphore.clone();
                        let mid = sync_my_id.clone();
                        join_set.spawn(async move {
                            let _permit = sem.acquire().await;
                            sync_peer_posts(&ep, &st, &f.pubkey, &mid, &hdl).await;
                        });
                    }

                    while let Some(result) = join_set.join_next().await {
                        if let Err(e) = result {
                            log::error!("[startup-sync] task panicked: {e}");
                        }
                    }
                    log::info!("[startup-sync] done");
                });

                // Background drip sync: periodically syncs each followed user
                let drip_endpoint = endpoint.clone();
                let drip_storage = storage_clone.clone();
                let drip_handle = handle.clone();
                let drip_my_id = endpoint.id().to_string();
                tokio::spawn(async move {
                    // Wait for startup sync to mostly complete before starting drip
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;

                    loop {
                        let follows = drip_storage.get_follows().unwrap_or_default();
                        let mut any_work = false;

                        for f in &follows {
                            let target: iroh::EndpointId = match f.pubkey.parse() {
                                Ok(t) => t,
                                Err(_) => continue,
                            };

                            log::info!("[drip-sync] syncing {}", short_id(&f.pubkey),);

                            let result = tokio::time::timeout(
                                std::time::Duration::from_secs(30),
                                sync::sync_from_peer(
                                    &drip_endpoint,
                                    &drip_storage,
                                    target,
                                    &f.pubkey,
                                ),
                            )
                            .await;

                            match result {
                                Ok(Ok(sync_result)) => {
                                    if sync_result.posts.is_empty()
                                        && sync_result.interactions.is_empty()
                                    {
                                        log::info!(
                                            "[drip-sync] {} up to date",
                                            short_id(&f.pubkey),
                                        );
                                        continue;
                                    }

                                    let stored = process_sync_result(
                                        &drip_storage,
                                        &f.pubkey,
                                        &sync_result,
                                        "drip-sync",
                                        &drip_my_id,
                                        &drip_handle,
                                    );

                                    if stored > 0 {
                                        any_work = true;
                                        let _ = drip_handle.emit("feed-updated", ());
                                    }

                                    log::info!(
                                        "[drip-sync] stored {stored}/{} posts from {} (mode={:?})",
                                        sync_result.posts.len(),
                                        short_id(&f.pubkey),
                                        sync_result.mode,
                                    );
                                }
                                Ok(Err(e)) => {
                                    log::error!(
                                        "[drip-sync] failed for {}: {e}",
                                        short_id(&f.pubkey)
                                    );
                                }
                                Err(_) => {
                                    log::error!(
                                        "[drip-sync] timed out for {}",
                                        short_id(&f.pubkey)
                                    );
                                }
                            }

                            // Pace between peers
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }

                        // Wait longer between full rounds
                        let delay = if any_work { 30 } else { 120 };
                        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                    }
                });

                // Spawn DM outbox flush task (every 60s)
                let outbox_dm = dm_handler.clone();
                let outbox_ep = endpoint.clone();
                let outbox_storage = storage_clone.clone();
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                        let peers = match outbox_storage.get_all_outbox_peers() {
                            Ok(p) => p,
                            Err(e) => {
                                log::error!("[dm-outbox] failed to get peers: {e}");
                                continue;
                            }
                        };
                        for peer in peers {
                            match outbox_dm.flush_outbox_for_peer(&outbox_ep, &peer).await {
                                Ok((sent, _)) if sent > 0 => {
                                    log::info!(
                                        "[dm-outbox] flushed {sent} queued messages to {}",
                                        short_id(&peer)
                                    );
                                }
                                Err(e) => {
                                    log::error!(
                                        "[dm-outbox] flush error for {}: {e}",
                                        short_id(&peer)
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                });

                let state = Arc::new(AppState {
                    endpoint,
                    router,
                    blobs,
                    store,
                    storage: storage_clone,
                    feed: Arc::new(Mutex::new(feed)),
                    dm: dm_handler,
                    secret_key_bytes,
                });

                handle.manage(state);
                log::info!("[setup] app state ready");
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_node_id,
            get_my_profile,
            save_my_profile,
            get_remote_profile,
            create_post,
            delete_post,
            get_feed,
            get_notifications,
            get_unread_notification_count,
            mark_notifications_read,
            get_user_posts,
            sync_posts,
            get_sync_status,
            fetch_older_posts,
            like_post,
            unlike_post,
            repost,
            unrepost,
            get_post_counts,
            get_replies,
            get_post,
            follow_user,
            unfollow_user,
            update_follow_alias,
            get_follows,
            get_followers,
            add_blob,
            fetch_blob,
            add_blob_bytes,
            fetch_blob_bytes,
            get_node_status,
            send_dm,
            get_conversations,
            get_dm_messages,
            mark_dm_read,
            delete_dm_message,
            flush_dm_outbox,
            get_unread_dm_count,
            send_dm_signal,
            toggle_bookmark,
            is_bookmarked,
            mute_user,
            unmute_user,
            is_muted,
            get_muted_pubkeys,
            block_user,
            unblock_user,
            is_blocked,
            get_blocked_pubkeys,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
