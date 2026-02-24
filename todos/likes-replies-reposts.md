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
2. **Eventual consistency** -- Counts are locally aggregated from received interactions. Different peers may see different counts depending on who they follow and what they have synced. This is acceptable for a P2P system.
3. **No global counters** -- There is no authoritative "like count" for a post. Each client tallies interactions it has seen. Community servers (Phase 2) can provide better-aggregated counts.
4. **Minimal protocol changes** -- Reuse the existing gossip topic and sync infrastructure. Interactions are new message types, not a separate protocol.
5. **Idempotent ingestion** -- Like any other data, interactions use `INSERT OR IGNORE` with unique constraints to handle duplicates from gossip and sync.

---

## Architecture Overview

```
User likes a post
  |
  v
Client creates an Interaction { kind: Like, target_post_id, target_author }
  |
  v
Broadcasts via GossipMessage::Interaction to OWN gossip topic
  (user_feed_topic(my_pubkey))
  |
  v
Followers receive via gossip subscription
  |
  v
Store interaction locally, update cached counts
```

Interactions propagate through the **author's** gossip topic, not the target post's author's topic. This means:
- You only see likes/replies/reposts from people you follow (or yourself).
- This is consistent with how posts work -- you see content from people you follow.
- Community servers can aggregate across all registered users for full counts.

---

## Data Model Changes

### New Type: Interaction (shared crate)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interaction {
    pub id: String,              // UUID, unique per interaction
    pub author: String,          // who performed the interaction
    pub kind: InteractionKind,
    pub target_post_id: String,  // the post being interacted with
    pub target_author: String,   // author of the target post
    pub timestamp: u64,          // millis since epoch
    // Only set for replies:
    pub content: Option<String>,
    #[serde(default)]
    pub media: Vec<MediaAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InteractionKind {
    Like,
    Reply,
    Repost,
}
```

### Why replies are interactions, not posts

Replies could be modeled as posts with a `parent_id` field, but treating them as interactions has advantages:
- **Unified interaction model** -- likes, replies, and reposts share the same propagation path and storage pattern.
- **Threading is explicit** -- A reply always references a specific post. The `content` field carries the reply text.
- **Replies appear in both contexts** -- They show up in the parent post's thread AND in the author's feed (since they propagate on the author's topic).

A reply with `content` and optional `media` is functionally equivalent to a post, but with an explicit parent reference.

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

These are computed from locally stored interactions, not stored on the Post itself.

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

### SyncResponse (extend existing struct)

```rust
pub struct SyncResponse {
    pub posts: Vec<Post>,
    #[serde(default)]
    pub profile: Option<Profile>,
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
    // Reply content length check (same limit as posts)
    if let Some(ref content) = interaction.content {
        if content.len() > MAX_POST_CONTENT_LEN {
            return Err("reply content too long".into());
        }
    }
    // Media count check (only replies can have media)
    if interaction.kind != InteractionKind::Reply && !interaction.media.is_empty() {
        return Err("only replies can have media".into());
    }
    if interaction.media.len() > MAX_MEDIA_COUNT {
        return Err("too many media attachments".into());
    }
    // Likes and reposts must not have content
    if interaction.kind != InteractionKind::Reply && interaction.content.is_some() {
        return Err("only replies can have content".into());
    }
    // Replies must have content
    if interaction.kind == InteractionKind::Reply && interaction.content.as_ref().map_or(true, |c| c.trim().is_empty()) {
        return Err("reply must have content".into());
    }
    // Timestamp drift check
    let now = now_millis();
    if interaction.timestamp > now + MAX_TIMESTAMP_DRIFT_MS {
        return Err("interaction timestamp too far in future".into());
    }
    Ok(())
}
```

---

## Storage Changes

### New Migration: `003_interactions.sql`

```sql
CREATE TABLE IF NOT EXISTS interactions (
    id TEXT NOT NULL,
    author TEXT NOT NULL,
    kind TEXT NOT NULL,           -- 'like', 'reply', 'repost'
    target_post_id TEXT NOT NULL,
    target_author TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    content TEXT,
    media_json TEXT NOT NULL DEFAULT '[]',
    PRIMARY KEY (author, id)
);

-- Find all interactions for a specific post
CREATE INDEX IF NOT EXISTS idx_interactions_target
    ON interactions(target_author, target_post_id, kind);

-- Find interactions by author (for sync)
CREATE INDEX IF NOT EXISTS idx_interactions_author_timestamp
    ON interactions(author, timestamp DESC);

-- Prevent duplicate likes/reposts (one per user per post)
CREATE UNIQUE INDEX IF NOT EXISTS idx_interactions_unique_like_repost
    ON interactions(author, kind, target_author, target_post_id)
    WHERE kind IN ('like', 'repost');
```

The unique index on `(author, kind, target_author, target_post_id)` for likes and reposts prevents a user from liking or reposting the same post twice. Replies are not constrained this way -- a user can reply multiple times.

### Storage Methods

```rust
impl Storage {
    pub fn save_interaction(&self, interaction: &Interaction) -> anyhow::Result<()>;
    pub fn delete_interaction(&self, id: &str, author: &str) -> anyhow::Result<()>;
    pub fn get_post_interactions(&self, target_author: &str, target_post_id: &str)
        -> anyhow::Result<Vec<Interaction>>;
    pub fn get_post_counts(&self, target_author: &str, target_post_id: &str)
        -> anyhow::Result<(u32, u32, u32)>;  // (likes, replies, reposts)
    pub fn has_liked(&self, author: &str, target_author: &str, target_post_id: &str)
        -> anyhow::Result<bool>;
    pub fn has_reposted(&self, author: &str, target_author: &str, target_post_id: &str)
        -> anyhow::Result<bool>;
    pub fn get_replies(&self, target_author: &str, target_post_id: &str, limit: u32, before: Option<u64>)
        -> anyhow::Result<Vec<Interaction>>;
    pub fn get_interactions_by_author(&self, author: &str, limit: u32, before: Option<u64>)
        -> anyhow::Result<Vec<Interaction>>;
}
```

### Gossip Handler Changes

In `gossip.rs`, add handling for `NewInteraction` and `DeleteInteraction`:

```rust
GossipMessage::NewInteraction(interaction) => {
    // Verify author matches the gossip topic owner
    if interaction.author != expected_author {
        warn!("interaction author mismatch");
        return;
    }
    if let Err(e) = validate_interaction(&interaction) {
        warn!("invalid interaction: {e}");
        return;
    }
    storage.save_interaction(&interaction)?;
    // Emit event to frontend
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
// When building SyncResponse for a peer:
let interactions = storage.get_interactions_by_author(&author, limit, before)?;
SyncResponse {
    posts,
    profile,
    interactions,
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
async fn reply_to_post(
    target_post_id: String,
    target_author: String,
    content: String,
    media: Vec<MediaAttachment>,
) -> Result<Interaction, String>;

#[tauri::command]
async fn repost(target_post_id: String, target_author: String) -> Result<Interaction, String>;

#[tauri::command]
async fn unrepost(target_post_id: String, target_author: String) -> Result<(), String>;

#[tauri::command]
async fn get_post_counts(target_post_id: String, target_author: String)
    -> Result<PostCounts, String>;

#[tauri::command]
async fn get_replies(target_post_id: String, target_author: String, limit: u32, before: Option<u64>)
    -> Result<Vec<Interaction>, String>;

#[tauri::command]
async fn delete_interaction(id: String) -> Result<(), String>;
```

### Command Behavior

**`like_post`:**
1. Create Interaction with kind=Like, generate UUID.
2. Save to local storage.
3. Broadcast `GossipMessage::NewInteraction` on own topic.
4. Return the interaction.

**`unlike_post`:**
1. Find the like interaction by (my_pubkey, "like", target_author, target_post_id).
2. Delete from storage.
3. Broadcast `GossipMessage::DeleteInteraction` on own topic.

**`reply_to_post`:**
1. Validate content (non-empty, length check).
2. Create Interaction with kind=Reply, content, media.
3. Save and broadcast.

**`repost` / `unrepost`:** Same pattern as like/unlike.

---

## Frontend Integration

### Post Action Bar

Each post gets an action bar below its content:

```
[Heart] 3   [Reply] 1   [Repost] 2
```

- Heart icon: toggles like (filled when liked by current user).
- Reply icon: opens reply composer.
- Repost icon: toggles repost (highlighted when reposted by current user).
- Counts show locally known totals.

### Post Detail / Thread View

When a post is tapped/clicked, expand to show:
- Full post content.
- Reply composer.
- Replies listed chronologically below the post.
- Each reply shows author avatar, name, content, timestamp.
- Replies are themselves interactive (can be liked, replied to).

### Feed Integration

- Reposts appear in the feed as "X reposted" with the original post embedded.
- Replies can optionally appear in the feed (configurable -- "show replies in feed" toggle).

### New TypeScript Types

```typescript
interface Interaction {
    id: string;
    author: string;
    kind: "like" | "reply" | "repost";
    target_post_id: string;
    target_author: string;
    timestamp: number;
    content: string | null;
    media: MediaAttachment[];
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
    // If it's a reply, add to thread if viewing
});

listen("interaction-deleted", (event) => {
    // Decrement counts for the target post
    // Remove from thread if viewing
});
```

### Component Structure

```
PostCard.svelte         -- existing, add action bar
  PostActions.svelte    -- new: like/reply/repost buttons + counts
  ReplyComposer.svelte  -- new: inline reply input
PostThread.svelte       -- new: expanded post with replies
  ReplyCard.svelte      -- new: single reply display
```

---

## Implementation Roadmap

### Phase 1: Data Model and Protocol

- [ ] Add `Interaction` and `InteractionKind` to `iroh-social-types/src/types.rs`
- [ ] Add `NewInteraction` and `DeleteInteraction` variants to `GossipMessage`
- [ ] Add `interactions` field to `SyncResponse`
- [ ] Add `validate_interaction()` to `validation.rs`
- [ ] Create `003_interactions.sql` migration
- [ ] Add interaction storage methods to `storage.rs`

### Phase 2: Backend Commands and Gossip

- [ ] Implement Tauri commands: `like_post`, `unlike_post`, `reply_to_post`, `repost`, `unrepost`, `get_post_counts`, `get_replies`, `delete_interaction`
- [ ] Handle `NewInteraction` and `DeleteInteraction` in gossip handler
- [ ] Include interactions in sync response building
- [ ] Process incoming interactions during sync
- [ ] Register all new commands in invoke_handler
- [ ] Emit `interaction-received` and `interaction-deleted` events

### Phase 3: Frontend - Action Bar and Counts

- [ ] Add `Interaction` and `PostCounts` types to `types.ts`
- [ ] Create `PostActions.svelte` component (like/reply/repost buttons with counts)
- [ ] Integrate `PostActions` into feed page post cards
- [ ] Integrate `PostActions` into user profile post cards
- [ ] Implement optimistic UI updates (increment count immediately, revert on error)
- [ ] Listen for `interaction-received` / `interaction-deleted` events to update counts

### Phase 4: Frontend - Replies and Threading

- [ ] Create `ReplyComposer.svelte` (inline reply input with media support)
- [ ] Create `PostThread.svelte` (expanded post view with reply list)
- [ ] Create `ReplyCard.svelte` (single reply display with avatar, content)
- [ ] Add post detail route or expand-in-place UI for viewing threads
- [ ] Support nested reply viewing (reply to a reply)
- [ ] Load more replies with pagination

### Phase 5: Frontend - Reposts in Feed

- [ ] Display reposts in feed with "X reposted" header and embedded original post
- [ ] Fetch original post content for repost display (may need new command)
- [ ] Add repost indicator styling

### Phase 6: Polish

- [ ] Handle edge cases: deleted target posts, self-interactions
- [ ] Add interaction counts to user profile page (total likes received, etc.)
- [ ] Run formatters and clippy
- [ ] Test with multiple peers
