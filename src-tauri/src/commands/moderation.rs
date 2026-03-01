use crate::state::AppState;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn toggle_bookmark(
    state: State<'_, Arc<AppState>>,
    post_id: String,
) -> Result<bool, String> {
    state
        .storage
        .toggle_bookmark(&post_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn is_bookmarked(
    state: State<'_, Arc<AppState>>,
    post_id: String,
) -> Result<bool, String> {
    state
        .storage
        .is_bookmarked(&post_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mute_user(state: State<'_, Arc<AppState>>, pubkey: String) -> Result<(), String> {
    state.storage.mute_user(&pubkey).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn unmute_user(state: State<'_, Arc<AppState>>, pubkey: String) -> Result<(), String> {
    state
        .storage
        .unmute_user(&pubkey)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn is_muted(state: State<'_, Arc<AppState>>, pubkey: String) -> Result<bool, String> {
    state.storage.is_muted(&pubkey).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_muted_pubkeys(state: State<'_, Arc<AppState>>) -> Result<Vec<String>, String> {
    state.storage.get_muted_pubkeys().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn block_user(state: State<'_, Arc<AppState>>, pubkey: String) -> Result<(), String> {
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
pub async fn unblock_user(state: State<'_, Arc<AppState>>, pubkey: String) -> Result<(), String> {
    state
        .storage
        .unblock_user(&pubkey)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn is_blocked(state: State<'_, Arc<AppState>>, pubkey: String) -> Result<bool, String> {
    state.storage.is_blocked(&pubkey).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_blocked_pubkeys(state: State<'_, Arc<AppState>>) -> Result<Vec<String>, String> {
    state
        .storage
        .get_blocked_pubkeys()
        .map_err(|e| e.to_string())
}
