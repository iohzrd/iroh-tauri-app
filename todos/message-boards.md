# Message Boards - Design Document

Decentralized, topic-based message boards for Iroh Social. Anyone can create a board, anyone can post to it. Boards are P2P gossip topics -- no central server owns them. Think of it as 4chan/8chan where every board is a gossip channel and every post propagates peer-to-peer.

## Table of Contents

- [Design Principles](#design-principles)
- [Board Identity](#board-identity)
- [Protocol](#protocol)
- [Post Structure](#post-structure)
- [Storage](#storage)
- [Moderation](#moderation)
- [Client Integration](#client-integration)
- [Implementation Roadmap](#implementation-roadmap)

---

## Design Principles

1. **Anyone can create a board** -- A board is just a deterministic gossip topic. Creating one is free, instant, and requires no permission. There is no registration, no approval, no server.
2. **No owner, no king** -- Boards have a creator (the first to publish the board definition) but no privileged operator. Moderation is local -- each user decides what they see.
3. **Topic-first, not person-first** -- The existing social feed is organized by who you follow. Boards are organized by what you're interested in. You subscribe to boards, not people.
4. **Configurable retention** -- Each node chooses how long to keep board posts: 7 days, 30 days, or indefinitely. Default is 7 days to match the imageboard ethos, but users who want a permanent archive can disable pruning entirely.
5. **Works without servers** -- Community servers can index boards for search and discovery, but the P2P layer is the foundation. Boards work fine with zero infrastructure.
6. **Pseudonymous** -- Posts carry the author's pubkey, but boards don't require profiles or identity. You can lurk, post, and leave.

---

## Board Identity

A board is identified by a short, human-readable name. The gossip topic is derived deterministically:

```
board_topic(name) = SHA256("iroh-board-v1:" + lowercase(name))
```

This means:
- `/tech/` on your node and `/tech/` on mine resolve to the same gossip topic.
- No registration or coordination needed -- just agree on the name.
- Case-insensitive: "Tech", "TECH", "tech" all hash to the same topic.

### Board Definition

The first message to a board topic is a `BoardDef` that establishes metadata. It's not authoritative -- if someone creates `/tech/` with a different description, both definitions propagate and clients display whichever they saw first (or let the user pick).

```rust
struct BoardDef {
    /// Short name (e.g. "tech", "music", "random").
    name: String,
    /// One-line description.
    description: String,
    /// Creator's pubkey (informational, not a privilege).
    creator: String,
    /// Creation timestamp.
    created_at: u64,
    /// Optional rules/guidelines (displayed in sidebar).
    rules: Option<String>,
    /// Whether posts without media are allowed (text-only boards vs media boards).
    text_only: bool,
}
```

### Well-Known Boards

A set of default board names could ship with the app for bootstrapping:

- `/random/` -- anything goes
- `/tech/` -- technology
- `/music/` -- music
- `/art/` -- art and creative work
- `/meta/` -- discussion about the app itself

Users can create any board by typing a name. If nobody else is on it, you're the first.

---

## Protocol

### Gossip Layer

Each board is a gossip topic. Subscribing to `/tech/` means joining the gossip swarm for `SHA256("iroh-board-v1:tech")`.

Messages on the topic are `BoardMessage` variants:

```rust
enum BoardMessage {
    /// Board metadata definition.
    Def(BoardDef),
    /// A new post or reply.
    Post(BoardPost),
    /// Delete a post (author only).
    Delete { post_id: String, author: String },
}
```

All messages are broadcast as JSON over gossip, same pattern as the existing social feed.

### Sync ALPN

For loading history when you first join a board (or reconnect after being offline), a sync protocol fetches older posts from peers:

```
ALPN: b"iroh-social/board-sync/1"
```

```rust
struct BoardSyncRequest {
    board_name: String,
    before: Option<u64>,  // timestamp cursor
    limit: u32,           // max posts to return (default 100)
}

struct BoardSyncResponse {
    posts: Vec<BoardPost>,
    board_def: Option<BoardDef>,
}
```

When you open a board for the first time:
1. Subscribe to the gossip topic (real-time posts start flowing).
2. Connect to known peers on `board-sync` ALPN and request the last N posts.
3. Merge received posts into local DB, deduplicate by `post_id`.

Peers to sync from are discovered via gossip `NeighborUp` events -- anyone in the swarm can serve history.

---

## Post Structure

### BoardPost

```rust
struct BoardPost {
    /// Unique post ID (UUID v4).
    id: String,
    /// Board name (lowercase).
    board: String,
    /// Author's pubkey.
    author: String,
    /// Post content (text). Max 10,000 chars.
    content: String,
    /// Unix timestamp in milliseconds.
    timestamp: u64,
    /// Media attachments (images, files). Max 10.
    media: Vec<MediaAttachment>,
    /// If this is a reply, the ID of the parent post.
    reply_to: Option<String>,
    /// Optional subject line for thread-starting posts.
    subject: Option<String>,
}
```

### Threading

Boards use flat threading (like classic imageboards):

- A post with `reply_to: None` starts a new thread.
- A post with `reply_to: Some(thread_id)` is a reply to that thread.
- Replies always reference the thread-starting post, not individual replies within the thread (no nested trees). To reference another reply, users quote it inline (e.g. `>>post_id`).
- Threads are sorted by "bump order" -- the thread with the most recent reply appears first.

### Post Validation

Same rules as social posts, plus board-specific checks:

- `content.len() <= 10_000`
- `media.len() <= 10`
- `timestamp` within 5 minutes of current time
- `board` matches the gossip topic the message arrived on
- `author` is a valid pubkey string
- If `reply_to` is set, it should reference a known post (soft check -- the parent may not have synced yet)

---

## Storage

### New Migration: `00N_message_boards.sql`

```sql
-- Board definitions (metadata)
CREATE TABLE IF NOT EXISTS boards (
    name TEXT PRIMARY KEY,
    description TEXT NOT NULL DEFAULT '',
    creator TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    rules TEXT,
    text_only INTEGER NOT NULL DEFAULT 0,
    subscribed INTEGER NOT NULL DEFAULT 0,
    last_post_at INTEGER NOT NULL DEFAULT 0
);

-- Board posts
CREATE TABLE IF NOT EXISTS board_posts (
    id TEXT PRIMARY KEY,
    board TEXT NOT NULL,
    author TEXT NOT NULL,
    content TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    media_json TEXT NOT NULL DEFAULT '[]',
    reply_to TEXT,
    subject TEXT,
    FOREIGN KEY (board) REFERENCES boards(name)
);
CREATE INDEX IF NOT EXISTS idx_board_posts_board_time
    ON board_posts(board, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_board_posts_reply
    ON board_posts(reply_to, timestamp ASC);

-- Board subscriptions (which boards are actively gossiped)
CREATE TABLE IF NOT EXISTS board_subscriptions (
    board_name TEXT PRIMARY KEY REFERENCES boards(name),
    subscribed_at INTEGER NOT NULL,
    last_synced_at INTEGER NOT NULL DEFAULT 0,
    unread_count INTEGER NOT NULL DEFAULT 0,
    -- Retention in days. 0 = retain indefinitely. NULL = use global default.
    retention_days INTEGER
);
```

### Retention / Pruning

To prevent unbounded growth, a background task prunes old posts:

```rust
/// max_age_days = 0 means retain indefinitely (no pruning).
fn prune_board_posts(board: &str, max_age_days: u32) -> Result<u32>
fn prune_all_boards(default_max_age_days: u32) -> Result<u32>
```

Retention is configurable per board and globally:
- **Global default**: 7 days (applied to boards without a per-board override).
- **Per-board override**: Set individually (e.g. keep `/meta/` forever, prune `/random/` after 1 day).
- **0 = retain indefinitely**: Setting retention to 0 disables pruning for that board.

Pruning runs every hour. Media blobs referenced only by pruned posts are eligible for garbage collection.

### Storage Methods

```rust
// Boards
fn upsert_board(def: &BoardDef) -> Result<()>
fn get_board(name: &str) -> Result<Option<BoardDef>>
fn get_subscribed_boards() -> Result<Vec<BoardDef>>
fn set_board_subscribed(name: &str, subscribed: bool) -> Result<()>
fn set_board_retention(name: &str, days: Option<u32>) -> Result<()>  // None = global default, Some(0) = forever
fn get_all_boards() -> Result<Vec<BoardDef>>  // all known boards

// Posts
fn insert_board_post(post: &BoardPost) -> Result<()>
fn get_board_posts(board: &str, limit: u32, before: Option<u64>) -> Result<Vec<BoardPost>>
fn get_thread(thread_id: &str) -> Result<Vec<BoardPost>>  // OP + all replies
fn get_threads(board: &str, limit: u32, offset: u32) -> Result<Vec<ThreadSummary>>
fn delete_board_post(post_id: &str, author: &str) -> Result<bool>
fn get_board_unread_count(board: &str) -> Result<u32>
fn mark_board_read(board: &str) -> Result<()>
fn get_total_board_unread() -> Result<u32>
```

### ThreadSummary

For the board catalog view (grid of threads):

```rust
struct ThreadSummary {
    op: BoardPost,           // thread-starting post
    reply_count: u32,        // total replies
    last_reply_at: u64,      // timestamp of most recent reply
    image_count: u32,        // total media across all posts in thread
    last_replies: Vec<BoardPost>,  // last 3 replies (preview)
}
```

---

## Moderation

There is no global moderation. Each user moderates their own experience:

### Local Filters

- **Hide post** -- Hide a specific post (stored locally, never broadcast).
- **Hide author** -- Hide all posts from a specific pubkey in all boards.
- **Word filter** -- Hide posts containing specific words/phrases.
- **Media filter** -- Option to hide all media by default (text-only mode).

### Board Creator Tools

The board creator has no special powers over the gossip layer, but community servers can optionally respect creator-signed moderation actions:

```rust
enum BoardModAction {
    /// Suggest hiding a post (community server may index this).
    HidePost { post_id: String },
    /// Suggest hiding an author from this board.
    HideAuthor { author: String },
    /// Pin a post to the top of the board.
    Pin { post_id: String },
}
```

These actions are signed by the creator and published to the board's gossip topic. They are suggestions -- clients can choose to respect them or not. Community servers that index the board can use them to filter search results.

### Spam Mitigation

Without a central authority, spam mitigation is local:

- **Rate limit display** -- If an author posts more than N times per minute, collapse their posts.
- **Proof of work (optional)** -- Boards can set a PoW difficulty. Posts must include a nonce such that `SHA256(post_id + nonce)` has N leading zeros. Clients verify before displaying.
- **Reputation (future)** -- Track per-author reliability scores locally. Authors whose posts you've hidden frequently get auto-collapsed.

---

## Client Integration

### New Types (`crates/iroh-social-types/src/board.rs`)

```rust
pub const BOARD_SYNC_ALPN: &[u8] = b"iroh-social/board-sync/1";

pub struct BoardDef { ... }
pub struct BoardPost { ... }
pub enum BoardMessage { Def, Post, Delete }
pub struct BoardSyncRequest { ... }
pub struct BoardSyncResponse { ... }
pub struct ThreadSummary { ... }
```

### Tauri Commands

```
// Board management
create_board(name, description, rules?, text_only?) -> BoardDef
subscribe_board(name)              -> ()
unsubscribe_board(name)            -> ()
get_subscribed_boards()            -> Vec<BoardDef>
get_board(name)                    -> Option<BoardDef>

// Posting
post_to_board(board, content, media?, reply_to?, subject?) -> BoardPost
delete_board_post(post_id)         -> ()

// Reading
get_board_catalog(board, limit?, offset?)     -> Vec<ThreadSummary>
get_board_thread(thread_id)                   -> Vec<BoardPost>
get_board_posts(board, limit?, before?)       -> Vec<BoardPost>
mark_board_read(board)                        -> ()
get_total_board_unread()                      -> u32

// Moderation
hide_board_post(post_id)           -> ()
hide_board_author(author)          -> ()
```

### Tauri Events

```
board-post-received    { board: String, post: BoardPost }
board-post-deleted     { board: String, post_id: String }
```

### Frontend Pages

**`/boards` page (board list):**

- List of subscribed boards with unread counts and last activity time.
- "Browse / Create" input at top -- type a board name to join or create it.
- Each row: board name, description, unread badge, last post timestamp.
- Unsubscribe button (swipe or context menu).

**`/boards/[name]` page (catalog view):**

- Grid or list of threads sorted by bump order.
- Each thread card: subject (if any), OP content preview, thumbnail, reply count, image count.
- "New Thread" button opens compose modal with subject + content + media.
- Pull to refresh / auto-update on gossip.
- Board description and rules in a collapsible sidebar/header.

**`/boards/[name]/[thread_id]` page (thread view):**

- Full thread: OP at top, replies in chronological order.
- Reply form at bottom (content + media).
- `>>post_id` references rendered as clickable links that scroll to the referenced post.
- Author pubkeys shown as short IDs with optional avatar.
- Hide button per post.

### Navigation Update

Add "Boards" tab to `+layout.svelte` navigation bar. Show total unread count badge across all subscribed boards.

---

## Relationship to Community Server

Community servers (see [community-server.md](community-server.md)) can optionally index boards:

- **Discovery** -- `GET /api/v1/boards` lists all boards the server has seen, ranked by activity.
- **Search** -- `GET /api/v1/boards/:name/search?q=query` searches posts within a board.
- **Trending** -- `GET /api/v1/boards/trending` shows boards with the most activity.
- **Archive** -- Servers can keep posts beyond the default 7-day retention, serving as long-term archives.

The server participates in board gossip topics and responds to board-sync requests like any other peer. It's just a peer that never goes offline and has a big disk.

---

## Implementation Roadmap

### Phase 1: Types & Storage

- [ ] Define `BoardDef`, `BoardPost`, `BoardMessage`, `ThreadSummary` in shared types crate
- [ ] Add `BOARD_SYNC_ALPN` constant
- [ ] Create migration for `boards`, `board_posts`, `board_subscriptions` tables
- [ ] Implement storage methods (CRUD, thread queries, pruning)
- [ ] Write storage tests

### Phase 2: Protocol & Handler

- [ ] Implement `BoardHandler` as `ProtocolHandler` for board-sync ALPN
- [ ] Implement gossip subscription management (subscribe/unsubscribe board topics)
- [ ] Implement gossip message handling (receive posts, deletes, board defs)
- [ ] Implement gossip broadcast (publish posts to board topic)
- [ ] Implement board-sync request/response (historical post fetch from peers)
- [ ] Add board-related state to `AppState` (subscriptions, gossip senders)
- [ ] Spawn background pruning task
- [ ] Add Tauri commands and events
- [ ] Register commands in invoke_handler

### Phase 3: Frontend

- [ ] Add TypeScript types for `BoardDef`, `BoardPost`, `ThreadSummary`
- [ ] Build `/boards` page (subscribed board list + create/browse input)
- [ ] Build `/boards/[name]` page (catalog view with thread cards)
- [ ] Build `/boards/[name]/[thread_id]` page (thread view with replies)
- [ ] Add "Boards" tab to navigation with unread badge
- [ ] Add post compose UI (new thread + reply forms)
- [ ] Add local moderation UI (hide post, hide author)
- [ ] Listen for `board-post-received` events for real-time updates

### Phase 4: Polish

- [ ] `>>post_id` quote rendering and click-to-scroll
- [ ] Image thumbnails in catalog view
- [ ] Configurable retention period per board
- [ ] Keyboard shortcuts (reply, navigate threads)
- [ ] Community server board indexing integration
- [ ] Board sharing via links (`iroh://board/name`)
