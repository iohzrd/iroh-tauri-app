use crate::state::{AppState, FrontendSyncResult, SyncStatus};
use crate::storage::Storage;
use iroh_social_types::{
    parse_mentions, short_id, validate_interaction, validate_post, verify_interaction_signature,
    verify_post_signature,
};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

/// Validate and store posts/interactions/profile from a sync result.
/// Returns the number of posts actually stored.
pub(crate) fn process_sync_result(
    storage: &Storage,
    pubkey: &str,
    result: &crate::sync::SyncResult,
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

#[tauri::command]
pub async fn sync_posts(
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
    pubkey: String,
) -> Result<FrontendSyncResult, String> {
    let endpoint = state.endpoint.clone();
    let storage = state.storage.clone();
    let target: iroh::EndpointId = pubkey.parse().map_err(|e| format!("{e}"))?;

    let my_id = state.endpoint.id().to_string();
    let result = crate::sync::sync_from_peer(&endpoint, &storage, target, &pubkey)
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

#[tauri::command]
pub async fn get_sync_status(
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
pub async fn fetch_older_posts(
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
    pubkey: String,
) -> Result<FrontendSyncResult, String> {
    sync_posts(app_handle, state, pubkey).await
}
