mod gossip;
mod storage;
mod sync;

use crate::gossip::FeedManager;
use crate::storage::Storage;
use iroh::{Endpoint, SecretKey, protocol::Router};
use iroh_blobs::{BlobsProtocol, HashAndFormat, store::fs::FsStore, ticket::BlobTicket};
use iroh_gossip::Gossip;
use iroh_social_types::{
    FollowEntry, FollowerEntry, MAX_BLOB_SIZE, MediaAttachment, Post, Profile, now_millis,
    validate_post,
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
) -> Result<Vec<Post>, String> {
    state
        .storage
        .get_posts_by_author(&pubkey, limit.unwrap_or(50), before)
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

    println!("[sync] requesting posts from {}...", &pubkey[..8]);
    let (posts, profile) =
        sync::fetch_remote_posts(&endpoint, target, &pubkey, before, limit.unwrap_or(20))
            .await
            .map_err(|e| e.to_string())?;

    println!(
        "[sync] received {} posts from {}",
        posts.len(),
        &pubkey[..8]
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
    println!("[follow] following {}...", &pubkey[..8]);
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
    println!("[follow] subscribed to gossip for {}", &pubkey[..8]);

    // Sync existing posts from the followed user (lock dropped, no blocking)
    println!("[follow] syncing posts from {}...", &pubkey[..8]);
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
                &pubkey[..8]
            );
        }
        Err(e) => {
            eprintln!("[follow-sync] failed to sync from {}: {e}", &pubkey[..8]);
        }
    }

    Ok(())
}

#[tauri::command]
async fn unfollow_user(state: State<'_, Arc<AppState>>, pubkey: String) -> Result<(), String> {
    println!("[follow] unfollowing {}...", &pubkey[..8]);
    state.storage.unfollow(&pubkey).map_err(|e| e.to_string())?;
    let mut feed = state.feed.lock().await;
    feed.unfollow_user(&pubkey);
    println!("[follow] unfollowed {}", &pubkey[..8]);
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
            &pubkey[..8],
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
                    &pubkey[..8],
                    elapsed.as_secs_f64()
                );
                return;
            }
            Ok(Err(e)) => {
                eprintln!(
                    "[startup-sync] attempt {attempt} failed for {} after {:.1}s: {e:?}",
                    &pubkey[..8],
                    elapsed.as_secs_f64()
                );
            }
            Err(_) => {
                eprintln!(
                    "[startup-sync] attempt {attempt} timed out for {} after {:.1}s",
                    &pubkey[..8],
                    elapsed.as_secs_f64()
                );
            }
        }

        if attempt < 3 {
            let delay = attempt as u64 * 5;
            println!("[startup-sync] retrying {} in {delay}s...", &pubkey[..8]);
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

            let storage_clone = storage.clone();
            tauri::async_runtime::spawn(async move {
                println!("[setup] binding iroh endpoint...");
                let endpoint = Endpoint::builder()
                    .secret_key(secret_key)
                    .alpns(vec![
                        iroh_blobs::ALPN.to_vec(),
                        iroh_gossip::ALPN.to_vec(),
                        sync::SYNC_ALPN.to_vec(),
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

                let router = Router::builder(endpoint.clone())
                    .accept(iroh_blobs::ALPN, blobs.clone())
                    .accept(iroh_gossip::ALPN, gossip.clone())
                    .accept(sync::SYNC_ALPN, sync_handler)
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
                    println!("[setup] resubscribing to {}...", &f.pubkey[..8]);
                    if let Err(e) = feed.follow_user(f.pubkey.clone()).await {
                        eprintln!("[setup] failed to resubscribe to {}: {e}", &f.pubkey[..8]);
                    } else {
                        println!("[setup] resubscribed to {}", &f.pubkey[..8]);
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

                let state = Arc::new(AppState {
                    endpoint,
                    router,
                    blobs,
                    store,
                    storage: storage_clone,
                    feed: Arc::new(Mutex::new(feed)),
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
