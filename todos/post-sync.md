# Post Sync - Design Document

Complete post synchronization for Iroh Social. Currently, following a user only downloads their 50 most recent posts. This design ensures that followers eventually receive a user's full post history through a combination of protocol enhancements, smart sync strategies, and on-demand fetching.

## Table of Contents

- [Problem Statement](#problem-statement)
- [Design Principles](#design-principles)
- [Protocol Changes](#protocol-changes)
- [Database Changes](#database-changes)
- [Sync Strategies](#sync-strategies)
- [Frontend Integration](#frontend-integration)
- [Implementation Roadmap](#implementation-roadmap)

---

## Problem Statement

Today, post syncing is capped at 50 posts in three places:

1. **On follow** (`follow_user`) -- fetches the 50 newest posts from the peer
2. **On startup** (`sync_one_follow`) -- fetches the 50 newest posts from each followed user
3. **Manual sync** (`sync_posts`) -- fetches 20 posts, never pages backwards

The sync protocol supports cursor-based pagination (`before` timestamp + `limit`), but nothing in the codebase ever uses it to page through older posts. If a user has 500 posts, followers only ever see the newest 50.

Additionally, there is no way to know how many posts you are missing. The client has no visibility into the remote peer's total post count or timestamp range.

---

## Design Principles

1. **Informed sync** -- The client should always know how many posts a peer has, so it can make smart sync decisions rather than guessing.
2. **Eventual completeness** -- Given enough time and connectivity, a follower should have a peer's full post history.
3. **Non-blocking** -- Sync should never block the UI or prevent other operations. Initial follow remains fast (50 posts), with the rest arriving in the background.
4. **Bandwidth-respectful** -- Don't hammer peers. Page slowly in the background, fetch on-demand when the user scrolls.
5. **Resumable** -- Track sync progress per follow so that partial syncs can resume across app restarts without re-fetching already-synced posts.


---

## Protocol Changes

### SyncRequest

Add an `after` field to support forward pagination (catching up on posts missed while offline):

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncRequest {
    pub author: String,
    pub before: Option<u64>,   // posts older than this timestamp
    pub after: Option<u64>,    // posts newer than this timestamp
    pub limit: u32,
}
```

- `before` queries return posts ordered `DESC` (newest first within the page) -- used for paging backwards into history.
- `after` queries return posts ordered `ASC` (oldest first within the page) -- used for catching up on missed posts.
- If both are specified, they define a time window. If neither is specified, returns the newest posts.

### SyncResponse

Add metadata about the peer's post history:

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncResponse {
    pub posts: Vec<Post>,
    pub profile: Option<Profile>,
    pub total_count: u64,      // total posts by this author
    pub newest_ts: Option<u64>, // timestamp of their newest post
    pub oldest_ts: Option<u64>, // timestamp of their oldest post
}
```

- **`total_count`** -- `SELECT COUNT(*) FROM posts WHERE author=?1`. Lets the client compare against its local count to know how many posts are missing.
- **`newest_ts`** -- `MAX(timestamp)`. Lets the client know if it has caught up on the forward end. `None` if zero posts.
- **`oldest_ts`** -- `MIN(timestamp)`. Lets the client know if it has paged all the way back. `None` if zero posts.

### Server Query

The server computes all three in a single query:

```sql
SELECT COUNT(*), MIN(timestamp), MAX(timestamp) FROM posts WHERE author=?1
```

This is a single index scan on `idx_posts_author_timestamp` -- cheap.

---

## Database Changes

### New Migration: `003_sync_tracking.sql`

```sql
ALTER TABLE follows ADD COLUMN newest_synced_ts INTEGER;
ALTER TABLE follows ADD COLUMN oldest_synced_ts INTEGER;
```

These columns track the sync window per followed user:

- **`newest_synced_ts`** -- timestamp of the most recent post synced from this peer. Used as the `after` cursor for forward catch-up.
- **`oldest_synced_ts`** -- timestamp of the oldest post synced from this peer. Used as the `before` cursor for background backward sync.

### New Storage Methods

```rust
// Count posts by a specific author (for sync comparison)
fn count_posts_by_author(&self, author: &str) -> anyhow::Result<u64>;

// Get min/max timestamps for an author's posts
fn get_author_post_range(&self, author: &str) -> anyhow::Result<(Option<u64>, Option<u64>)>;

// Update sync tracking timestamps for a follow
fn update_sync_timestamps(
    &self,
    pubkey: &str,
    newest_synced_ts: Option<u64>,
    oldest_synced_ts: Option<u64>,
) -> anyhow::Result<()>;

// Get current sync timestamps for a follow
fn get_sync_timestamps(&self, pubkey: &str) -> anyhow::Result<(Option<u64>, Option<u64>)>;

// Query posts with `after` support (for server handler)
fn get_posts_by_author_after(
    &self,
    author: &str,
    after: u64,
    limit: usize,
) -> anyhow::Result<Vec<Post>>;
```

---

## Sync Strategies

### Sync Window Model

Per followed user, the client maintains a window of synced posts defined by two cursors:

```
Peer's full history:  [oldest_ts] ========================== [newest_ts]  (total_count posts)

Your synced window:        [oldest_synced_ts] ---------- [newest_synced_ts]

                      ^^^^^ backward drip sync ^^^^^          ^^^^^ forward catch-up ^^^^^
```

### Strategy 1: Forward Catch-Up (Startup / Periodic)

Catches posts the peer published while you were offline.

- **Trigger:** App startup, periodic timer (every 60s)
- **Request:** `{ after: newest_synced_ts, limit: 50 }`
- **Ordering:** Server returns posts `ASC` by timestamp
- **Update:** Set `newest_synced_ts` to the max timestamp in the response
- **Termination:** Response returns fewer posts than `limit`, or `newest_synced_ts >= newest_ts` from the response metadata
- **Replaces:** Current startup sync that always fetches the same 50 newest posts

### Strategy 2: Background Backward Drip

Pages through older history slowly in the background.

- **Trigger:** After forward catch-up completes, runs continuously with delays
- **Request:** `{ before: oldest_synced_ts, limit: 50 }`
- **Ordering:** Server returns posts `DESC` by timestamp (existing behavior)
- **Update:** Set `oldest_synced_ts` to the min timestamp in the response
- **Pacing:** Wait 30 seconds between pages to avoid hammering peers
- **Termination:** Response is empty, or `oldest_synced_ts <= oldest_ts` from response metadata, or local count matches `total_count`
- **Resumable:** Since `oldest_synced_ts` is persisted, this resumes across restarts

### Strategy 3: On-Demand Scroll Fetch (Lazy)

When viewing a user's profile, fetch more posts if the user scrolls past what's locally available.

- **Trigger:** Infinite scroll sentinel on user profile page, when local posts run out
- **Request:** `{ before: oldest_local_timestamp, limit: 50 }`
- **Behavior:** Same as existing pagination, but falls through to remote fetch when local DB has no more results
- **Fallback:** If the peer is offline, show "peer offline, showing cached posts only"
- **Update:** Updates `oldest_synced_ts` if the fetched posts extend the window

### Strategy 4: On-Follow Initial Sync

Unchanged from current behavior, but now tracks timestamps:

- Fetch 50 newest posts
- Set `newest_synced_ts` = max timestamp from response
- Set `oldest_synced_ts` = min timestamp from response
- Background drip kicks in afterward to fill the rest

### Sync State Machine (Per Follow)

```
IDLE
  |
  v
FORWARD_CATCH_UP  (after: newest_synced_ts)
  |  loop until caught up
  v
BACKWARD_DRIP  (before: oldest_synced_ts, 30s delay between pages)
  |  loop until fully synced
  v
FULLY_SYNCED
  |
  v  (periodic timer fires or feed-updated event)
FORWARD_CATCH_UP  (restart cycle)
```

---

## Frontend Integration

### Sync Status Display

On the user profile page, show sync progress:

- "47 of 347 posts synced" (using local count vs `total_count`)
- "Syncing..." indicator while background drip is active
- "Fully synced" when local count matches `total_count`

### On-Scroll Remote Fetch

When viewing a user's profile and the Intersection Observer fires at the bottom:

1. First, query local DB for more posts (`get_user_posts` with `before` cursor)
2. If local returns empty and peer has more posts (based on `oldest_ts`), attempt remote fetch
3. If peer is offline, show "end of cached posts" message
4. If peer responds, insert posts into DB and display them

### Sync Controls

Optional: a per-user "Sync Now" button that triggers immediate forward + backward sync without waiting for the background timer.

---

## Implementation Roadmap

### Phase 1: Protocol + Storage

1. Add `after` field to `SyncRequest`
2. Add `total_count`, `newest_ts`, `oldest_ts` to `SyncResponse`
3. Create migration `003_sync_tracking.sql`
4. Add storage methods: `count_posts_by_author`, `get_author_post_range`, `update_sync_timestamps`, `get_posts_by_author_after`
5. Update `SyncHandler::accept()` to populate new response fields and handle `after` queries

### Phase 2: Smart Sync Logic

6. Update `follow_user()` to set initial `newest_synced_ts` / `oldest_synced_ts`
7. Rewrite `sync_one_follow()` to use forward catch-up (`after: newest_synced_ts`) instead of always fetching newest 50
8. Add background backward drip task that pages through history with pacing
9. Update periodic sync to use the new forward catch-up strategy

### Phase 3: Frontend

10. Add on-scroll remote fetch to user profile page
11. Show sync progress on user profile page
12. Add "Sync Now" button (optional)
13. Handle offline peer gracefully in UI
