# Iroh Social

A decentralized peer-to-peer social network built with [Iroh](https://iroh.computer/), [Tauri 2](https://tauri.app/), and [SvelteKit 5](https://svelte.dev/).

Every user runs their own node. Posts, profiles, and follows are stored locally. Peers exchange data directly -- no central server, no accounts, no passwords.

## How It Works

Each node gets a permanent cryptographic identity (stored in `identity.key`). Your public key is your Node ID -- share it with others so they can follow you.

**Three protocol layers handle all communication:**

- **Gossip** -- Real-time pub/sub. When you post, it broadcasts instantly to anyone following you. Each user has a topic derived from their public key.
- **Sync** -- Historical pull. When you follow someone, their existing posts are fetched via a custom QUIC protocol. On startup, all followed users are synced in parallel.
- **Blobs** -- Content-addressed media storage. Images, videos, and files are stored locally and transferred peer-to-peer using iroh-blobs.

All data is persisted in a local [redb](https://github.com/cberner/redb) embedded database. The app works offline and syncs when peers are available.

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

## Recommended IDE Setup

[VS Code](https://code.visualstudio.com/) + [Svelte](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer).

## License

MIT
