# Linked Devices - Design Document

Link multiple devices (phone, desktop, tablet) to a single identity so they share the same social graph, message history, and profile. Inspired by Signal's linked devices: a primary device holds the master identity, secondary devices are authorized via QR code pairing, and all devices stay in sync.

## Table of Contents

- [Design Principles](#design-principles)
- [Identity Model](#identity-model)
- [Pairing Protocol](#pairing-protocol)
- [Data Sync](#data-sync)
- [DM Considerations](#dm-considerations)
- [Storage](#storage)
- [Client Integration](#client-integration)
- [Implementation Roadmap](#implementation-roadmap)

---

## Design Principles

1. **Primary device is the authority** -- One device holds the master Ed25519 keypair. Secondary devices receive a delegated keypair and a signed authorization certificate. The primary can revoke any secondary at any time.
2. **No server required** -- Pairing and sync happen directly between devices over QUIC (same iroh transport). No relay or cloud service stores device keys or user data.
3. **Secure pairing** -- Devices pair via QR code containing a short-lived secret. The pairing channel is encrypted with Noise IK (same as DM session establishment) plus the QR secret as a pre-shared key, preventing man-in-the-middle attacks.
4. **Eventual consistency** -- Devices sync opportunistically. Each device works independently when offline and merges state when reconnected. Conflicts are resolved with last-write-wins by timestamp.
5. **Minimal trust expansion** -- Secondary devices can post, follow, and read DMs, but cannot authorize new devices or change the master identity. Only the primary can link/unlink devices.

---

## Identity Model

### Current State

Each device generates its own Ed25519 keypair (`identity.key`). The public key is the user's identity (EndpointId / pubkey). There is no concept of multiple devices sharing an identity.

### New Model

```
Primary Device
  - Master Ed25519 keypair (the user's pubkey, unchanged)
  - Signs DeviceCertificate for each secondary

Secondary Device
  - Own Ed25519 keypair (device-local, for QUIC transport)
  - Holds DeviceCertificate signed by primary
  - Posts and actions are signed by primary's key (via delegated signing key)
```

### Device Certificate

When a secondary device is linked, the primary signs a certificate that authorizes it:

```rust
struct DeviceCertificate {
    /// The primary device's pubkey (the user's identity).
    primary_pubkey: String,
    /// The secondary device's pubkey (its own EndpointId).
    device_pubkey: String,
    /// Human-readable device name (e.g. "Desktop", "Laptop").
    device_name: String,
    /// When this certificate was issued.
    issued_at: u64,
    /// Optional expiry (0 = no expiry).
    expires_at: u64,
    /// Ed25519 signature from the primary's secret key over the above fields.
    signature: Vec<u8>,
}
```

Other peers can verify that a secondary device is authorized by checking the certificate signature against the primary's public key. This is important for DMs -- a peer should accept encrypted messages from a secondary device only if it presents a valid certificate.

### Delegated Signing Key

For posting, the secondary needs to produce content that looks like it came from the primary's pubkey. Two options:

**Option A: Share the master secret key** (simpler but riskier)
- Primary sends its Ed25519 secret key to the secondary during pairing.
- Secondary can sign posts directly as the primary identity.
- Risk: if secondary is compromised, the master key is exposed.
- Mitigation: secondary stores the key encrypted at rest, primary can rotate if needed.

**Option B: Proxy signing** (safer but more complex)
- Secondary creates content, sends to primary for signing.
- Requires primary to be online for every action.
- Impractical for offline use.

**Recommended: Option A** with encrypted key storage, matching Signal's approach. The shared secret key is transferred once during pairing over the encrypted channel and stored encrypted on disk. The primary can unlink a device if compromised.

---

## Pairing Protocol

### Overview

```
Primary                              Secondary
   |                                     |
   |  1. Display QR code                 |
   |     (contains: primary pubkey,      |
   |      one-time secret, device addr)  |
   |                                     |
   |  2. Scan QR code  <-----------      |
   |                                     |
   |  3. Secondary connects via QUIC     |
   |     on LINK_ALPN                    |
   |     <----------------------------   |
   |                                     |
   |  4. Noise IK handshake + PSK        |
   |     (QR secret as pre-shared key)   |
   |     ---------------------------->   |
   |     <----------------------------   |
   |                                     |
   |  5. Primary sends:                  |
   |     - DeviceCertificate             |
   |     - Master secret key (encrypted) |
   |     - Full data export (profile,    |
   |       follows, conversations, etc.) |
   |     ---------------------------->   |
   |                                     |
   |  6. Secondary confirms receipt      |
   |     <----------------------------   |
   |                                     |
   |  [Paired - ongoing sync begins]     |
```

### ALPN

```rust
pub const LINK_ALPN: &[u8] = b"iroh-social/link/1";
```

### QR Code Content

```rust
struct LinkQrPayload {
    /// Primary device's EndpointId (pubkey).
    primary_pubkey: String,
    /// One-time secret for Noise PSK (32 bytes, base64-encoded).
    secret: String,
    /// Primary device's network address for direct connection.
    addrs: String,
}
```

Serialized as a compact URI: `iroh-social://link?pk=<pubkey>&s=<secret>&a=<addrs>`

The QR code expires after 60 seconds. The one-time secret is generated fresh each time the user initiates pairing.

### Noise IK + PSK Handshake

Use `Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s` -- the same Noise IK pattern used for DM sessions but with an additional pre-shared key (the QR secret). This ensures:

1. **Authentication** -- Both sides know each other's static keys after handshake.
2. **QR verification** -- Only someone who scanned the QR code knows the PSK, preventing MITM.
3. **Forward secrecy** -- Ephemeral keys ensure the pairing channel cannot be decrypted later.

### Data Transfer During Pairing

After the encrypted channel is established, the primary sends a `LinkBundle`:

```rust
struct LinkBundle {
    /// Signed certificate authorizing this device.
    certificate: DeviceCertificate,
    /// The primary's Ed25519 secret key (32 bytes).
    master_secret_key: [u8; 32],
    /// User profile.
    profile: Profile,
    /// Follow list.
    follows: Vec<FollowEntry>,
    /// DM conversation metadata (not message content -- too large).
    conversations: Vec<ConversationMeta>,
    /// DM ratchet sessions (serialized state for each peer).
    ratchet_sessions: Vec<(String, String)>, // (peer_pubkey, state_json)
}
```

Message history is synced separately after pairing completes (large data, can be incremental).

---

## Data Sync

After initial pairing, devices need to stay in sync. Sync happens over QUIC using a dedicated protocol.

### Sync ALPN

```rust
pub const DEVICE_SYNC_ALPN: &[u8] = b"iroh-social/device-sync/1";
```

### Sync Strategy

Devices discover each other via iroh's existing peer discovery. When a linked device is detected online:

1. **Connect** on `DEVICE_SYNC_ALPN`.
2. **Exchange sync vectors** -- each device sends a summary of what it has (latest timestamps per category).
3. **Transfer deltas** -- only send data the other device is missing.

### Sync Categories

| Category | Sync Method | Conflict Resolution |
|----------|------------|-------------------|
| Profile | Last-write-wins by timestamp | Latest edit wins |
| Follows | Set union | Add wins over remove (tombstone with timestamp) |
| Posts | Set union by (author, post_id) | Deduplicate, keep all |
| DM messages | Set union by message_id | Deduplicate, keep all |
| DM conversations | Merge: max(last_message_at), sum(unread) | Merge fields |
| Ratchet sessions | Latest updated_at wins | Most recent state wins |
| Blocked users | Set union | Add wins |

### Sync Vector

```rust
struct DeviceSyncVector {
    /// Device pubkey (secondary's own EndpointId).
    device_pubkey: String,
    /// Timestamp of last profile update.
    profile_updated_at: u64,
    /// Timestamp of last follow change.
    follows_updated_at: u64,
    /// Latest post timestamp.
    latest_post_at: u64,
    /// Per-conversation latest message timestamp.
    conversation_heads: Vec<(String, u64)>, // (conversation_id, latest_timestamp)
    /// Ratchet session update timestamps.
    ratchet_heads: Vec<(String, u64)>, // (peer_pubkey, updated_at)
}
```

### Sync Frequency

- **On reconnection** -- When a linked device comes online, sync immediately.
- **Periodic** -- Every 30 seconds while both devices are online (lightweight vector exchange).
- **Event-driven** -- After sending/receiving a DM or making a post, push to linked devices if online.

---

## DM Considerations

DMs with the Double Ratchet are the hardest part of multi-device. The ratchet is a sequential state machine -- two devices cannot independently advance the same ratchet without desynchronizing.

### Approach: Shared Ratchet with Sync

Since devices share the master secret key and the same identity, they share ratchet sessions:

1. **Before encrypting/decrypting**, acquire the latest ratchet state (check if a linked device has a newer version).
2. **After encrypting/decrypting**, push the updated ratchet state to linked devices.
3. **If a conflict occurs** (both devices advanced the ratchet simultaneously), the device with the higher message_number wins, and the other device re-derives from the winning state.

### Practical Simplification

In practice, users rarely type on two devices simultaneously in the same conversation. The sync window (30s periodic + event-driven push) is fast enough to avoid most conflicts. When conflicts do occur:

- The "losing" device detects the conflict on next sync (its ratchet state has a different root key than the synced version).
- It adopts the winning state and re-encrypts any unsent messages.
- Messages already sent with the old state are still decryptable by the recipient (ratchet allows out-of-order decryption via skipped message keys).

### Receiving DMs on Secondary Devices

Peers need to know about linked devices so they can send DMs to whichever device is online:

1. Primary publishes a `LinkedDevices` announcement via gossip (signed list of authorized device pubkeys).
2. When sending a DM, the sender tries the primary first, then falls back to secondary devices.
3. All devices share the same ratchet session, so any device can decrypt.

```rust
struct LinkedDevicesAnnouncement {
    primary_pubkey: String,
    devices: Vec<DeviceInfo>,
    timestamp: u64,
    signature: Vec<u8>, // signed by primary
}

struct DeviceInfo {
    device_pubkey: String,
    device_name: String,
}
```

---

## Storage

### New Migration: `005_linked_devices.sql`

```sql
-- Linked devices registry
CREATE TABLE IF NOT EXISTS linked_devices (
    device_pubkey TEXT PRIMARY KEY,
    device_name TEXT NOT NULL,
    is_primary INTEGER NOT NULL DEFAULT 0,
    certificate_json TEXT NOT NULL,
    linked_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL DEFAULT 0
);

-- Sync state tracking
CREATE TABLE IF NOT EXISTS device_sync_state (
    device_pubkey TEXT PRIMARY KEY REFERENCES linked_devices(device_pubkey),
    sync_vector_json TEXT NOT NULL DEFAULT '{}',
    last_sync_at INTEGER NOT NULL DEFAULT 0
);
```

### New Storage Methods

```rust
// Device management
fn add_linked_device(device: &LinkedDevice) -> Result<()>
fn remove_linked_device(device_pubkey: &str) -> Result<()>
fn get_linked_devices() -> Result<Vec<LinkedDevice>>
fn update_device_last_seen(device_pubkey: &str, timestamp: u64) -> Result<()>
fn is_device_linked(device_pubkey: &str) -> Result<bool>

// Sync state
fn get_sync_state(device_pubkey: &str) -> Result<Option<DeviceSyncVector>>
fn update_sync_state(device_pubkey: &str, vector: &DeviceSyncVector) -> Result<()>

// Data export for initial pairing
fn export_link_bundle(master_secret: &[u8; 32], certificate: &DeviceCertificate) -> Result<LinkBundle>
fn import_link_bundle(bundle: &LinkBundle) -> Result<()>
```

---

## Client Integration

### New Types (`crates/iroh-social-types/src/link.rs`)

```rust
pub const LINK_ALPN: &[u8] = b"iroh-social/link/1";
pub const DEVICE_SYNC_ALPN: &[u8] = b"iroh-social/device-sync/1";

pub struct DeviceCertificate { ... }
pub struct LinkQrPayload { ... }
pub struct LinkBundle { ... }
pub struct DeviceSyncVector { ... }
pub struct LinkedDevicesAnnouncement { ... }
pub struct DeviceInfo { ... }
```

### Tauri Commands

```
// Pairing
start_device_link()                -> LinkQrPayload  // generates QR, starts listening
cancel_device_link()               -> ()
link_with_primary(qr_payload)      -> DeviceCertificate  // secondary scans QR, pairs

// Device management
get_linked_devices()               -> Vec<LinkedDevice>
unlink_device(device_pubkey)       -> ()
rename_device(device_pubkey, name) -> ()
is_primary_device()                -> bool

// Sync
force_device_sync()                -> { synced_items: u32 }
```

### Tauri Events

```
device-link-started    { qr_uri: String }           // QR code ready to display
device-link-progress   { step: String }              // pairing progress updates
device-linked          { device: LinkedDevice }      // pairing complete
device-unlinked        { device_pubkey: String }     // device removed
device-sync-complete   { device_pubkey, items: u32 } // sync finished
```

### Frontend Pages

**`/settings/devices` page:**

- List of linked devices (name, pubkey, last seen, primary/secondary badge)
- "Link New Device" button (primary only) -- shows QR code
- "Link to Primary" button (for new devices) -- opens camera/paste input
- Unlink button per device (primary only)
- Rename device

**QR Code Display (Primary):**

- Full-screen QR code with countdown timer (60s expiry)
- "Waiting for secondary to scan..." status
- Cancel button

**QR Scanner (Secondary):**

- Camera viewfinder for scanning
- Manual paste option (for desktop-to-desktop pairing)
- Progress indicator during pairing

---

## Implementation Roadmap

### Phase 1: Identity & Certificates

- [ ] Define `DeviceCertificate` type with Ed25519 signing/verification
- [ ] Implement certificate creation (primary signs for secondary)
- [ ] Implement certificate verification
- [ ] Add `linked_devices` and `device_sync_state` tables (migration 005)
- [ ] Add storage methods for device management
- [ ] Add `is_primary` / `master_secret_key` fields to local storage
- [ ] Write certificate signing/verification tests

### Phase 2: Pairing Protocol

- [ ] Define `LINK_ALPN` and wire types
- [ ] Implement QR code payload generation (with one-time secret)
- [ ] Implement Noise IK + PSK handshake for pairing channel
- [ ] Implement `LinkBundle` export from primary
- [ ] Implement `LinkBundle` import on secondary
- [ ] Implement `LinkHandler` (ProtocolHandler for pairing)
- [ ] Add Tauri commands: `start_device_link`, `link_with_primary`, `cancel_device_link`
- [ ] Build QR code display page (primary side)
- [ ] Build QR scan / paste page (secondary side)
- [ ] Write pairing integration tests

### Phase 3: Device Sync

- [ ] Define `DEVICE_SYNC_ALPN` and sync vector types
- [ ] Implement sync vector generation from local state
- [ ] Implement delta computation (what the other device is missing)
- [ ] Implement `DeviceSyncHandler` (ProtocolHandler)
- [ ] Add periodic sync task (30s interval)
- [ ] Add event-driven sync (push after post/DM)
- [ ] Handle ratchet session sync with conflict resolution
- [ ] Add Tauri commands: `get_linked_devices`, `unlink_device`, `force_device_sync`
- [ ] Build `/settings/devices` management page
- [ ] Write sync conflict resolution tests

### Phase 4: DM Multi-Device

- [ ] Publish `LinkedDevicesAnnouncement` via gossip
- [ ] Update DM sender to try all linked devices when primary is offline
- [ ] Handle ratchet state conflicts between devices
- [ ] Sync DM message history between linked devices
- [ ] Update unread counts across devices
- [ ] Write multi-device DM tests

### Phase 5: Polish

- [ ] Device revocation (primary unlinks, secondary wipes master key)
- [ ] Re-pairing after revocation
- [ ] Notification sync (dismiss on one device, dismiss on all)
- [ ] Bandwidth-aware sync (don't sync large media over metered connections)
- [ ] Sync progress UI (show what's syncing, how much remains)
