# Likes, Replies, and Reposts - Design Document

Social interactions (likes, replies, reposts) for Iroh Social. These build on the existing Post and gossip infrastructure, adding new interaction types that propagate across the P2P network using the same gossip topics and sync protocol.

## Table of Contents

- [Design Principles](#design-principles)
- [Architecture Overview](#architecture-overview)
- [Data Model Changes](#data-model-changes)
- [Protocol Changes](#protocol-changes)
- [Storage Changes](#storage-changes)
- [Backend Commands](#backend-commands)
- [Frontend Integration](#frontend-integration)
- [Implementation Roadmap](#implementation-roadmap)

---

## Design Principles

1. **Interactions are authored data** -- A like, reply, or repost is authored by a specific user and published to that user's feed topic. This means interactions follow the same ownership model as posts: you control your own interactions.
2. **Replies are posts** -- A reply is a Post with a `reply_to` field. This means replies can themselves be liked, replied to, and reposted without needing interactions-on-interactions. Replies appear in the author's feed naturally and thread view is simply "get posts where reply_to = X."
3. **Likes and reposts are lightweight interactions** -- Unlike replies, likes and reposts carry no content. They are a separate `Interaction` type with simple toggle semantics.
4. **Eventual consistency** -- Counts are locally aggregated from received interactions. Different peers may see different counts depending on who they follow and what they have synced. This is acceptable for a P2P system.
5. **No global counters** -- There is no authoritative "like count" for a post. Each client tallies interactions it has seen. Community servers (future) can provide better-aggregated counts.
6. **Minimal protocol changes** -- Reuse the existing gossip topic and sync infrastructure. Interactions are new message types, not a separate protocol.
7. **Idempotent ingestion** -- Like any other data, interactions use `INSERT OR IGNORE` with unique constraints to handle duplicates from gossip and sync.

---

## Architecture Overview

```
User likes a post
  |
  v
Client creates an Interaction { kind: Like, target_post_id, target_author }
  |
  v
Broadcasts via GossipMessage::NewInteraction to OWN gossip topic
  (user_feed_topic(my_pubkey))
  |
  v
Followers receive via gossip subscription
  |
  v
Store interaction locally, update cached counts
```

```
User replies to a post
  |
  v
Client creates a Post { reply_to: Some(parent_id), reply_to_author: Some(parent_author), ... }
  |
  v
Broadcasts via GossipMessage::NewPost to OWN gossip topic (same as any post)
  |
  v
Followers receive via gossip subscription
  |
  v
Store post locally (reply appears in feed AND can be viewed in thread)
```

Interactions propagate through the **author's** gossip topic, not the target post's author's topic. This means:

- You only see likes/reposts from people you follow (or yourself).
- You see replies from people you follow (they are posts in their feed).
- This is consistent with how posts work -- you see content from people you follow.
- Community servers can aggregate across all registered users for full counts.

---

## Data Model Changes

### Post (extended)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Post {
    pub id: String,
    pub author: String,
    pub content: String,
    pub timestamp: u64,
    #[serde(default)]
    pub media: Vec<MediaAttachment>,
    #[serde(default)]
    pub reply_to: Option<String>,         // parent post ID
    #[serde(default)]
    pub reply_to_author: Option<String>,  // parent post author
}
```

The `#[serde(default)]` attributes ensure backward compatibility -- older posts without these fields deserialize with `None`.

### New Type: Interaction (shared crate)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interaction {
    pub id: String,              // unique per interaction
    pub author: String,          // who performed the interaction
    pub kind: InteractionKind,
    pub target_post_id: String,  // the post being interacted with
    pub target_author: String,   // author of the target post
    pub timestamp: u64,          // millis since epoch
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum InteractionKind {
    Like,
    Repost,
}
```

Note: No `content` or `media` on Interaction -- replies are Posts, not Interactions.

### Post Display Counts (Frontend-Only)

```typescript
interface PostCounts {
  likes: number;
  replies: number;
  reposts: number;
  liked_by_me: boolean;
  reposted_by_me: boolean;
}
```

These are computed from locally stored interactions and reply posts, not stored on the Post itself.

---

## Protocol Changes

### GossipMessage (extend existing enum)

```rust
pub enum GossipMessage {
    NewPost(Post),
    DeletePost { id: String, author: String },
    ProfileUpdate(Profile),
    // New variants:
    NewInteraction(Interaction),
    DeleteInteraction { id: String, author: String },
}
```

Adding new variants is backward-compatible with serde JSON deserialization -- older clients that don't recognize `NewInteraction` will skip it (serde will error on unknown variant, which the gossip handler already catches and ignores).

Replies are just Posts with `reply_to` set, so they use the existing `NewPost` variant. No new variant needed for replies.

### SyncResponse (extend existing struct)

```rust
pub struct SyncResponse {
    pub posts: Vec<Post>,
    #[serde(default)]
    pub profile: Option<Profile>,
    pub total_count: u64,
    pub newest_ts: Option<u64>,
    pub oldest_ts: Option<u64>,
    // New field:
    #[serde(default)]
    pub interactions: Vec<Interaction>,
}
```

The `#[serde(default)]` attribute ensures backward compatibility -- older peers that don't include `interactions` will deserialize to an empty Vec.

### Validation

Add `validate_interaction()` to the shared crate:

```rust
pub fn validate_interaction(interaction: &Interaction) -> Result<(), String> {
    let now = now_millis();
    if interaction.timestamp > now + MAX_TIMESTAMP_DRIFT_MS {
        return Err("interaction timestamp too far in future".into());
    }
    Ok(())
}
```

Interaction validation is minimal since likes/reposts carry no content.

---

## Storage Changes

### New Migration: `005_interactions.sql`

```sql
CREATE TABLE IF NOT EXISTS interactions (
    id TEXT NOT NULL,
    author TEXT NOT NULL,
    kind TEXT NOT NULL,           -- 'Like', 'Repost'
    target_post_id TEXT NOT NULL,
    target_author TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    PRIMARY KEY (author, id)
);

-- Find all interactions for a specific post
CREATE INDEX IF NOT EXISTS idx_interactions_target
    ON interactions(target_post_id, kind);

-- Find interactions by author (for sync)
CREATE INDEX IF NOT EXISTS idx_interactions_author_timestamp
    ON interactions(author, timestamp DESC);

-- Prevent duplicate likes/reposts (one per user per post)
CREATE UNIQUE INDEX IF NOT EXISTS idx_interactions_unique
    ON interactions(author, kind, target_post_id);
```

### Post table migration: `006_post_replies.sql`

```sql
ALTER TABLE posts ADD COLUMN reply_to TEXT;
ALTER TABLE posts ADD COLUMN reply_to_author TEXT;

-- Find replies to a specific post
CREATE INDEX IF NOT EXISTS idx_posts_reply_to
    ON posts(reply_to) WHERE reply_to IS NOT NULL;
```

### Storage Methods

```rust
impl Storage {
    // Interactions (likes/reposts)
    pub fn save_interaction(&self, interaction: &Interaction) -> anyhow::Result<()>;
    pub fn delete_interaction(&self, id: &str, author: &str) -> anyhow::Result<()>;
    pub fn delete_interaction_by_target(&self, author: &str, kind: &str, target_post_id: &str) -> anyhow::Result<Option<String>>;
    pub fn get_post_counts(&self, my_pubkey: &str, target_post_id: &str) -> anyhow::Result<PostCounts>;
    pub fn get_interactions_by_author(&self, author: &str, limit: usize, before: Option<u64>) -> anyhow::Result<Vec<Interaction>>;

    // Replies (just post queries)
    pub fn get_replies(&self, parent_post_id: &str, limit: usize, before: Option<u64>) -> anyhow::Result<Vec<Post>>;
    pub fn count_replies(&self, parent_post_id: &str) -> anyhow::Result<u64>;
}
```

### Gossip Handler Changes

In `gossip.rs`, add handling for `NewInteraction` and `DeleteInteraction`:

```rust
GossipMessage::NewInteraction(interaction) => {
    if interaction.author != expected_author {
        warn!("interaction author mismatch");
        return;
    }
    if let Err(e) = validate_interaction(&interaction) {
        warn!("invalid interaction: {e}");
        return;
    }
    storage.save_interaction(&interaction)?;
    app_handle.emit("interaction-received", &interaction)?;
}
GossipMessage::DeleteInteraction { id, author } => {
    if author != expected_author {
        return;
    }
    storage.delete_interaction(&id, &author)?;
    app_handle.emit("interaction-deleted", serde_json::json!({ "id": id, "author": author }))?;
}
```

### Sync Handler Changes

In `sync.rs`, include interactions in SyncResponse:

```rust
let interactions = storage.get_interactions_by_author(&author, limit, before)?;
SyncResponse {
    posts,
    profile,
    interactions,
    ..
}
```

When receiving a SyncResponse:

```rust
for interaction in response.interactions {
    if interaction.author != expected_author {
        continue;
    }
    if validate_interaction(&interaction).is_ok() {
        storage.save_interaction(&interaction)?;
    }
}
```

---

## Backend Commands

### New Tauri Commands

```rust
#[tauri::command]
async fn like_post(target_post_id: String, target_author: String) -> Result<Interaction, String>;

#[tauri::command]
async fn unlike_post(target_post_id: String, target_author: String) -> Result<(), String>;

#[tauri::command]
async fn repost(target_post_id: String, target_author: String) -> Result<Interaction, String>;

#[tauri::command]
async fn unrepost(target_post_id: String, target_author: String) -> Result<(), String>;

#[tauri::command]
async fn get_post_counts(target_post_id: String) -> Result<PostCounts, String>;

#[tauri::command]
async fn get_replies(target_post_id: String, limit: u32, before: Option<u64>) -> Result<Vec<Post>, String>;

#[tauri::command]
async fn delete_interaction(id: String) -> Result<(), String>;
```

Reply is just `create_post` with `reply_to` and `reply_to_author` set. The existing `create_post` command is extended:

```rust
#[tauri::command]
async fn create_post(
    content: String,
    media: Option<Vec<MediaAttachment>>,
    reply_to: Option<String>,
    reply_to_author: Option<String>,
) -> Result<Post, String>;
```

### Command Behavior

**`like_post`:**

1. Create Interaction with kind=Like, generate ID.
2. Save to local storage.
3. Broadcast `GossipMessage::NewInteraction` on own topic.
4. Return the interaction.

**`unlike_post`:**

1. Find the like interaction by (my_pubkey, "Like", target_post_id).
2. Delete from storage.
3. Broadcast `GossipMessage::DeleteInteraction` on own topic.

**`repost` / `unrepost`:** Same pattern as like/unlike.

**`create_post` with reply_to:** Same as regular post creation, just with reply_to fields populated.

---

## Frontend Integration

### Post Action Bar

Each post gets an action bar below its content:

```
[Heart] 3   [Reply] 1   [Repost] 2
```

- Heart icon: toggles like (filled when liked by current user).
- Reply icon: opens reply composer inline.
- Repost icon: toggles repost (highlighted when reposted by current user).
- Counts show locally known totals.

### Reply Threading

- Clicking reply count or reply button expands to show replies inline.
- Reply composer appears below the post.
- Replies show author avatar, name, content, timestamp.
- Replies are themselves posts, so they have their own action bars.

### Feed Integration

- Reposts appear in the feed as "X reposted" with the original post embedded.
- Replies appear in the feed naturally (they are posts with reply_to set).
- Optionally show "Replying to @author" context above the reply content.

### New TypeScript Types

```typescript
interface Interaction {
  id: string;
  author: string;
  kind: "Like" | "Repost";
  target_post_id: string;
  target_author: string;
  timestamp: number;
}

interface PostCounts {
  likes: number;
  replies: number;
  reposts: number;
  liked_by_me: boolean;
  reposted_by_me: boolean;
}
```

### Frontend Events

Listen for these Tauri events to update counts in real time:

```typescript
listen("interaction-received", (event) => {
  // Update counts for the target post
});

listen("interaction-deleted", (event) => {
  // Decrement counts for the target post
});
```

---

## Implementation Roadmap

### Phase 1: Data Model and Types

- [ ] Add `reply_to` and `reply_to_author` to `Post` in `iroh-social-types/src/types.rs`
- [ ] Add `Interaction` and `InteractionKind` to `iroh-social-types/src/types.rs`
- [ ] Add `validate_interaction()` to `validation.rs`
- [ ] Add `NewInteraction` and `DeleteInteraction` variants to `GossipMessage`
- [ ] Add `interactions` field to `SyncResponse`

### Phase 2: Storage

- [ ] Create `005_interactions.sql` migration
- [ ] Create `006_post_replies.sql` migration
- [ ] Add interaction storage methods to `storage.rs`
- [ ] Update `insert_post` and `row_to_post` for reply fields
- [ ] Add `get_replies` and `count_replies` methods

### Phase 3: Gossip and Sync

- [ ] Handle `NewInteraction` and `DeleteInteraction` in gossip handler
- [ ] Add `broadcast_interaction` and `broadcast_delete_interaction` to FeedManager
- [ ] Include interactions in sync response building
- [ ] Process incoming interactions during sync

### Phase 4: Backend Commands

- [ ] Implement `like_post`, `unlike_post`
- [ ] Implement `repost`, `unrepost`
- [ ] Implement `get_post_counts`, `get_replies`
- [ ] Extend `create_post` with `reply_to` / `reply_to_author` params
- [ ] Register all new commands in invoke_handler

### Phase 5: Frontend

- [ ] Add `Interaction` and `PostCounts` types to `types.ts`
- [ ] Create `PostActions.svelte` component (like/reply/repost buttons with counts)
- [ ] Integrate `PostActions` into feed page post cards
- [ ] Integrate `PostActions` into user profile post cards
- [ ] Add inline reply composer
- [ ] Add reply threading view
- [ ] Listen for `interaction-received` / `interaction-deleted` events

### Phase 6: Polish

- [ ] Handle edge cases: deleted target posts, self-interactions
- [ ] Run formatters and clippy
- [ ] Test with multiple peers
