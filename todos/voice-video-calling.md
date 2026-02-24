# Voice & Video Calling - Design Document

End-to-end encrypted peer-to-peer voice and video calling for Iroh Social. Call signaling is encrypted using the same Double Ratchet session as DMs (see [direct-messaging.md](direct-messaging.md) for encryption details). Audio and video streams flow over a dedicated QUIC connection.

## Table of Contents

- [Voice Calling](#voice-calling)
- [Video Calling](#video-calling)
- [Storage](#storage)
- [Client Integration](#client-integration)
- [Implementation Roadmap](#implementation-roadmap)

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

### Call History Table

```rust
// Call history
const CALL_HISTORY_TABLE: TableDefinition<(u64, &str), &[u8]> = TableDefinition::new("call_history");
// Key: (timestamp, call_id)
// Value: CallRecord { peer, direction, duration, had_video, timestamp }
```

### Data Types

```rust
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

### Tauri Commands

```
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
incoming-call        { call_id, peer, video }
call-answered        { call_id }
call-ended           { call_id, reason }
audio-frame          { call_id, data: Vec<u8> }
video-frame          { call_id, data: Vec<u8> }
```

### Frontend Components

**Call overlay component (global, in layout):**

- Incoming call notification with accept/reject
- Active call UI: timer, mute button, video toggle, end call
- Video display (if video call)
- Minimizable to floating pip

### TypeScript Types

```typescript
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

### Phase 1: Voice Calling

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

### Phase 2: Video Calling

- [ ] Add VP9 codec dependency (`vpx-sys` or `libvpx`)
- [ ] Extend call signaling for video negotiation
- [ ] Implement video frame capture (Canvas -> Rust)
- [ ] Implement VP9 encoding/decoding
- [ ] Add video stream alongside audio on QUIC connection
- [ ] Build video display in call overlay (canvas rendering)
- [ ] Implement adaptive bitrate control
- [ ] Add video toggle during active call
- [ ] Picture-in-picture support for minimized calls

### Phase 3: Polish

- [ ] Connection quality indicator during calls
- [ ] Call reconnection on network change
