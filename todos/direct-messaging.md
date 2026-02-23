# Direct Messaging & Voice/Video Calls - Design Document

End-to-end encrypted direct messaging and peer-to-peer voice/video calling for Iroh Social. All private communication is encrypted such that only the two participants can read/hear it -- not relay servers, not community servers, not anyone else.

## Table of Contents

- [Design Principles](#design-principles)
- [End-to-End Encryption](#end-to-end-encryption)
- [Direct Messaging](#direct-messaging)
- [Voice Calling](#voice-calling)
- [Video Calling](#video-calling)
- [Storage](#storage)
- [Client Integration](#client-integration)
- [Implementation Roadmap](#implementation-roadmap)

---

## Design Principles

1. **E2E encrypted by default** -- All DMs are encrypted with keys derived from a Diffie-Hellman exchange between the two participants. The QUIC transport layer (TLS 1.3) provides transport encryption, but E2E encryption ensures that even relay servers cannot read message content.
2. **No server dependency** -- DMs and calls work purely P2P. Community servers never see message content.
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

## Voice Calling

### Protocol: `iroh-social/call/1`

A separate ALPN for voice/video calls, distinct from DMs.

### Call Signaling

Signaling messages are E2E encrypted using the same Double Ratchet session as DMs (or a derived session). This means call metadata (who called whom) is also encrypted.

```rust
enum CallSignal {
    Offer {
        call_id: String,
        audio_codec: AudioCodec,
        video: bool,            // audio-only vs audio+video
    },
    Answer {
        call_id: String,
        audio_codec: AudioCodec,
        video: bool,
    },
    Reject {
        call_id: String,
        reason: String,
    },
    End {
        call_id: String,
    },
    IceCandidate {              // not needed with iroh (NAT handled by relay)
        call_id: String,        // reserved for future
        candidate: String,
    },
}

enum AudioCodec {
    Opus48k,    // 48kHz, preferred
    Opus24k,    // 24kHz, lower bandwidth
}
```

### Call Setup Flow

```
Alice                              Bob
  |                                  |
  |-- [DM_ALPN] CallSignal::Offer ->|
  |                                  | (ring notification)
  |<- [DM_ALPN] CallSignal::Answer -|
  |                                  |
  |== [CALL_ALPN] QUIC connection ==|
  |                                  |
  |-- audio stream (opus frames) --> |
  |<-- audio stream (opus frames) --|
  |                                  |
  |-- CallSignal::End ------------> |
  |                                  |
```

1. Alice sends `CallSignal::Offer` over the DM protocol (encrypted).
2. Bob receives it, shows incoming call UI.
3. Bob sends `CallSignal::Answer` over DM protocol.
4. Both sides open a QUIC connection on `CALL_ALPN`.
5. Audio streams flow over unidirectional QUIC streams.
6. Either side can send `CallSignal::End` to hang up.

### Audio Pipeline

**Capture (frontend, via WebView):**
```
navigator.mediaDevices.getUserMedia({ audio: true })
  -> MediaStream -> AudioContext -> ScriptProcessorNode / AudioWorklet
  -> PCM Float32 samples -> send to Rust via Tauri command
```

**Encode (Rust):**
```
PCM samples -> opus::Encoder (48kHz, mono, 20ms frames)
  -> Opus frame bytes -> QUIC unidirectional stream
```

**Decode (Rust):**
```
QUIC stream -> Opus frame bytes -> opus::Decoder
  -> PCM samples -> send to frontend via Tauri event
```

**Playback (frontend):**
```
Tauri event -> AudioContext -> AudioBuffer -> speaker output
```

### Audio Frame Format

```rust
struct AudioFrame {
    sequence: u32,          // monotonic counter for ordering
    timestamp_us: u64,      // capture timestamp (microseconds)
    data: Vec<u8>,          // Opus-encoded frame (typically 20ms)
}
```

Serialized as: `[4 bytes sequence][8 bytes timestamp][2 bytes length][data]` (binary, not JSON, for minimal overhead).

### Codec Configuration

- Opus 48kHz, mono, 20ms frames
- Variable bitrate: 16-32 kbps for voice
- Frame size: 960 samples at 48kHz = 20ms
- Complexity: 5 (balance of quality and CPU)

### Required Crates

```toml
opus = "0.3"                    # Opus codec bindings
```

### Platform Requirements

- **macOS:** `NSMicrophoneUsageDescription` in Info.plist
- **Linux:** PulseAudio or PipeWire (usually available)
- **Windows:** Windows Audio Session API (automatic)

---

## Video Calling

### Extension of Voice Calling

Video calling adds a video stream alongside the audio stream on the same QUIC connection.

### Stream Multiplexing

```
QUIC Connection (CALL_ALPN)
  Stream 0 (uni, local->remote): Audio frames (Opus)
  Stream 1 (uni, remote->local): Audio frames (Opus)
  Stream 2 (uni, local->remote): Video frames (VP9)
  Stream 3 (uni, remote->local): Video frames (VP9)
```

### Video Pipeline

**Capture (frontend):**
```
navigator.mediaDevices.getUserMedia({ video: { width: 640, height: 480, frameRate: 24 } })
  -> MediaStream -> <video> element -> Canvas
  -> canvas.toBlob('image/jpeg') or getImageData() -> raw pixels
  -> send to Rust via Tauri command (as bytes)
```

**Encode (Rust):**
```
Raw pixels -> VP9 encoder -> compressed frame
  -> QUIC unidirectional stream
```

**Decode (Rust):**
```
QUIC stream -> compressed frame -> VP9 decoder
  -> raw pixels -> send to frontend via Tauri event
```

**Render (frontend):**
```
Tauri event -> ImageData -> Canvas / <img> element
```

### Video Frame Format

```rust
struct VideoFrame {
    sequence: u32,
    timestamp_us: u64,
    key_frame: bool,        // I-frame (full) vs P-frame (delta)
    width: u16,
    height: u16,
    data: Vec<u8>,          // VP9-encoded frame
}
```

### Codec Configuration

- VP9, 640x480, 24fps
- Bitrate: 500-1500 kbps (adaptive based on connection quality)
- Keyframe interval: every 2 seconds (48 frames)
- Use `libvpx-sys` or `vpx` Rust bindings

### Adaptive Bitrate

Monitor QUIC stream throughput and adjust:
- If packet loss or latency increases: reduce resolution (320x240) and bitrate (250kbps)
- If bandwidth is good: increase to 1280x720 and 2000kbps
- Audio always takes priority over video bandwidth

### Required Crates

```toml
vpx-sys = "0.4"                 # VP9 encoder/decoder bindings
# or
libvpx = "0.1"                  # Higher-level VP9 wrapper
```

---

## Storage

### New redb Tables (Client)

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

// Call history
const CALL_HISTORY_TABLE: TableDefinition<(u64, &str), &[u8]> = TableDefinition::new("call_history");
// Key: (timestamp, call_id)
// Value: CallRecord { peer, direction, duration, had_video, timestamp }
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

struct CallRecord {
    call_id: String,
    peer: String,
    direction: CallDirection,  // Incoming | Outgoing
    duration_seconds: u64,
    had_video: bool,
    timestamp: u64,
    ended_reason: String,      // "completed", "rejected", "missed", "failed"
}
```

---

## Client Integration

### New Tauri Commands

```
// Messaging
send_dm(to, content, media?, reply_to?)    -> DirectMessage
get_conversations()                         -> Vec<ConversationMeta>
get_messages(peer, limit?, before?)         -> Vec<StoredMessage>
mark_read(peer, message_id)                 -> ()
delete_message(message_id)                  -> ()
flush_outbox()                              -> { sent: u32, failed: u32 }

// Calling
start_call(peer, video: bool)              -> CallSession
answer_call(call_id)                       -> ()
reject_call(call_id)                       -> ()
end_call(call_id)                          -> ()
send_audio_frame(call_id, data: Vec<u8>)   -> ()
send_video_frame(call_id, data: Vec<u8>)   -> ()
get_call_history(limit?, before?)           -> Vec<CallRecord>
toggle_video(call_id, enabled: bool)       -> ()
toggle_mute(call_id, muted: bool)          -> ()
```

### Tauri Events (Backend -> Frontend)

```
dm-received          { from, message }
dm-delivered         { message_id }
dm-read              { peer, message_id }
typing-indicator     { peer }
incoming-call        { call_id, peer, video }
call-answered        { call_id }
call-ended           { call_id, reason }
audio-frame          { call_id, data: Vec<u8> }
video-frame          { call_id, data: Vec<u8> }
```

### New Frontend Pages

**`/messages` page:**
- Conversation list (sorted by last message time)
- Unread count badges
- Click to open conversation

**`/messages/[pubkey]` page:**
- Message thread with the peer
- Text input with send button
- Media attachment support (reuse existing blob upload)
- Typing indicator
- Voice call / video call buttons in header
- Message status (sent, delivered, read)

**Call overlay component (global, in layout):**
- Incoming call notification with accept/reject
- Active call UI: timer, mute button, video toggle, end call
- Video display (if video call)
- Minimizable to floating pip

### Navigation Update

Add "Messages" tab to `+layout.svelte` navigation bar (alongside Feed, Profile, Follows, Servers).

### New TypeScript Types

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

interface CallRecord {
    call_id: string;
    peer: string;
    direction: "incoming" | "outgoing";
    duration_seconds: number;
    had_video: boolean;
    timestamp: number;
    ended_reason: string;
}

interface IncomingCall {
    call_id: string;
    peer: string;
    video: boolean;
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

### Phase 3: Voice Calling
- [ ] Add `opus` crate dependency
- [ ] Define `CALL_ALPN` protocol (`iroh-social/call/1`)
- [ ] Implement call signaling over DM protocol (offer/answer/reject/end)
- [ ] Implement `CallHandler` for audio stream multiplexing
- [ ] Implement Opus encoding/decoding in Rust
- [ ] Bridge audio capture (WebView getUserMedia -> Rust via Tauri command)
- [ ] Bridge audio playback (Rust -> WebView via Tauri event -> AudioContext)
- [ ] Implement Tauri commands: `start_call`, `answer_call`, `end_call`, `send_audio_frame`
- [ ] Build incoming call notification component
- [ ] Build active call overlay UI
- [ ] Add call history storage and display

### Phase 4: Video Calling
- [ ] Add VP9 codec dependency (`vpx-sys` or `libvpx`)
- [ ] Extend call signaling for video negotiation
- [ ] Implement video frame capture (Canvas -> Rust)
- [ ] Implement VP9 encoding/decoding
- [ ] Add video stream alongside audio on QUIC connection
- [ ] Build video display in call overlay (canvas rendering)
- [ ] Implement adaptive bitrate control
- [ ] Add video toggle during active call
- [ ] Picture-in-picture support for minimized calls

### Phase 5: Polish
- [ ] Connection quality indicator during calls
- [ ] Call reconnection on network change
- [ ] Message search within conversations
- [ ] Group DM support (3+ participants, shared ratchet)
- [ ] Push notification integration (OS-level)
- [ ] Encrypted message backup/export
