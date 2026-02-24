# Iroh Social

A decentralized peer-to-peer social network built with [Iroh](https://iroh.computer/), [Tauri 2](https://tauri.app/), and [SvelteKit 5](https://svelte.dev/). Successor to [follow](https://github.com/iohzrd/follow) and [identia](https://github.com/iohzrd/identia), rebuilt on iroh's QUIC transport with end-to-end encrypted messaging.

Every user runs their own node. Posts, profiles, and follows are stored locally. Peers exchange data directly -- no central server, no accounts, no passwords.

## How It Works

Each node gets a permanent cryptographic identity (stored in `identity.key`). Your public key is your Node ID -- share it with others so they can follow you.

**Four protocol layers handle all communication:**

- **Gossip** -- Real-time pub/sub. When you post, it broadcasts instantly to anyone following you. Each user has a topic derived from their public key.
- **Sync** -- Historical pull. When you follow someone, their existing posts are fetched via a custom QUIC protocol. On startup, all followed users are synced in parallel.
- **Blobs** -- Content-addressed media storage. Images, videos, and files are stored locally and transferred peer-to-peer using iroh-blobs.
- **DM** -- End-to-end encrypted direct messaging. A Noise IK handshake over QUIC establishes a shared secret between peers, which seeds a Double Ratchet providing per-message forward secrecy with ChaCha20-Poly1305 encryption. Messages are sent directly peer-to-peer with no intermediary, and queued locally for retry when the recipient is offline.

All data is persisted in a local SQLite database. The app works offline and syncs when peers are available.

## Features

- Create and delete posts (text + media attachments)
- Follow/unfollow users by Node ID
- View user profiles with their post history
- End-to-end encrypted direct messages with delivery status
- Offline message queuing with automatic retry
- Image lightbox for fullscreen viewing
- File downloads for non-media attachments
- Infinite scroll with cursor-based pagination
- Real-time feed updates via gossip
- Unread message badge in navigation
- 60-second auto-sync (pauses when window is hidden)
- Connection status indicator (relay + peer count)
- Confirmation dialogs for destructive actions
- Dark theme UI

**Backend state model:** Only the `FeedManager` (which manages gossip subscriptions) is behind a mutex. All other state -- the Iroh endpoint, blob store, database -- is accessed lock-free, so blob fetches and feed queries never block each other.

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (latest stable)
- [Node.js](https://nodejs.org/) (v18+)
- [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your platform

## Development

```bash
npm install
npm run tauri dev
```

This starts both the Vite dev server (port 1420) and the Tauri backend.

## Building

```bash
npm run tauri build
```

Produces a native desktop application in `src-tauri/target/release/`.

## Community Server (Planned)

A self-hosted, headless server binary that adds opt-in aggregation, discovery, full-text search, and trending to the P2P network. Users register with a server by signing a cryptographic proof of identity. The server subscribes to their gossip topics and indexes their posts in SQLite with FTS5, exposing an HTTP API for search, trending hashtags, user directory, and aggregated feeds.

The server is an overlay -- the P2P layer remains the foundation. Users who never connect to a server lose nothing.

See [todos/community-server.md](todos/community-server.md) for the full design document and implementation roadmap.

## Direct Messaging

End-to-end encrypted direct messaging over a custom QUIC protocol (`iroh-social/dm/1`). E2E encryption uses X25519 key exchange derived from each user's existing ed25519 identity, with a Noise IK handshake for session establishment and a Double Ratchet for per-message forward secrecy. Messages are encrypted such that only the two participants can read them -- not relay servers, not community servers, not anyone.

- Noise IK + Double Ratchet (Signal Protocol pattern) with ChaCha20-Poly1305
- Offline message queuing with background retry (60-second outbox flush)
- Delivery acknowledgment over QUIC with real-time status updates
- Conversation list with unread badges and message previews
- Start conversations from any user's profile page

See [todos/direct-messaging.md](todos/direct-messaging.md) for the original design document.

## Voice/Video Calls (Planned)

Peer-to-peer voice and video calls, with call signaling encrypted via the DM ratchet session.

- Voice calls with Opus audio codec over multiplexed QUIC streams
- Video calls with VP9 codec and adaptive bitrate

See [todos/voice-video-calling.md](todos/voice-video-calling.md) for the design document.

## Linked Devices (Planned)

Link multiple devices to a single identity, similar to Signal's linked devices. A primary device holds the master keypair and authorizes secondaries via QR code pairing over an encrypted channel. Linked devices share the social graph, message history, and profile.

See [todos/linked-devices.md](todos/linked-devices.md) for the design document.

## Recommended IDE Setup

[VS Code](https://code.visualstudio.com/) + [Svelte](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer).

## License

MIT
