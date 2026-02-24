# Community Server - Design Document

A self-hosted, headless server binary that provides aggregation, discovery, search, and trending for the Iroh Social P2P network. Users opt in by registering with a server -- the server never scrapes or indexes without consent.

## Table of Contents

- [Architecture Overview](#architecture-overview)
- [Workspace Structure](#workspace-structure)
- [Registration Protocol](#registration-protocol)
- [Post Ingestion](#post-ingestion)
- [Server Storage](#server-storage)
- [HTTP API](#http-api)
- [Trending Algorithm](#trending-algorithm)
- [Client Integration](#client-integration)
- [Server Configuration](#server-configuration)
- [Federation (Phase 2)](#federation-phase-2)
- [Implementation Roadmap](#implementation-roadmap)

---

## Architecture Overview

```
                         +--------------------+
                         | Community Server   |
                         | (headless binary)  |
                         |                    |
  Users opt-in           | - Iroh node        |   HTTP API
  via signed    -------> | - Gossip listener  | <-------  Clients query for
  registration           | - Sync puller      |           search, trending,
                         | - sqlx (SQLite or  |           discovery
                         |   Postgres)        |
                         | - axum HTTP server |
                         +--------------------+
                                  |
                          Participates in
                          the P2P network
                          as a first-class
                          Iroh node
```

The server runs its own Iroh endpoint and joins the same gossip topics and sync protocol that regular clients use. It stores an aggregated index of all registered users' posts using sqlx (SQLite for small deployments, Postgres for scale) with full-text search. An axum HTTP API exposes this index for search, trending, user discovery, and aggregated feeds.

Key principle: **the server is an overlay, not a replacement**. The P2P layer remains the foundation. Users who never connect to a server lose nothing. Servers add opt-in social features that require aggregation (search, trending, discovery).

---

## Workspace Structure

The repo becomes a Cargo workspace with three crates:

```
iroh-tauri-app/
  Cargo.toml                      # Workspace root (new)
  crates/
    iroh-social-types/            # Shared types and protocol definitions (new)
      Cargo.toml
      src/
        lib.rs
        types.rs                  # Post, Profile, MediaAttachment, FollowEntry
        protocol.rs               # GossipMessage, SyncRequest/Response, SYNC_ALPN, user_feed_topic()
        validation.rs             # validate_post(), constants
        crypto.rs                 # Registration signing/verification types
    iroh-social-server/           # Server binary (new)
      Cargo.toml
      migrations/                 # sqlx migrations (SQLite + Postgres variants)
      src/
        main.rs                   # CLI entry, config loading, startup
        config.rs                 # TOML config parsing
        node.rs                   # Iroh endpoint, gossip, sync setup
        storage.rs                # sqlx storage (SQLite or Postgres)
        ingestion.rs              # Gossip subscriber + sync scheduler
        trending.rs               # Trending computation
        api/
          mod.rs                  # axum Router assembly
          server_info.rs          # GET /api/v1/info
          auth.rs                 # POST/DELETE /api/v1/register
          users.rs                # GET /api/v1/users, search, profile
          posts.rs                # GET /api/v1/posts/search
          feed.rs                 # GET /api/v1/feed
          trending.rs             # GET /api/v1/trending
  src-tauri/                      # Existing Tauri app (modified to use shared crate)
  src/                            # Existing Svelte frontend
```

### What moves to the shared crate

From `src-tauri/src/storage.rs`:

- `Post`, `Profile`, `MediaAttachment`, `FollowEntry` structs

From `src-tauri/src/gossip.rs`:

- `GossipMessage` enum
- `user_feed_topic()` function

From `src-tauri/src/sync.rs`:

- `SyncRequest`, `SyncResponse` structs
- `SYNC_ALPN` constant

From `src-tauri/src/lib.rs`:

- `validate_post()` function and validation constants

The Tauri crate then imports these from `iroh-social-types` instead of defining them inline.

### Shared crate dependencies (minimal)

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
iroh-gossip = "0.96"   # for TopicId
ed25519-dalek = { version = "2", features = ["serde"] }
```

---

## Registration Protocol

### Design

Single-step signed registration over HTTP. The user signs a payload with their Iroh secret key (ed25519) to prove identity. No challenge-response needed -- the payload includes server URL and timestamp to prevent replay.

### Registration flow

1. User constructs a `RegistrationPayload`:
   ```
   { pubkey, server_url, timestamp }
   ```
2. Serializes it deterministically (canonical JSON or a fixed `pubkey|server_url|timestamp` string).
3. Signs the bytes with their ed25519 secret key.
4. POSTs a `RegistrationRequest` to the server:
   ```
   { pubkey, server_url, timestamp, signature }
   ```
5. Server verifies:
   - Timestamp within 5 minutes of server time
   - `server_url` matches the server's own URL
   - Signature is valid for the pubkey over the reconstructed payload bytes
6. Server stores a registration record and begins ingesting the user's posts.

### Unregistration

Same mechanism: sign a payload with `action: "unregister"`, send to `DELETE /api/v1/register`. Server stops ingesting posts and marks user inactive.

### Data types

```rust
struct RegistrationPayload {
    pubkey: String,
    server_url: String,
    timestamp: u64,
}

struct RegistrationRequest {
    pubkey: String,
    server_url: String,
    timestamp: u64,
    signature: String,  // hex-encoded ed25519 signature
}

struct Registration {
    pubkey: String,
    registered_at: u64,
    last_seen: u64,
    display_name: Option<String>,
    bio: Option<String>,
    avatar_hash: Option<String>,
    is_active: bool,
}
```

---

## Post Ingestion

### Dual-mode: gossip + sync

Both mechanisms are needed for completeness:

**Gossip (real-time):** When a user registers, the server subscribes to their gossip topic (`user_feed_topic(pubkey)`). This is the same subscription pattern used by the Tauri client in `gossip.rs`. The server receives `NewPost`, `DeletePost`, and `ProfileUpdate` messages in real time.

**Sync (historical catch-up):** Uses the existing `fetch_remote_posts()` function from `sync.rs`. Triggered on:

- Server startup (sync all registered users)
- New user registration (sync their history immediately)
- Periodic catch-up every 15 minutes for users whose last gossip was >30 min ago

### Architecture

```
IngestionManager
  |
  +-- GossipSubscriber (per registered user)
  |     Subscribes to user_feed_topic(pubkey)
  |     Processes NewPost, DeletePost, ProfileUpdate
  |     Writes to SQLite
  |
  +-- SyncScheduler
        On startup: sync all registered users
        Every 15 min: catch-up sync for stale users
        On registration: immediate history pull
        Bounded concurrency via semaphore (max 10)
```

### Validation

Same checks as the Tauri client:

- `validate_post()` (content length, media count, timestamp drift)
- Author matches expected pubkey for the gossip topic
- Deduplication via `(author, id)` unique constraint in SQLite

---

## Server Storage

### sqlx with SQLite + Postgres

The server uses **sqlx** for database access, supporting both SQLite (simple self-hosting) and Postgres (high-scale deployments):

- **sqlx** provides async-native database access that fits naturally with the axum server
- **Compile-time query checking** catches SQL errors at build time via `sqlx::query!` macros
- **Start with SQLite** for easy self-hosting (single file, no external service, zero config)
- **Migrate to Postgres** when scaling demands it -- change the connection string, run migrations, done
- **Full-text search** via SQLite FTS5 or Postgres `tsvector`/`tsquery` (both supported behind a thin abstraction)

### Why not rusqlite?

The Tauri client uses rusqlite because it's an embedded single-user desktop app where async provides no benefit. The server is different:

- axum is async -- sqlx queries compose naturally without `spawn_blocking`
- Connection pooling matters for concurrent HTTP request handling
- Postgres support provides a scaling path for large communities
- Compile-time query checking is practical in a server build environment

### Dependencies

```toml
[dependencies]
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "postgres", "migrate"] }
```

### Schema (SQLite dialect -- Postgres uses equivalent types)

```sql
CREATE TABLE registrations (
    pubkey TEXT PRIMARY KEY,
    registered_at INTEGER NOT NULL,
    last_seen INTEGER NOT NULL,
    display_name TEXT,
    bio TEXT,
    avatar_hash TEXT,
    is_active BOOLEAN NOT NULL DEFAULT 1
);

CREATE TABLE posts (
    id TEXT NOT NULL,
    author TEXT NOT NULL,
    content TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    media_json TEXT,
    indexed_at INTEGER NOT NULL,
    PRIMARY KEY (author, id),
    FOREIGN KEY (author) REFERENCES registrations(pubkey)
);

CREATE INDEX idx_posts_timestamp ON posts(timestamp DESC);
CREATE INDEX idx_posts_author_timestamp ON posts(author, timestamp DESC);

-- Full-text search (SQLite variant)
CREATE VIRTUAL TABLE posts_fts USING fts5(
    content,
    content=posts,
    content_rowid=rowid,
    tokenize='unicode61'
);

-- Keep FTS in sync automatically
CREATE TRIGGER posts_ai AFTER INSERT ON posts BEGIN
    INSERT INTO posts_fts(rowid, content) VALUES (new.rowid, new.content);
END;
CREATE TRIGGER posts_ad AFTER DELETE ON posts BEGIN
    INSERT INTO posts_fts(posts_fts, rowid, content) VALUES('delete', old.rowid, old.content);
END;

-- Full-text search (Postgres variant, used instead of FTS5)
-- ALTER TABLE posts ADD COLUMN content_tsv tsvector
--     GENERATED ALWAYS AS (to_tsvector('english', content)) STORED;
-- CREATE INDEX idx_posts_fts ON posts USING GIN(content_tsv);

CREATE TABLE trending_hashtags (
    tag TEXT PRIMARY KEY,
    post_count INTEGER NOT NULL,
    unique_authors INTEGER NOT NULL,
    latest_post_at INTEGER NOT NULL,
    score REAL NOT NULL,
    computed_at INTEGER NOT NULL
);

CREATE TABLE sync_state (
    pubkey TEXT PRIMARY KEY,
    last_synced_at INTEGER NOT NULL,
    last_post_timestamp INTEGER,
    FOREIGN KEY (pubkey) REFERENCES registrations(pubkey)
);

CREATE TABLE server_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

### Database abstraction

The server uses sqlx's `Any` pool or a thin trait to abstract over SQLite and Postgres:

```rust
// Config determines which backend to use
enum DatabaseUrl {
    Sqlite(String),   // e.g. "sqlite:///var/lib/iroh-social/server.db"
    Postgres(String), // e.g. "postgres://user:pass@localhost/iroh_social"
}
```

Migrations are managed via `sqlx migrate` with separate migration directories for each backend where SQL differs (FTS5 vs tsvector).

---

## HTTP API

All endpoints under `/api/v1/`. Server also returns basic HTML at `GET /`.

### Endpoints

#### Server Info

```
GET /api/v1/info

Response: {
    name, description, version, node_id,
    registered_users, total_posts,
    uptime_seconds, registration_open
}
```

#### Registration

```
POST /api/v1/register
Body: { pubkey, server_url, timestamp, signature }
Response (201): { pubkey, registered_at, message }
Errors: 400 (bad sig/timestamp), 409 (exists), 403 (closed)

DELETE /api/v1/register
Body: { pubkey, server_url, timestamp, action: "unregister", signature }
Response (200): { message }
```

#### User Directory

```
GET /api/v1/users?limit=20&offset=0
Response: { users: [...], total, limit, offset }

GET /api/v1/users/search?q=alice&limit=20
Response: { users: [...], total, query }

GET /api/v1/users/:pubkey
Response: { pubkey, display_name, bio, avatar_hash, registered_at, post_count, latest_post_at }
```

#### Posts

```
GET /api/v1/users/:pubkey/posts?limit=50&before=<timestamp>
Response: { posts: [...] }

GET /api/v1/posts/search?q=rust+iroh&limit=20&offset=0
Response: { posts: [...], total, query }
```

#### Feed (Global)

```
GET /api/v1/feed?limit=50&before=<timestamp>
Response: { posts: [...] }

GET /api/v1/feed?limit=50&before=<timestamp>&authors=<pk1>,<pk2>
Optional author filter for custom feeds.
```

#### Trending

```
GET /api/v1/trending?limit=10
Response: { hashtags: [...], computed_at }

GET /api/v1/trending/posts?limit=20
Response: { posts: [...] }
```

### Middleware

- **Rate limiting** via `tower::limit`: registration 5/hr/IP, search 60/min/IP, reads 120/min/IP
- **CORS** enabled for all GET endpoints
- **Request logging** via `tower-http::trace`

---

## Trending Algorithm

### Hashtag extraction

Regex: `#[a-zA-Z0-9_]+`, normalized to lowercase.

### Scoring formula (per hashtag, over 24-hour window)

```
score = (post_count * author_weight * recency_factor) / age_decay

post_count    = posts containing the hashtag in the window
author_weight = sqrt(unique_authors)
recency_factor = sum(1.0 / (1.0 + hours_since_post)) for each post
age_decay     = 1.0 + (hours_since_oldest_post / 24.0)
```

- `sqrt(unique_authors)` prevents one user spamming a tag from dominating
- `recency_factor` weights newer posts higher
- `age_decay` reduces stale bursts

### Trending post score

```
post_score = (1 + min(hashtag_boost, 3)) * (1.0 / (1.0 + hours_since_post)^1.5)
```

Where `hashtag_boost` is the number of currently-trending hashtags in the post.

### Computation

Background task recomputes every 5 minutes. Results stored in `trending_hashtags` table. API reads always serve precomputed data.

---

## Client Integration

### New Tauri commands

```
add_server(url)              -- Fetch /api/v1/info, store connection
remove_server(url)           -- Remove stored connection
list_servers()               -- List all stored servers with status
register_with_server(url)    -- Sign + POST /api/v1/register
unregister_from_server(url)  -- Sign + DELETE /api/v1/register
server_search_posts(url, q)  -- Query /api/v1/posts/search
server_search_users(url, q)  -- Query /api/v1/users/search
server_get_feed(url, ...)    -- Query /api/v1/feed
server_get_trending(url)     -- Query /api/v1/trending
server_discover_users(url)   -- Query /api/v1/users
```

### New dependency

Add `reqwest` to `src-tauri/Cargo.toml` for HTTP client.

### New storage

Add a `servers` table to the client's SQLite database:

```sql
CREATE TABLE IF NOT EXISTS servers (
    url TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    is_registered INTEGER NOT NULL DEFAULT 0,
    added_at INTEGER NOT NULL
);
```

### New frontend pages

**`/servers` page:**

- List connected servers with status (online/offline)
- Add server by URL
- Register/unregister with each server

**`/discover` page (or integrated into servers):**

- Browse user directory from a selected server
- Search users by name
- "Follow" button for discovered users

**Search integration in feed:**

- When servers are configured, show search bar
- Results from server's FTS endpoint

**Trending section:**

- Display trending hashtags from connected server
- Click hashtag to search

### New TypeScript types

```typescript
interface ServerInfo {
  name: string;
  description: string;
  version: string;
  node_id: string;
  registered_users: number;
  total_posts: number;
  registration_open: boolean;
}
interface StoredServer {
  url: string;
  name: string;
  is_registered: boolean;
  added_at: number;
  status: "online" | "offline" | "unknown";
}
interface ServerUser {
  pubkey: string;
  display_name: string | null;
  bio: string | null;
  post_count: number;
}
interface TrendingHashtag {
  tag: string;
  post_count: number;
  unique_authors: number;
  score: number;
}
```

---

## Server Configuration

TOML config file:

```toml
[server]
name = "My Iroh Social Server"
description = "A community aggregation server"
listen_addr = "0.0.0.0:3000"
data_dir = "/var/lib/iroh-social-server"
public_url = "https://social.example.com"
registration_open = true

[limits]
max_registered_users = 1000
max_posts_per_user = 10000
rate_limit_requests_per_minute = 120

[sync]
interval_minutes = 15
startup_sync = true
max_concurrent_syncs = 10

[trending]
recompute_interval_minutes = 5
window_hours = 24
```

CLI (via `clap`):

```
iroh-social-server [OPTIONS]
  -c, --config <PATH>    Config file path (default: ./config.toml)
  --data-dir <PATH>      Override data directory
  --port <PORT>          Override listen port
```

---

## Federation (Phase 2)

Planned but not in initial scope. Servers would peer over iroh QUIC with a custom ALPN:

```
ALPN: b"iroh-social/federation/1"
```

What gets shared between servers:

- Registered user lists (pubkeys + profiles)
- Post metadata (other servers fetch full posts from users via P2P)
- Trending data

What does NOT get shared:

- Media blobs (fetch from users directly)
- User credentials

Federation uses iroh's QUIC transport (not HTTP) for NAT traversal and consistent P2P architecture.

---

## Implementation Roadmap

### Phase 1: Workspace Refactor

- [ ] Create workspace root `Cargo.toml`
- [ ] Create `crates/iroh-social-types/` with types extracted from `src-tauri/src/storage.rs`, `gossip.rs`, `sync.rs`, `lib.rs`
- [ ] Update `src-tauri/Cargo.toml` to use workspace deps and depend on shared crate
- [ ] Verify Tauri app builds and runs unchanged

### Phase 2: Server Core

- [ ] Create `crates/iroh-social-server/` skeleton with `main.rs` and config
- [ ] Implement sqlx storage layer with migrations (SQLite initially, Postgres-ready)
- [ ] Set up full-text search (FTS5 for SQLite, tsvector for Postgres)
- [ ] Implement Iroh node setup (endpoint, gossip, sync handler -- no Tauri)
- [ ] Implement registration verification (ed25519 signature check)

### Phase 3: Server API + Ingestion

- [ ] Set up axum with middleware (CORS, rate limiting, logging)
- [ ] Implement endpoints: `/info`, `/register`, `/users`, `/feed`, `/posts/search`, `/trending`
- [ ] Implement ingestion manager (gossip subscriber + sync scheduler)
- [ ] Implement trending computation background task

### Phase 4: Client Integration

- [ ] Add `reqwest` to Tauri app
- [ ] Add servers table to client SQLite storage
- [ ] Implement Tauri commands for server interaction
- [ ] Build `/servers` page in Svelte
- [ ] Integrate search and discover into UI

### Phase 5: Polish

- [ ] Error handling and logging
- [ ] Server health check / metrics endpoint
- [ ] Stub federation module with reserved ALPN
- [ ] Documentation and deployment guide
