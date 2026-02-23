mod gossip;
mod storage;
mod sync;

use crate::gossip::FeedManager;
use crate::storage::{FollowEntry, MediaAttachment, Post, Profile, Storage};
use iroh::{Endpoint, SecretKey, protocol::Router};
use iroh_blobs::{BlobsProtocol, HashAndFormat, store::fs::FsStore, ticket::BlobTicket};
use iroh_gossip::Gossip;
use rand::Rng;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Manager, State};
use tokio::sync::Mutex;

pub struct AppState {
    pub endpoint: Endpoint,
    pub router: Router,
    pub blobs: BlobsProtocol,
    pub store: FsStore,
    pub storage: Arc<Storage>,
    pub feed: FeedManager,
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

// -- Identity --

#[tauri::command]
async fn get_node_id(state: State<'_, Arc<Mutex<AppState>>>) -> Result<String, String> {
    let state = state.lock().await;
    Ok(state.endpoint.id().to_string())
}

// -- Profile --

#[tauri::command]
async fn get_my_profile(state: State<'_, Arc<Mutex<AppState>>>) -> Result<Option<Profile>, String> {
    let state = state.lock().await;
    state.storage.get_profile().map_err(|e| e.to_string())
}

#[tauri::command]
async fn save_my_profile(
    state: State<'_, Arc<Mutex<AppState>>>,
    display_name: String,
    bio: String,
) -> Result<(), String> {
    let state = state.lock().await;
    let profile = Profile {
        display_name,
        bio,
        avatar_hash: None,
    };
    state
        .storage
        .save_profile(&profile)
        .map_err(|e| e.to_string())?;
    state
        .feed
        .broadcast_profile(&profile)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn get_remote_profile(
    state: State<'_, Arc<Mutex<AppState>>>,
    pubkey: String,
) -> Result<Option<Profile>, String> {
    let state = state.lock().await;
    state
        .storage
        .get_remote_profile(&pubkey)
        .map_err(|e| e.to_string())
}

// -- Posts --

#[tauri::command]
async fn create_post(
    state: State<'_, Arc<Mutex<AppState>>>,
    content: String,
    media: Option<Vec<MediaAttachment>>,
) -> Result<Post, String> {
    let state = state.lock().await;
    let author = state.endpoint.id().to_string();
    let post = Post {
        id: format!("{:016x}", rand::thread_rng().r#gen::<u64>()),
        author,
        content,
        timestamp: now_millis(),
        media: media.unwrap_or_default(),
    };

    state
        .storage
        .insert_post(&post)
        .map_err(|e| e.to_string())?;
    state
        .feed
        .broadcast_post(&post)
        .await
        .map_err(|e| e.to_string())?;

    Ok(post)
}

#[tauri::command]
async fn get_feed(
    state: State<'_, Arc<Mutex<AppState>>>,
    limit: Option<usize>,
    before: Option<u64>,
) -> Result<Vec<Post>, String> {
    let state = state.lock().await;
    state
        .storage
        .get_feed(limit.unwrap_or(50), before)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_user_posts(
    state: State<'_, Arc<Mutex<AppState>>>,
    pubkey: String,
    limit: Option<usize>,
) -> Result<Vec<Post>, String> {
    let state = state.lock().await;
    state
        .storage
        .get_posts_by_author(&pubkey, limit.unwrap_or(50))
        .map_err(|e| e.to_string())
}

// -- Sync (pull history from peers) --

#[tauri::command]
async fn sync_posts(
    state: State<'_, Arc<Mutex<AppState>>>,
    pubkey: String,
    before: Option<u64>,
    limit: Option<u32>,
) -> Result<Vec<Post>, String> {
    let state = state.lock().await;
    let target: iroh::EndpointId = pubkey.parse().map_err(|e| format!("{e}"))?;

    let posts = sync::fetch_remote_posts(
        &state.endpoint,
        target,
        &pubkey,
        before,
        limit.unwrap_or(20),
    )
    .await
    .map_err(|e| e.to_string())?;

    // Store fetched posts locally
    for post in &posts {
        if let Err(e) = state.storage.insert_post(post) {
            eprintln!("[sync] failed to store post: {e}");
        }
    }

    Ok(posts)
}

// -- Follows --

#[tauri::command]
async fn follow_user(state: State<'_, Arc<Mutex<AppState>>>, pubkey: String) -> Result<(), String> {
    let mut state = state.lock().await;
    let entry = FollowEntry {
        pubkey: pubkey.clone(),
        alias: None,
        followed_at: now_millis(),
    };
    state.storage.follow(&entry).map_err(|e| e.to_string())?;
    state
        .feed
        .follow_user(pubkey.clone())
        .await
        .map_err(|e| e.to_string())?;

    // Sync existing posts from the followed user
    let target: iroh::EndpointId = pubkey.parse().map_err(|e| format!("{e}"))?;
    match sync::fetch_remote_posts(&state.endpoint, target, &pubkey, None, 50).await {
        Ok(posts) => {
            for post in &posts {
                if let Err(e) = state.storage.insert_post(post) {
                    eprintln!("[follow-sync] failed to store post: {e}");
                }
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
async fn unfollow_user(
    state: State<'_, Arc<Mutex<AppState>>>,
    pubkey: String,
) -> Result<(), String> {
    let mut state = state.lock().await;
    state.storage.unfollow(&pubkey).map_err(|e| e.to_string())?;
    state.feed.unfollow_user(&pubkey);
    Ok(())
}

#[tauri::command]
async fn get_follows(state: State<'_, Arc<Mutex<AppState>>>) -> Result<Vec<FollowEntry>, String> {
    let state = state.lock().await;
    state.storage.get_follows().map_err(|e| e.to_string())
}

// -- Blobs (media) --

#[tauri::command]
async fn add_blob(
    state: State<'_, Arc<Mutex<AppState>>>,
    content: String,
) -> Result<serde_json::Value, String> {
    let state = state.lock().await;
    let tag = state
        .store
        .add_slice(content.as_bytes())
        .await
        .map_err(|e| e.to_string())?;

    let addr = state.endpoint.addr();
    let ticket = BlobTicket::new(addr, tag.hash, tag.format);

    Ok(serde_json::json!({
        "hash": tag.hash.to_string(),
        "ticket": ticket.to_string(),
    }))
}

#[tauri::command]
async fn fetch_blob(
    state: State<'_, Arc<Mutex<AppState>>>,
    ticket: String,
) -> Result<String, String> {
    let ticket: BlobTicket = ticket.parse().map_err(|e| format!("{e}"))?;
    let state = state.lock().await;

    let conn = state
        .endpoint
        .connect(ticket.addr().clone(), iroh_blobs::ALPN)
        .await
        .map_err(|e| e.to_string())?;

    let hash_and_format: HashAndFormat = ticket.hash_and_format();
    state
        .blobs
        .remote()
        .fetch(conn, hash_and_format)
        .await
        .map_err(|e| e.to_string())?;

    let bytes = state
        .store
        .get_bytes(ticket.hash())
        .await
        .map_err(|e| e.to_string())?;

    String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string())
}

#[tauri::command]
async fn add_blob_bytes(
    state: State<'_, Arc<Mutex<AppState>>>,
    data: Vec<u8>,
) -> Result<serde_json::Value, String> {
    let state = state.lock().await;
    let tag = state
        .store
        .add_slice(&data)
        .await
        .map_err(|e| e.to_string())?;

    let addr = state.endpoint.addr();
    let ticket = BlobTicket::new(addr, tag.hash, tag.format);

    Ok(serde_json::json!({
        "hash": tag.hash.to_string(),
        "ticket": ticket.to_string(),
    }))
}

#[tauri::command]
async fn fetch_blob_bytes(
    state: State<'_, Arc<Mutex<AppState>>>,
    ticket: String,
) -> Result<Vec<u8>, String> {
    let ticket: BlobTicket = ticket.parse().map_err(|e| format!("{e}"))?;
    let state = state.lock().await;

    // Try local store first
    if let Ok(bytes) = state.store.get_bytes(ticket.hash()).await {
        return Ok(bytes.to_vec());
    }

    // Fetch from remote peer
    let conn = state
        .endpoint
        .connect(ticket.addr().clone(), iroh_blobs::ALPN)
        .await
        .map_err(|e| e.to_string())?;

    let hash_and_format: HashAndFormat = ticket.hash_and_format();
    state
        .blobs
        .remote()
        .fetch(conn, hash_and_format)
        .await
        .map_err(|e| e.to_string())?;

    let bytes = state
        .store
        .get_bytes(ticket.hash())
        .await
        .map_err(|e| e.to_string())?;

    Ok(bytes.to_vec())
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

            let secret_key = load_or_create_key(&data_dir.join("identity.key"));
            let db_path = data_dir.join("social.redb");
            let storage = Arc::new(Storage::open(&db_path).expect("failed to open database"));

            let follows = storage.get_follows().unwrap_or_default();

            let storage_clone = storage.clone();
            tauri::async_runtime::spawn(async move {
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

                println!("Node ID: {}", endpoint.id());

                let blobs_dir = data_dir.join("blobs");
                let store = FsStore::load(&blobs_dir)
                    .await
                    .expect("failed to open blob store");
                let blobs = BlobsProtocol::new(&store, None);
                let gossip = Gossip::builder().spawn(endpoint.clone());

                let sync_handler = sync::SyncHandler::new(storage_clone.clone());

                let router = Router::builder(endpoint.clone())
                    .accept(iroh_blobs::ALPN.to_vec(), blobs.clone())
                    .accept(iroh_gossip::ALPN.to_vec(), gossip.clone())
                    .accept(sync::SYNC_ALPN.to_vec(), sync_handler)
                    .spawn();

                let mut feed = FeedManager::new(
                    gossip,
                    endpoint.clone(),
                    storage_clone.clone(),
                    handle.clone(),
                );

                if let Err(e) = feed.start_own_feed().await {
                    eprintln!("Failed to start own feed: {e}");
                }

                if let Ok(Some(profile)) = storage_clone.get_profile() {
                    if let Err(e) = feed.broadcast_profile(&profile).await {
                        eprintln!("Failed to broadcast profile on startup: {e}");
                    }
                }

                for f in &follows {
                    if let Err(e) = feed.follow_user(f.pubkey.clone()).await {
                        eprintln!("Failed to resubscribe to {}: {e}", f.pubkey);
                    }
                }

                // Sync historical posts from followed users in the background
                let sync_endpoint = endpoint.clone();
                let sync_storage = storage_clone.clone();
                let sync_follows = follows.clone();
                tokio::spawn(async move {
                    for f in &sync_follows {
                        let target: iroh::EndpointId = match f.pubkey.parse() {
                            Ok(t) => t,
                            Err(_) => continue,
                        };
                        match sync::fetch_remote_posts(&sync_endpoint, target, &f.pubkey, None, 50)
                            .await
                        {
                            Ok(posts) => {
                                for post in &posts {
                                    let _ = sync_storage.insert_post(post);
                                }
                                println!(
                                    "[startup-sync] synced {} posts from {}",
                                    posts.len(),
                                    &f.pubkey[..8]
                                );
                            }
                            Err(e) => {
                                eprintln!(
                                    "[startup-sync] failed to sync from {}: {e}",
                                    &f.pubkey[..8]
                                );
                            }
                        }
                    }
                });

                let state = Arc::new(Mutex::new(AppState {
                    endpoint,
                    router,
                    blobs,
                    store,
                    storage: storage_clone,
                    feed,
                }));

                handle.manage(state);
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_node_id,
            get_my_profile,
            save_my_profile,
            get_remote_profile,
            create_post,
            get_feed,
            get_user_posts,
            sync_posts,
            follow_user,
            unfollow_user,
            get_follows,
            add_blob,
            fetch_blob,
            add_blob_bytes,
            fetch_blob_bytes,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
