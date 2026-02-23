use iroh::{
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler, Router},
    Endpoint,
};
use iroh_blobs::{
    store::mem::MemStore,
    ticket::BlobTicket,
    BlobsProtocol, HashAndFormat,
};
use std::sync::Arc;
use tauri::{Manager, State};
use tokio::sync::Mutex;

pub struct AppState {
    pub endpoint: Endpoint,
    pub router: Router,
    pub blobs: BlobsProtocol,
    pub store: MemStore,
}

#[tauri::command]
async fn get_node_info(state: State<'_, Arc<Mutex<AppState>>>) -> Result<serde_json::Value, String> {
    let state = state.lock().await;
    let id = state.endpoint.id();
    let addr = state.endpoint.addr();

    Ok(serde_json::json!({
        "node_id": id.to_string(),
        "addrs": format!("{:?}", addr),
    }))
}

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
    let ticket: BlobTicket = ticket.parse().map_err(|e| format!("{e}") )?;
    let state = state.lock().await;

    // Connect to the remote node using the blobs ALPN
    let conn = state
        .endpoint
        .connect(ticket.addr().clone(), iroh_blobs::ALPN)
        .await
        .map_err(|e| e.to_string())?;

    // Fetch the blob via the remote API
    let hash_and_format: HashAndFormat = ticket.hash_and_format();
    state
        .blobs
        .remote()
        .fetch(conn, hash_and_format)
        .await
        .map_err(|e| e.to_string())?;

    // Read the blob content from the local store
    let bytes = state
        .store
        .get_bytes(ticket.hash())
        .await
        .map_err(|e| e.to_string())?;

    String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string())
}

#[tauri::command]
async fn send_message(
    state: State<'_, Arc<Mutex<AppState>>>,
    node_id: String,
    message: String,
) -> Result<String, String> {
    let state = state.lock().await;
    let target: iroh::EndpointId = node_id
        .parse()
        .map_err(|e| format!("{e}"))?;
    let addr = iroh::EndpointAddr::from(target);

    let conn = state
        .endpoint
        .connect(addr, b"iroh-tauri/echo/1")
        .await
        .map_err(|e| e.to_string())?;

    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
    send.write_all(message.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    send.finish().map_err(|e| e.to_string())?;

    let response = recv.read_to_end(65536).await.map_err(|e| e.to_string())?;
    String::from_utf8(response.to_vec()).map_err(|e| e.to_string())
}

#[derive(Debug, Clone)]
struct EchoHandler;

impl ProtocolHandler for EchoHandler {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let (mut send, mut recv) = conn.accept_bi().await?;
        let data = recv.read_to_end(65536).await.map_err(AcceptError::from_err)?;
        let msg = String::from_utf8_lossy(&data);
        println!("[echo] received: {msg}");
        let response = format!("echo: {msg}");
        send.write_all(response.as_bytes()).await.map_err(AcceptError::from_err)?;
        send.finish().map_err(AcceptError::from_err)?;
        Ok(())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let endpoint = Endpoint::builder()
                    .alpns(vec![
                        b"iroh-tauri/echo/1".to_vec(),
                        iroh_blobs::ALPN.to_vec(),
                    ])
                    .bind()
                    .await
                    .expect("failed to bind iroh endpoint");

                println!("Iroh node ID: {}", endpoint.id());

                let store = MemStore::new();
                let blobs = BlobsProtocol::new(store.as_ref(), None);

                let router = Router::builder(endpoint.clone())
                    .accept(b"iroh-tauri/echo/1".to_vec(), Arc::new(EchoHandler))
                    .accept(iroh_blobs::ALPN.to_vec(), blobs.clone())
                    .spawn();

                let state = Arc::new(Mutex::new(AppState {
                    endpoint,
                    router,
                    blobs,
                    store,
                }));

                handle.manage(state);
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_node_info,
            add_blob,
            fetch_blob,
            send_message,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
