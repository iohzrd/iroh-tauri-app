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
    ConversationMeta, DM_ALPN, DirectMessage, FollowEntry, FollowerEntry, MAX_BLOB_SIZE,
    MediaAttachment, Post, Profile, StoredMessage, now_millis, short_id, validate_post,
    validate_profile,
};
use rand::Rng;
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
}

// -- Identity --

#[tauri::command]
async fn get_node_id(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    Ok(state.endpoint.id().to_string())
}

// -- Profile --

#[tauri::command]
async fn get_my_profile(state: State<'_, Arc<AppState>>) -> Result<Option<Profile>, String> {
    state.storage.get_profile().map_err(|e| e.to_string())
}

#[tauri::command]
async fn save_my_profile(
    state: State<'_, Arc<AppState>>,
    display_name: String,
    bio: String,
    avatar_hash: Option<String>,
    avatar_ticket: Option<String>,
) -> Result<(), String> {
    let profile = Profile {
        display_name: display_name.clone(),
        bio: bio.clone(),
        avatar_hash,
        avatar_ticket,
    };
    validate_profile(&profile)?;
    state
        .storage
        .save_profile(&profile)
        .map_err(|e| e.to_string())?;
    println!("[profile] saved profile: {display_name}");
    let feed = state.feed.lock().await;
    feed.broadcast_profile(&profile)
        .await
        .map_err(|e| e.to_string())?;
    println!("[profile] broadcast profile update");
    Ok(())
}

#[tauri::command]
async fn get_remote_profile(
    state: State<'_, Arc<AppState>>,
    pubkey: String,
) -> Result<Option<Profile>, String> {
    state
        .storage
        .get_remote_profile(&pubkey)
        .map_err(|e| e.to_string())
}

// -- Posts --

#[tauri::command]
async fn create_post(
    state: State<'_, Arc<AppState>>,
    content: String,
    media: Option<Vec<MediaAttachment>>,
) -> Result<Post, String> {
    let author = state.endpoint.id().to_string();
    let media_count = media.as_ref().map_or(0, |m| m.len());
    let post = Post {
        id: format!("{:016x}", rand::thread_rng().r#gen::<u64>()),
        author,
        content,
        timestamp: now_millis(),
        media: media.unwrap_or_default(),
    };

    validate_post(&post)?;

    state
        .storage
        .insert_post(&post)
        .map_err(|e| e.to_string())?;
    println!(
        "[post] created post {} ({} media attachments)",
        &post.id, media_count
    );
    let feed = state.feed.lock().await;
    feed.broadcast_post(&post)
        .await
        .map_err(|e| e.to_string())?;
    println!("[post] broadcast post {}", &post.id);

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
    println!("[post] delete post {id}: removed={removed}");
    let feed = state.feed.lock().await;
    feed.broadcast_delete(&id, &my_id)
        .await
        .map_err(|e| e.to_string())?;
    println!("[post] broadcast delete {id}");

    Ok(())
}

#[tauri::command]
async fn get_feed(
    state: State<'_, Arc<AppState>>,
    limit: Option<usize>,
    before: Option<u64>,
) -> Result<Vec<Post>, String> {
    let posts = state
        .storage
        .get_feed(limit.unwrap_or(50), before)
        .map_err(|e| e.to_string())?;
    println!("[feed] loaded {} posts", posts.len());
    Ok(posts)
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
            limit.unwrap_or(50),
            before,
            media_filter.as_deref(),
        )
        .map_err(|e| e.to_string())
}

// -- Sync (pull history from peers) --

#[tauri::command]
async fn sync_posts(
    state: State<'_, Arc<AppState>>,
    pubkey: String,
    before: Option<u64>,
    limit: Option<u32>,
) -> Result<Vec<Post>, String> {
    let endpoint = state.endpoint.clone();
    let storage = state.storage.clone();
    let target: iroh::EndpointId = pubkey.parse().map_err(|e| format!("{e}"))?;

    println!("[sync] requesting posts from {}...", short_id(&pubkey));
    let (posts, profile) =
        sync::fetch_remote_posts(&endpoint, target, &pubkey, before, limit.unwrap_or(20))
            .await
            .map_err(|e| e.to_string())?;

    println!(
        "[sync] received {} posts from {}",
        posts.len(),
        short_id(&pubkey)
    );

    // Store fetched posts and profile locally
    for post in &posts {
        if let Err(reason) = validate_post(post) {
            eprintln!("[sync] rejected post {}: {reason}", &post.id);
            continue;
        }
        if let Err(e) = storage.insert_post(post) {
            eprintln!("[sync] failed to store post: {e}");
        }
    }
    if let Some(profile) = &profile
        && let Err(e) = storage.save_remote_profile(&pubkey, profile)
    {
        eprintln!("[sync] failed to store profile: {e}");
    }

    Ok(posts)
}

// -- Follows --

#[tauri::command]
async fn follow_user(state: State<'_, Arc<AppState>>, pubkey: String) -> Result<(), String> {
    let my_id = state.endpoint.id().to_string();
    if pubkey == my_id {
        return Err("cannot follow yourself".to_string());
    }
    println!("[follow] following {}...", short_id(&pubkey));
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
    println!("[follow] subscribed to gossip for {}", short_id(&pubkey));

    // Sync existing posts from the followed user (lock dropped, no blocking)
    println!("[follow] syncing posts from {}...", short_id(&pubkey));
    let endpoint = state.endpoint.clone();
    let storage = state.storage.clone();
    let target: iroh::EndpointId = pubkey.parse().map_err(|e| format!("{e}"))?;
    match sync::fetch_remote_posts(&endpoint, target, &pubkey, None, 50).await {
        Ok((posts, profile)) => {
            for post in &posts {
                if let Err(reason) = validate_post(post) {
                    eprintln!("[follow-sync] rejected post {}: {reason}", &post.id);
                    continue;
                }
                if let Err(e) = storage.insert_post(post) {
                    eprintln!("[follow-sync] failed to store post: {e}");
                }
            }
            if let Some(profile) = &profile
                && let Err(e) = storage.save_remote_profile(&pubkey, profile)
            {
                eprintln!("[follow-sync] failed to store profile: {e}");
            }
            println!(
                "[follow-sync] synced {} posts from {}",
                posts.len(),
                short_id(&pubkey)
            );
        }
        Err(e) => {
            eprintln!(
                "[follow-sync] failed to sync from {}: {e}",
                short_id(&pubkey)
            );
        }
    }

    Ok(())
}

#[tauri::command]
async fn unfollow_user(state: State<'_, Arc<AppState>>, pubkey: String) -> Result<(), String> {
    println!("[follow] unfollowing {}...", short_id(&pubkey));
    state.storage.unfollow(&pubkey).map_err(|e| e.to_string())?;
    let mut feed = state.feed.lock().await;
    feed.unfollow_user(&pubkey);
    println!("[follow] unfollowed {}", short_id(&pubkey));
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
    println!("[blob] added text blob {}", tag.hash);

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

    println!("[blob] fetching text blob {}...", ticket.hash());
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

    println!(
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
    println!("[blob] added blob {} ({size} bytes)", tag.hash);

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
        println!(
            "[blob] found {} locally ({} bytes)",
            ticket.hash(),
            bytes.len()
        );
        return Ok(bytes.to_vec());
    }

    // Fetch from remote peer -- no lock held
    println!("[blob] fetching {} from remote...", ticket.hash());
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

    println!(
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
    println!(
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
        read: true,
        delivered: false,
        reply_to,
    };

    // Store locally -- conversation first (FK constraint), then message
    state
        .storage
        .upsert_conversation(&to, &my_id, timestamp, &preview)
        .map_err(|e| {
            eprintln!("[dm-cmd] upsert_conversation error: {e}");
            e.to_string()
        })?;
    state.storage.insert_dm_message(&stored).map_err(|e| {
        eprintln!("[dm-cmd] insert_dm_message error: {e}");
        e.to_string()
    })?;

    println!("[dm-cmd] stored message {} locally", short_id(&msg_id));

    // Send async
    let endpoint = state.endpoint.clone();
    let dm_handler = state.dm.clone();
    let to_clone = to.clone();
    tokio::spawn(async move {
        println!("[dm-cmd] async send starting to {}", short_id(&to_clone));
        match dm_handler.send_dm(&endpoint, &to_clone, dm_msg).await {
            Ok(()) => println!("[dm-cmd] async send completed to {}", short_id(&to_clone)),
            Err(e) => eprintln!("[dm-cmd] async send failed to {}: {e}", short_id(&to_clone)),
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
    println!("[dm-cmd] get_conversations: {} conversations", convos.len());
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
    println!(
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
                eprintln!("[dm-outbox] flush error for {}: {e}", short_id(&peer));
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

async fn sync_one_follow(
    endpoint: &Endpoint,
    storage: &Arc<Storage>,
    pubkey: &str,
    handle: &AppHandle,
) {
    let target: iroh::EndpointId = match pubkey.parse() {
        Ok(t) => t,
        Err(_) => return,
    };

    for attempt in 1..=3 {
        println!(
            "[startup-sync] syncing from {} (attempt {}/3)...",
            short_id(pubkey),
            attempt
        );
        let start = std::time::Instant::now();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            sync::fetch_remote_posts(endpoint, target, pubkey, None, 50),
        )
        .await;
        let elapsed = start.elapsed();

        match result {
            Ok(Ok((posts, profile))) => {
                for post in &posts {
                    if let Err(reason) = validate_post(post) {
                        eprintln!("[startup-sync] rejected post {}: {reason}", &post.id);
                        continue;
                    }
                    let _ = storage.insert_post(post);
                }
                if let Some(profile) = &profile {
                    let _ = storage.save_remote_profile(pubkey, profile);
                }
                if !posts.is_empty() || profile.is_some() {
                    let _ = handle.emit("feed-updated", ());
                }
                println!(
                    "[startup-sync] synced {} posts from {} in {:.1}s",
                    posts.len(),
                    short_id(pubkey),
                    elapsed.as_secs_f64()
                );
                return;
            }
            Ok(Err(e)) => {
                eprintln!(
                    "[startup-sync] attempt {attempt} failed for {} after {:.1}s: {e:?}",
                    short_id(pubkey),
                    elapsed.as_secs_f64()
                );
            }
            Err(_) => {
                eprintln!(
                    "[startup-sync] attempt {attempt} timed out for {} after {:.1}s",
                    short_id(pubkey),
                    elapsed.as_secs_f64()
                );
            }
        }

        if attempt < 3 {
            let delay = attempt as u64 * 5;
            println!(
                "[startup-sync] retrying {} in {delay}s...",
                short_id(pubkey)
            );
            tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();

            let data_dir = handle
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            std::fs::create_dir_all(&data_dir).expect("failed to create app data dir");
            println!("[setup] data dir: {}", data_dir.display());

            let secret_key = load_or_create_key(&data_dir.join("identity.key"));
            let db_path = data_dir.join("social.db");
            let storage = Arc::new(Storage::open(&db_path).expect("failed to open database"));
            println!("[setup] database opened");

            let follows = storage.get_follows().unwrap_or_default();
            println!("[setup] loaded {} follows", follows.len());

            let secret_key_bytes = secret_key.to_bytes();
            let storage_clone = storage.clone();
            tauri::async_runtime::spawn(async move {
                println!("[setup] binding iroh endpoint...");
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

                println!("[setup] Node ID: {}", endpoint.id());
                println!("[setup] addr (immediate): {:?}", endpoint.addr());

                // Log relay address after it has time to connect
                let ep_clone = endpoint.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    println!("[setup] addr (after 3s): {:?}", ep_clone.addr());
                });

                let blobs_dir = data_dir.join("blobs");
                let store = FsStore::load(&blobs_dir)
                    .await
                    .expect("failed to open blob store");
                println!("[setup] blob store opened at {}", blobs_dir.display());

                let blobs = BlobsProtocol::new(&store, None);
                let gossip = Gossip::builder().spawn(endpoint.clone());
                println!("[setup] gossip started");

                let sync_handler = sync::SyncHandler::new(storage_clone.clone());
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
                println!("[setup] router spawned");

                let mut feed = FeedManager::new(
                    gossip,
                    endpoint.clone(),
                    storage_clone.clone(),
                    handle.clone(),
                );

                if let Err(e) = feed.start_own_feed().await {
                    eprintln!("[setup] failed to start own feed: {e}");
                } else {
                    println!("[setup] own gossip feed started");
                }

                if let Ok(Some(profile)) = storage_clone.get_profile() {
                    if let Err(e) = feed.broadcast_profile(&profile).await {
                        eprintln!("[setup] failed to broadcast profile: {e}");
                    } else {
                        println!("[setup] broadcast profile: {}", profile.display_name);
                    }
                }

                for f in &follows {
                    println!("[setup] resubscribing to {}...", short_id(&f.pubkey));
                    if let Err(e) = feed.follow_user(f.pubkey.clone()).await {
                        eprintln!(
                            "[setup] failed to resubscribe to {}: {e}",
                            short_id(&f.pubkey)
                        );
                    } else {
                        println!("[setup] resubscribed to {}", short_id(&f.pubkey));
                    }
                }

                // Concurrent startup sync with semaphore for bounded parallelism
                let sync_endpoint = endpoint.clone();
                let sync_storage = storage_clone.clone();
                let sync_follows = follows.clone();
                let sync_handle = handle.clone();
                tokio::spawn(async move {
                    // Wait for relay to connect before attempting sync
                    println!("[startup-sync] waiting for relay connectivity...");
                    let mut has_relay = false;
                    for i in 0..10 {
                        let addr = sync_endpoint.addr();
                        if addr.relay_urls().next().is_some() {
                            println!("[startup-sync] relay connected after {}s", i);
                            has_relay = true;
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                    if !has_relay {
                        eprintln!("[startup-sync] no relay after 10s, attempting sync anyway");
                    }

                    // Additional delay to let remote peers finish their own startup
                    println!("[startup-sync] waiting 5s for peers to be ready...");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                    let semaphore = Arc::new(tokio::sync::Semaphore::new(5));
                    let mut join_set = tokio::task::JoinSet::new();

                    for f in sync_follows {
                        let ep = sync_endpoint.clone();
                        let st = sync_storage.clone();
                        let hdl = sync_handle.clone();
                        let sem = semaphore.clone();
                        join_set.spawn(async move {
                            let _permit = sem.acquire().await;
                            sync_one_follow(&ep, &st, &f.pubkey, &hdl).await;
                        });
                    }

                    while let Some(result) = join_set.join_next().await {
                        if let Err(e) = result {
                            eprintln!("[startup-sync] task panicked: {e}");
                        }
                    }
                    println!("[startup-sync] done");
                });

                // Spawn DM outbox flush task (every 60s)
                let outbox_dm = dm_handler.clone();
                let outbox_ep = endpoint.clone();
                let outbox_storage = storage_clone.clone();
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                        let peers = match outbox_storage.get_all_outbox_peers() {
                            Ok(p) => p,
                            Err(e) => {
                                eprintln!("[dm-outbox] failed to get peers: {e}");
                                continue;
                            }
                        };
                        for peer in peers {
                            match outbox_dm.flush_outbox_for_peer(&outbox_ep, &peer).await {
                                Ok((sent, _)) if sent > 0 => {
                                    println!(
                                        "[dm-outbox] flushed {sent} queued messages to {}",
                                        short_id(&peer)
                                    );
                                }
                                Err(e) => {
                                    eprintln!(
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
                });

                handle.manage(state);
                println!("[setup] app state ready");
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
            get_user_posts,
            sync_posts,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
