# Direct Messaging - Design Document

End-to-end encrypted direct messaging for Iroh Social. All private communication is encrypted such that only the two participants can read it -- not relay servers, not community servers, not anyone else.

See also: [voice-video-calling.md](voice-video-calling.md) for voice and video calling design.

## Table of Contents

- [Design Principles](#design-principles)
- [End-to-End Encryption](#end-to-end-encryption)
- [Direct Messaging](#direct-messaging)
- [Storage](#storage)
- [Client Integration](#client-integration)
- [Implementation Roadmap](#implementation-roadmap)

---

## Design Principles

1. **E2E encrypted by default** -- All DMs are encrypted with keys derived from a Diffie-Hellman exchange between the two participants. The QUIC transport layer (TLS 1.3) provides transport encryption, but E2E encryption ensures that even relay servers cannot read message content.
2. **No server dependency** -- DMs work purely P2P. Community servers never see message content.
3. **Offline message delivery** -- When a recipient is offline, messages are stored locally and delivered on next connection. No intermediate server stores plaintext.
4. **Forward secrecy** -- Compromising a long-term key does not reveal past messages.
5. **Minimal metadata** -- Community servers and relays see that two peers connected, but not what was said.

---

## End-to-End Encryption

### Key Derivation

Every Iroh node already has an ed25519 keypair (SecretKey/EndpointId). For DM encryption, we derive X25519 keys from the ed25519 keys, which is a well-established technique (used by Signal, libsodium, etc.):

```
ed25519 SecretKey  -->  X25519 private key  (via clamping/conversion)
ed25519 PublicKey  -->  X25519 public key   (via birational map)
```

This avoids requiring users to manage a separate encryption keypair -- their Iroh identity is their encryption identity.

### Protocol: Double Ratchet (Signal Protocol)

Use the Double Ratchet algorithm for message encryption:

1. **X3DH Key Agreement** (Extended Triple Diffie-Hellman) for initial session setup
2. **Double Ratchet** for ongoing message encryption with forward secrecy
3. **AEAD** (AES-256-GCM or ChaCha20-Poly1305) for symmetric encryption of each message

### Session Establishment (X3DH-like)

Since both peers are often online simultaneously (P2P), we simplify X3DH:

**When Alice wants to message Bob for the first time:**

1. Alice generates an ephemeral X25519 keypair.
2. Alice computes a shared secret from:
   - Alice's ephemeral private key + Bob's identity public key (X25519)
   - Alice's identity private key + Bob's identity public key (X25519)
3. Alice derives a root key and chain keys from the shared secret (HKDF).
4. Alice sends her ephemeral public key + encrypted first message to Bob.
5. Bob reconstructs the same shared secret using his private keys + Alice's public keys.
6. Both sides initialize the Double Ratchet with the shared root key.

**Subsequent messages** use the Double Ratchet:
- Each message advances the sending chain (new symmetric key per message).
- When a reply is received, a DH ratchet step occurs (new DH keypair exchanged).
- Old keys are deleted -- forward secrecy.

### Rust Crate

Use `double-ratchet-rs` or implement on top of:
- `x25519-dalek` -- X25519 Diffie-Hellman (already a transitive dep via iroh)
- `hkdf` -- Key derivation
- `chacha20poly1305` or `aes-gcm` -- AEAD symmetric encryption
- `ed25519-dalek` -- Already in use; convert keys to X25519

### Alternative: Noise Protocol for Session Establishment

Consider using the Noise Protocol Framework (`snow` crate) for the session establishment step instead of a hand-rolled X3DH. The `IK` handshake pattern is a close match: the initiator already knows the responder's static key (their iroh public key), and both sides authenticate and derive a shared secret in a formally verified pattern. This is the approach Signal uses -- Noise for session setup, Double Ratchet for ongoing per-message encryption.

Advantages over custom X3DH:
- Battle-tested framework (used by WireGuard, Lightning Network, libp2p, WhatsApp)
- Handles ephemeral key generation, DH computations, key derivation, and AEAD binding internally
- Less surface area for cryptographic bugs
- The `snow` crate is mature and already uses `x25519-dalek` + `chacha20poly1305` under the hood

Usage would look roughly like:
```rust
let mut initiator = snow::Builder::new("Noise_IK_25519_ChaChaPoly_BLAKE2s".parse()?)
    .local_private_key(&my_x25519_private)
    .remote_public_key(&peer_x25519_public)
    .build_initiator()?;
```

Noise does NOT replace the Double Ratchet -- it only covers session establishment. The Double Ratchet is still needed for per-message forward secrecy in long-lived conversations.

### Session Storage

Each DM conversation stores:
- Root key (current ratchet state)
- Sending chain key + message counter
- Receiving chain key + message counter
- Skipped message keys (for out-of-order delivery)
- Peer's current DH ratchet public key

This state is stored locally in redb, encrypted at rest with a key derived from the user's identity key.

---

## Direct Messaging

### Protocol: `iroh-social/dm/1`

A new custom ALPN for direct messaging over QUIC.

### Message Types

```rust
struct EncryptedEnvelope {
    sender: String,             // sender's EndpointId (pubkey)
    ephemeral_key: Option<Vec<u8>>,  // X25519 ephemeral public key (first msg or ratchet step)
    message_number: u32,        // for ordering and skipped-key management
    previous_chain_length: u32, // messages sent under previous ratchet
    ciphertext: Vec<u8>,        // AEAD-encrypted payload
    nonce: Vec<u8>,             // AEAD nonce
}

// Decrypted payload
struct DirectMessage {
    id: String,
    content: String,
    timestamp: u64,
    media: Vec<MediaAttachment>,  // reuse existing type
    reply_to: Option<String>,     // optional reply threading
}

// Control messages (also encrypted)
enum DmControl {
    Message(DirectMessage),
    Typing,
    Read { message_id: String },
    Delete { message_id: String },
}
```

### Connection Lifecycle

**Sending a message (peer online):**

1. Connect to peer: `endpoint.connect(addr, DM_ALPN)`
2. Open a bidirectional stream.
3. Encrypt `DmControl::Message(msg)` with the Double Ratchet.
4. Send `EncryptedEnvelope` as JSON.
5. Receive acknowledgment (also encrypted).
6. Keep connection open for continued conversation.

**Sending a message (peer offline):**

1. Connection attempt fails or times out.
2. Store message locally in an outbox table, marked as `pending`.
3. On next successful connection to the peer, send all pending messages in order.
4. Pending messages are encrypted at rest using the current ratchet state.

**Receiving messages:**

1. `DmHandler` implements `ProtocolHandler` (same pattern as `SyncHandler`).
2. On incoming connection: accept bidirectional stream.
3. Read `EncryptedEnvelope`, decrypt with Double Ratchet.
4. Store decrypted message in local DB.
5. Emit `dm-received` Tauri event to frontend.
6. Send encrypted acknowledgment.

### Offline Delivery Strategy

Since there is no central server to buffer messages, offline delivery relies on:

1. **Outbox + retry** -- Messages queued locally, sent on next connection.
2. **Background connection attempts** -- Periodically try to connect to peers with pending outbox messages (e.g., every 60 seconds, same cadence as post sync).
3. **Gossip hint (optional)** -- When a peer comes online, their gossip activity signals availability. The DM system can use gossip presence as a trigger to flush the outbox.

### Conversation Management

A conversation is identified by the sorted pair of participant pubkeys:

```
conversation_id = SHA256(sort(pubkey_a, pubkey_b))
```

This ensures both sides derive the same conversation ID regardless of who initiated.

---

## Storage

### New Tables (Client)

```rust
// DM conversations
const CONVERSATIONS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("conversations");
// Key: conversation_id (SHA256 of sorted pubkey pair)
// Value: ConversationMeta { peer_pubkey, last_message_at, unread_count }

// DM messages (encrypted at rest)
const MESSAGES_TABLE: TableDefinition<(u64, &str), &[u8]> = TableDefinition::new("messages");
// Key: (timestamp, message_id)
// Value: StoredMessage { from, to, content, timestamp, media, read, reply_to }

// DM outbox (pending messages for offline peers)
const OUTBOX_TABLE: TableDefinition<(u64, &str), &[u8]> = TableDefinition::new("dm_outbox");
// Key: (timestamp, message_id)
// Value: PendingMessage { to, encrypted_envelope }

// Ratchet session state
const RATCHET_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("ratchet_sessions");
// Key: peer_pubkey
// Value: serialized ratchet state (root key, chain keys, counters, skipped keys)
```

### Data Types

```rust
struct ConversationMeta {
    peer_pubkey: String,
    last_message_at: u64,
    unread_count: u32,
}

struct StoredMessage {
    id: String,
    from: String,
    to: String,
    content: String,
    timestamp: u64,
    media: Vec<MediaAttachment>,
    read: bool,
    reply_to: Option<String>,
}
```

---

## Client Integration

### Tauri Commands

```
send_dm(to, content, media?, reply_to?)    -> DirectMessage
get_conversations()                         -> Vec<ConversationMeta>
get_messages(peer, limit?, before?)         -> Vec<StoredMessage>
mark_read(peer, message_id)                 -> ()
delete_message(message_id)                  -> ()
flush_outbox()                              -> { sent: u32, failed: u32 }
```

### Tauri Events (Backend -> Frontend)

```
dm-received          { from, message }
dm-delivered         { message_id }
dm-read              { peer, message_id }
typing-indicator     { peer }
```

### Frontend Pages

**`/messages` page:**
- Conversation list (sorted by last message time)
- Unread count badges
- Click to open conversation

**`/messages/[pubkey]` page:**
- Message thread with the peer
- Text input with send button
- Media attachment support (reuse existing blob upload)
- Typing indicator
- Voice call / video call buttons in header (see [voice-video-calling.md](voice-video-calling.md))
- Message status (sent, delivered, read)

### Navigation Update

Add "Messages" tab to `+layout.svelte` navigation bar (alongside Feed, Profile, Follows, Servers).

### TypeScript Types

```typescript
interface ConversationMeta {
    peer_pubkey: string;
    last_message_at: number;
    unread_count: number;
}

interface StoredMessage {
    id: string;
    from: string;
    to: string;
    content: string;
    timestamp: number;
    media: MediaAttachment[];
    read: boolean;
    reply_to: string | null;
}
```

---

## Implementation Roadmap

### Phase 1: E2E Encryption Foundation
- [ ] Add `x25519-dalek`, `hkdf`, `chacha20poly1305` dependencies
- [ ] Implement ed25519-to-X25519 key conversion
- [ ] Implement X3DH-style session establishment
- [ ] Implement Double Ratchet (or integrate `double-ratchet-rs`)
- [ ] Add ratchet session storage to redb
- [ ] Write encryption/decryption tests

### Phase 2: Direct Messaging
- [ ] Define `DM_ALPN` protocol (`iroh-social/dm/1`)
- [ ] Implement `DmHandler` (ProtocolHandler for incoming DMs)
- [ ] Implement DM sending (connect to peer, encrypt, send envelope)
- [ ] Add message and conversation storage tables
- [ ] Add outbox for offline peers with retry logic
- [ ] Implement Tauri commands: `send_dm`, `get_conversations`, `get_messages`, `mark_read`
- [ ] Emit `dm-received` events to frontend
- [ ] Build `/messages` conversation list page
- [ ] Build `/messages/[pubkey]` chat thread page
- [ ] Add "Messages" tab to navigation
- [ ] Add typing indicators and read receipts

### Phase 3: Polish
- [ ] Message search within conversations
- [ ] Group DM support (3+ participants, shared ratchet)
- [ ] Push notification integration (OS-level)
- [ ] Encrypted message backup/export
