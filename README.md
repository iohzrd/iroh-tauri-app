# Iroh Social

A decentralized peer-to-peer social network built with [Iroh](https://iroh.computer/), [Tauri 2](https://tauri.app/), and [SvelteKit 5](https://svelte.dev/).

Every user runs their own node. Posts, profiles, and follows are stored locally. Peers exchange data directly -- no central server, no accounts, no passwords.

## How It Works

Each node gets a permanent cryptographic identity (stored in `identity.key`). Your public key is your Node ID -- share it with others so they can follow you.

**Three protocol layers handle all communication:**

- **Gossip** -- Real-time pub/sub. When you post, it broadcasts instantly to anyone following you. Each user has a topic derived from their public key.
- **Sync** -- Historical pull. When you follow someone, their existing posts are fetched via a custom QUIC protocol. On startup, all followed users are synced in parallel.
- **Blobs** -- Content-addressed media storage. Images, videos, and files are stored locally and transferred peer-to-peer using iroh-blobs.

All data is persisted in a local SQLite database. The app works offline and syncs when peers are available.

## Features

- Create and delete posts (text + media attachments)
- Follow/unfollow users by Node ID
- View user profiles with their post history
- Image lightbox for fullscreen viewing
- File downloads for non-media attachments
- Infinite scroll with cursor-based pagination
- Real-time feed updates via gossip
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

## Direct Messaging & Calls (Planned)

End-to-end encrypted direct messaging and peer-to-peer voice/video calls. E2E encryption uses X25519 key exchange derived from each user's existing ed25519 identity, with a Double Ratchet protocol for forward secrecy. Messages are encrypted such that only the two participants can read them -- not relay servers, not community servers, not anyone.

- DMs over a custom QUIC protocol (`iroh-social/dm/1`) with offline outbox and retry
- Voice calls with Opus audio codec over multiplexed QUIC streams
- Video calls with VP9 codec and adaptive bitrate
- Call signaling is also E2E encrypted via the DM ratchet session

See [todos/direct-messaging.md](todos/direct-messaging.md) for the full design document and implementation roadmap.

## Recommended IDE Setup

[VS Code](https://code.visualstudio.com/) + [Svelte](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer).

## License

MIT
