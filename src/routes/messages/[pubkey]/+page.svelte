<script lang="ts">
  import { page } from "$app/state";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { platform } from "@tauri-apps/plugin-os";
  import { onMount } from "svelte";
  import Avatar from "$lib/Avatar.svelte";
  import type { StoredMessage, Profile, PendingAttachment } from "$lib/types";
  import {
    shortId,
    getDisplayName,
    getCachedAvatarTicket,
    isImage,
    isVideo,
    isAudio,
    formatSize,
  } from "$lib/utils";
  import { createBlobCache } from "$lib/blobs";
  import { hapticNotification } from "$lib/haptics";

  const isMobile = platform() === "android" || platform() === "ios";
  let pubkey: string = $derived(page.params.pubkey ?? "");
  let nodeId = $state("");
  let peerName = $state("");
  let peerProfile = $state<Profile | null>(null);
  let messages = $state<StoredMessage[]>([]);
  let loading = $state(true);
  let messageText = $state("");
  let sending = $state(false);
  let sendError = $state("");
  let hasMore = $state(true);
  let loadingMore = $state(false);
  let messagesContainer = $state<HTMLDivElement>(null!);
  let shouldAutoScroll = $state(true);
  let peerTyping = $state(false);
  let typingTimeout: ReturnType<typeof setTimeout> | null = null;
  let lastTypingSent = 0;
  let attachments = $state<PendingAttachment[]>([]);
  let uploading = $state(false);
  let fileInput = $state<HTMLInputElement>(null!);
  let cameraInput = $state<HTMLInputElement>(null!);

  // Message delivery tracking
  const sendTimestamps = new Map<string, number>();
  let failedIds = $state(new Set<string>());
  let retryingIds = $state(new Set<string>());
  const SEND_TIMEOUT_MS = 30_000;

  const blobs = createBlobCache();

  async function init() {
    try {
      nodeId = await invoke("get_node_id");
      peerName = await getDisplayName(pubkey, nodeId);
      try {
        peerProfile = await invoke("get_remote_profile", { pubkey });
      } catch {
        // peer profile may not be available
      }
      const msgs: StoredMessage[] = await invoke("get_dm_messages", {
        peerPubkey: pubkey,
        limit: 50,
        before: null,
      });
      messages = msgs;
      hasMore = msgs.length >= 50;
      loading = false;

      await invoke("mark_dm_read", { peerPubkey: pubkey });

      // Send read receipts for unread incoming messages
      for (const msg of msgs) {
        if (msg.from_pubkey !== nodeId && !msg.read) {
          invoke("send_dm_signal", {
            to: pubkey,
            signalType: "read",
            messageId: msg.id,
          }).catch(() => {});
        }
      }

      requestAnimationFrame(() => scrollToBottom());
    } catch {
      setTimeout(init, 500);
    }
  }

  function scrollToBottom() {
    if (messagesContainer) {
      messagesContainer.scrollTop = messagesContainer.scrollHeight;
    }
  }

  function handleScroll() {
    if (!messagesContainer) return;
    const { scrollTop, scrollHeight, clientHeight } = messagesContainer;
    shouldAutoScroll = scrollHeight - scrollTop - clientHeight < 100;

    if (scrollTop < 100 && hasMore && !loadingMore) {
      loadOlder();
    }
  }

  async function loadOlder() {
    if (loadingMore || !hasMore || messages.length === 0) return;
    loadingMore = true;
    try {
      const oldest = messages[0];
      const olderMsgs: StoredMessage[] = await invoke("get_dm_messages", {
        peerPubkey: pubkey,
        limit: 50,
        before: oldest.timestamp,
      });
      if (olderMsgs.length === 0) {
        hasMore = false;
      } else {
        const prevHeight = messagesContainer?.scrollHeight ?? 0;
        messages = [...olderMsgs, ...messages];
        hasMore = olderMsgs.length >= 50;
        requestAnimationFrame(() => {
          if (messagesContainer) {
            const newHeight = messagesContainer.scrollHeight;
            messagesContainer.scrollTop = newHeight - prevHeight;
          }
        });
      }
    } catch (e) {
      console.error("Failed to load older messages:", e);
    }
    loadingMore = false;
  }

  function sendTypingSignal() {
    const now = Date.now();
    if (now - lastTypingSent < 3000) return;
    lastTypingSent = now;
    invoke("send_dm_signal", {
      to: pubkey,
      signalType: "typing",
      messageId: null,
    }).catch(() => {});
  }

  function handleInput() {
    sendTypingSignal();
  }

  async function handleFiles(e: Event) {
    const input = e.target as HTMLInputElement;
    const files = input.files;
    if (!files || files.length === 0) return;

    uploading = true;
    for (const file of files) {
      try {
        const buffer = await file.arrayBuffer();
        const data = Array.from(new Uint8Array(buffer));
        const result: { hash: string; ticket: string } = await invoke(
          "add_blob_bytes",
          { data },
        );

        const previewUrl = URL.createObjectURL(file);
        attachments = [
          ...attachments,
          {
            hash: result.hash,
            ticket: result.ticket,
            mime_type: file.type || "application/octet-stream",
            filename: file.name,
            size: file.size,
            previewUrl,
          },
        ];
      } catch (e) {
        console.error("Failed to upload file:", file.name, e);
      }
    }
    uploading = false;
    input.value = "";
  }

  function removeAttachment(index: number) {
    const removed = attachments[index];
    if (removed) URL.revokeObjectURL(removed.previewUrl);
    attachments = attachments.filter((_, i) => i !== index);
  }

  async function sendMessage() {
    const text = messageText.trim();
    if ((!text && attachments.length === 0) || sending) return;
    sending = true;
    sendError = "";
    try {
      const media =
        attachments.length > 0
          ? attachments.map(({ hash, ticket, mime_type, filename, size }) => ({
              hash,
              ticket,
              mime_type,
              filename,
              size,
            }))
          : null;
      const msg: StoredMessage = await invoke("send_dm", {
        to: pubkey,
        content: text,
        media,
      });
      messages = [...messages, msg];
      sendTimestamps.set(msg.id, Date.now());
      hapticNotification("success");
      messageText = "";
      for (const a of attachments) URL.revokeObjectURL(a.previewUrl);
      attachments = [];
      requestAnimationFrame(() => scrollToBottom());
    } catch (e) {
      console.error("Failed to send message:", e);
      sendError = String(e);
      setTimeout(() => (sendError = ""), 5000);
    }
    sending = false;
  }

  async function retryMessage(msgId: string) {
    retryingIds.add(msgId);
    retryingIds = new Set(retryingIds);
    failedIds.delete(msgId);
    failedIds = new Set(failedIds);
    sendTimestamps.set(msgId, Date.now());
    try {
      await invoke("flush_dm_outbox");
    } catch (e) {
      console.error("Retry failed:", e);
    }
    retryingIds.delete(msgId);
    retryingIds = new Set(retryingIds);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  }

  function formatTime(ts: number): string {
    return new Date(ts).toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  function isSameDay(ts1: number, ts2: number): boolean {
    const d1 = new Date(ts1);
    const d2 = new Date(ts2);
    return (
      d1.getFullYear() === d2.getFullYear() &&
      d1.getMonth() === d2.getMonth() &&
      d1.getDate() === d2.getDate()
    );
  }

  function formatDate(ts: number): string {
    const d = new Date(ts);
    const today = new Date();
    if (isSameDay(ts, today.getTime())) return "Today";
    const yesterday = new Date(today);
    yesterday.setDate(yesterday.getDate() - 1);
    if (isSameDay(ts, yesterday.getTime())) return "Yesterday";
    return d.toLocaleDateString([], {
      month: "short",
      day: "numeric",
      year: d.getFullYear() !== today.getFullYear() ? "numeric" : undefined,
    });
  }

  function shouldShowDate(index: number): boolean {
    if (index === 0) return true;
    return !isSameDay(messages[index].timestamp, messages[index - 1].timestamp);
  }

  // Check for timed-out messages every 5s
  $effect(() => {
    const interval = setInterval(() => {
      const now = Date.now();
      let changed = false;
      for (const [msgId, sentAt] of sendTimestamps) {
        const msg = messages.find((m) => m.id === msgId);
        if (!msg || msg.delivered || msg.read) {
          sendTimestamps.delete(msgId);
          failedIds.delete(msgId);
          changed = true;
          continue;
        }
        if (now - sentAt > SEND_TIMEOUT_MS && !retryingIds.has(msgId)) {
          failedIds.add(msgId);
          changed = true;
        }
      }
      if (changed) {
        failedIds = new Set(failedIds);
      }
    }, 5000);
    return () => clearInterval(interval);
  });

  onMount(() => {
    init();
    const unlisteners: Promise<UnlistenFn>[] = [];
    unlisteners.push(
      listen("dm-received", (event) => {
        const payload = event.payload as {
          from: string;
          message: StoredMessage;
        };
        if (payload.from === pubkey) {
          messages = [...messages, payload.message];
          invoke("mark_dm_read", { peerPubkey: pubkey });
          // Send read receipt
          invoke("send_dm_signal", {
            to: pubkey,
            signalType: "read",
            messageId: payload.message.id,
          }).catch(() => {});
          if (shouldAutoScroll) {
            requestAnimationFrame(() => scrollToBottom());
          }
        }
      }),
    );
    unlisteners.push(
      listen("dm-delivered", (event) => {
        const payload = event.payload as { message_id: string };
        messages = messages.map((m) =>
          m.id === payload.message_id ? { ...m, delivered: true } : m,
        );
        sendTimestamps.delete(payload.message_id);
        failedIds.delete(payload.message_id);
        retryingIds.delete(payload.message_id);
        failedIds = new Set(failedIds);
        retryingIds = new Set(retryingIds);
      }),
    );
    unlisteners.push(
      listen("typing-indicator", (event) => {
        const payload = event.payload as { peer: string };
        if (payload.peer === pubkey) {
          peerTyping = true;
          if (typingTimeout) clearTimeout(typingTimeout);
          typingTimeout = setTimeout(() => {
            peerTyping = false;
          }, 4000);
        }
      }),
    );
    unlisteners.push(
      listen("dm-read", (event) => {
        const payload = event.payload as { message_id: string };
        messages = messages.map((m) =>
          m.id === payload.message_id ? { ...m, read: true } : m,
        );
      }),
    );
    return () => {
      if (typingTimeout) clearTimeout(typingTimeout);
      blobs.revokeAll();
      for (const a of attachments) URL.revokeObjectURL(a.previewUrl);
      unlisteners.forEach((p) => p.then((fn) => fn()));
    };
  });
</script>

<input
  type="file"
  multiple
  class="hidden-input"
  bind:this={fileInput}
  onchange={handleFiles}
/>
<input
  type="file"
  accept="image/*,video/*"
  capture="environment"
  class="hidden-input"
  bind:this={cameraInput}
  onchange={handleFiles}
/>

{#if loading}
  <div class="loading">
    <div class="spinner"></div>
    <p>Loading conversation...</p>
  </div>
{:else}
  <div class="chat-layout">
    <div class="chat-header">
      <a href="/messages" class="back-btn">&larr;</a>
      <Avatar
        {pubkey}
        name={peerName}
        ticket={peerProfile?.avatar_ticket ?? getCachedAvatarTicket(pubkey)}
        size={32}
      />
      <div class="header-info">
        <span class="header-name">{peerName}</span>
      </div>
    </div>

    <div
      class="messages-container"
      bind:this={messagesContainer}
      onscroll={handleScroll}
    >
      {#if loadingMore}
        <div class="loading-more">
          <span class="btn-spinner"></span> Loading...
        </div>
      {/if}

      {#each messages as msg, i (msg.id)}
        {#if shouldShowDate(i)}
          <div class="date-separator">
            <span>{formatDate(msg.timestamp)}</span>
          </div>
        {/if}
        <div
          class="message-row"
          class:sent={msg.from_pubkey === nodeId}
          class:received={msg.from_pubkey !== nodeId}
          class:failed-msg={msg.from_pubkey === nodeId && failedIds.has(msg.id)}
        >
          <div class="message-bubble">
            {#if msg.media && msg.media.length > 0}
              <div class="message-media">
                {#each msg.media as att}
                  {#if isImage(att.mime_type)}
                    {#await blobs.getBlobUrl(att) then url}
                      <img src={url} alt={att.filename} class="media-img" />
                    {/await}
                  {:else if isVideo(att.mime_type)}
                    {#await blobs.getBlobUrl(att) then url}
                      <video
                        src={url}
                        controls
                        class="media-video"
                        preload="metadata"
                      ></video>
                    {/await}
                  {:else if isAudio(att.mime_type)}
                    {#await blobs.getBlobUrl(att) then url}
                      <div class="audio-attachment">
                        <span class="audio-filename">{att.filename}</span>
                        <audio src={url} controls preload="metadata"></audio>
                      </div>
                    {/await}
                  {:else}
                    <button
                      class="file-attachment"
                      onclick={() => blobs.downloadFile(att)}
                    >
                      <span class="file-icon">&#128196;</span>
                      <span class="file-name">{att.filename}</span>
                      <span class="file-size">{formatSize(att.size)}</span>
                    </button>
                  {/if}
                {/each}
              </div>
            {/if}
            {#if msg.content}
              <p class="message-text">{msg.content}</p>
            {/if}
            <div class="message-meta">
              <span class="message-time">{formatTime(msg.timestamp)}</span>
              {#if msg.from_pubkey === nodeId}
                {#if msg.read}
                  <span class="delivery-status read" title="Read">Read</span>
                {:else if msg.delivered}
                  <span class="delivery-status delivered" title="Delivered"
                    >Delivered</span
                  >
                {:else if failedIds.has(msg.id)}
                  <button
                    class="delivery-status failed"
                    onclick={() => retryMessage(msg.id)}
                    title="Tap to retry"
                  >
                    Failed -- Tap to retry
                  </button>
                {:else if retryingIds.has(msg.id)}
                  <span class="delivery-status retrying">Retrying...</span>
                {:else}
                  <span class="delivery-status pending" title="Sending..."
                    >Sending</span
                  >
                {/if}
              {/if}
            </div>
          </div>
        </div>
      {:else}
        <div class="empty-chat">
          <p>No messages yet. Say hello!</p>
        </div>
      {/each}

      {#if peerTyping}
        <div class="typing-indicator">
          <span class="typing-name">{peerName}</span> is typing
          <span class="typing-dots">
            <span class="dot"></span>
            <span class="dot"></span>
            <span class="dot"></span>
          </span>
        </div>
      {/if}
    </div>

    {#if sendError}
      <div class="send-error">{sendError}</div>
    {/if}

    {#if attachments.length > 0}
      <div class="attachment-preview">
        {#each attachments as att, i}
          <div class="attachment-item">
            {#if isImage(att.mime_type)}
              <img src={att.previewUrl} alt={att.filename} />
            {:else}
              <span class="att-file">{att.filename}</span>
            {/if}
            <button class="att-remove" onclick={() => removeAttachment(i)}
              >x</button
            >
          </div>
        {/each}
      </div>
    {/if}

    <div class="compose-bar">
      {#if isMobile}
        <button
          class="attach-btn"
          onclick={() => cameraInput?.click()}
          disabled={uploading}
          title="Take photo"
        >
          {uploading ? "..." : "Cam"}
        </button>
      {/if}
      <button
        class="attach-btn"
        onclick={() => fileInput?.click()}
        disabled={uploading}
        title="Attach file"
      >
        {uploading ? "..." : "+"}
      </button>
      <textarea
        class="compose-input"
        placeholder="Type a message..."
        bind:value={messageText}
        onkeydown={handleKeydown}
        oninput={handleInput}
        rows="1"
      ></textarea>
      <button
        class="btn-accent send-btn"
        onclick={sendMessage}
        disabled={(!messageText.trim() && attachments.length === 0) || sending}
      >
        {sending ? "..." : "Send"}
      </button>
    </div>
  </div>
{/if}

<style>
  .chat-layout {
    display: flex;
    flex-direction: column;
    height: calc(100vh - 60px);
    margin: -1rem -1rem -2rem;
  }

  .chat-header {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.75rem 1rem;
    border-bottom: 1px solid var(--border);
    background: var(--bg-base);
    flex-shrink: 0;
  }

  .back-btn {
    color: var(--accent-medium);
    text-decoration: none;
    font-size: 1.2rem;
    padding: 0.25rem;
  }

  .back-btn:hover {
    color: var(--accent-light);
  }

  .header-info {
    flex: 1;
    min-width: 0;
  }

  .header-name {
    font-weight: 600;
    font-size: 0.95rem;
    color: var(--text-primary);
  }

  .messages-container {
    flex: 1;
    overflow-y: auto;
    padding: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .loading-more {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.4rem;
    padding: 0.5rem;
    color: var(--text-secondary);
    font-size: 0.8rem;
  }

  .date-separator {
    display: flex;
    justify-content: center;
    padding: 0.75rem 0 0.5rem;
  }

  .date-separator span {
    background: var(--bg-elevated);
    color: var(--text-secondary);
    font-size: 0.7rem;
    padding: 0.2rem 0.75rem;
    border-radius: 999px;
  }

  .message-row {
    display: flex;
  }

  .message-row.sent {
    justify-content: flex-end;
  }

  .message-row.received {
    justify-content: flex-start;
  }

  .message-bubble {
    max-width: 75%;
    padding: 0.5rem 0.75rem;
    border-radius: 12px;
    word-break: break-word;
  }

  .sent .message-bubble {
    background: var(--accent);
    color: white;
    border-bottom-right-radius: 4px;
  }

  .received .message-bubble {
    background: var(--bg-surface);
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-bottom-left-radius: 4px;
  }

  .message-text {
    margin: 0;
    white-space: pre-wrap;
    font-size: 0.9rem;
    line-height: 1.4;
  }

  .message-media {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    margin-bottom: 0.3rem;
  }

  .media-img {
    max-width: 100%;
    max-height: 300px;
    border-radius: 8px;
    object-fit: contain;
    cursor: pointer;
  }

  .media-video {
    max-width: 100%;
    max-height: 300px;
    border-radius: 8px;
  }

  .audio-attachment {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    width: 100%;
  }

  .audio-filename {
    color: var(--accent-light);
    font-size: 0.75rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .audio-attachment audio {
    width: 100%;
    height: 36px;
    border-radius: 4px;
  }

  .file-attachment {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    background: var(--bg-elevated);
    border: 1px solid var(--border-hover);
    border-radius: 6px;
    padding: 0.4rem 0.6rem;
    color: var(--accent-light);
    font-size: 0.8rem;
    cursor: pointer;
  }

  .file-attachment:hover {
    background: var(--bg-elevated-hover);
  }

  .file-icon {
    font-size: 1rem;
  }

  .file-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .file-size {
    color: var(--text-secondary);
    font-size: 0.7rem;
    flex-shrink: 0;
  }

  .message-meta {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin-top: 0.2rem;
    justify-content: flex-end;
  }

  .message-time {
    font-size: 0.65rem;
    opacity: 0.6;
  }

  .delivery-status {
    font-size: 0.6rem;
    opacity: 0.6;
  }

  .delivery-status.delivered {
    color: var(--color-delivered);
  }

  .delivery-status.read {
    color: var(--color-read);
  }

  .failed-msg .message-bubble {
    border: 1px solid var(--danger-bg);
  }

  .delivery-status.failed {
    background: none;
    border: none;
    color: var(--color-error-light);
    font-size: 0.6rem;
    cursor: pointer;
    padding: 0;
    text-decoration: underline;
    text-decoration-style: dotted;
  }

  .delivery-status.retrying {
    color: var(--color-warning);
  }

  .typing-indicator {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.3rem 0;
    color: var(--text-secondary);
    font-size: 0.75rem;
    font-style: italic;
  }

  .typing-name {
    color: var(--accent-medium);
    font-style: normal;
    font-weight: 600;
  }

  .typing-dots {
    display: inline-flex;
    gap: 2px;
    margin-left: 2px;
  }

  .dot {
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--text-secondary);
    animation: bounce 1.2s infinite;
  }

  .dot:nth-child(2) {
    animation-delay: 0.2s;
  }

  .dot:nth-child(3) {
    animation-delay: 0.4s;
  }

  @keyframes bounce {
    0%,
    60%,
    100% {
      transform: translateY(0);
    }
    30% {
      transform: translateY(-4px);
    }
  }

  .empty-chat {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-tertiary);
    font-size: 0.9rem;
  }

  .send-error {
    background: var(--color-error-light-bg);
    color: var(--color-error-light);
    font-size: 0.8rem;
    padding: 0.4rem 1rem;
    border-top: 1px solid var(--color-error-light-border);
  }

  .attachment-preview {
    display: flex;
    gap: 0.5rem;
    padding: 0.5rem 1rem;
    border-top: 1px solid var(--border);
    background: var(--bg-surface);
    overflow-x: auto;
    flex-shrink: 0;
  }

  .attachment-item {
    position: relative;
    flex-shrink: 0;
  }

  .attachment-item img {
    width: 60px;
    height: 60px;
    object-fit: cover;
    border-radius: 6px;
    border: 1px solid var(--border);
  }

  .att-file {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 60px;
    height: 60px;
    background: var(--bg-elevated);
    border-radius: 6px;
    font-size: 0.6rem;
    color: var(--text-secondary);
    text-align: center;
    padding: 4px;
    word-break: break-all;
    overflow: hidden;
  }

  .att-remove {
    position: absolute;
    top: -4px;
    right: -4px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--color-error-light);
    color: white;
    border: none;
    font-size: 0.6rem;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    line-height: 1;
  }

  .compose-bar {
    display: flex;
    align-items: flex-end;
    gap: 0.5rem;
    padding: 0.75rem 1rem;
    border-top: 1px solid var(--border);
    background: var(--bg-base);
    flex-shrink: 0;
  }

  .attach-btn {
    background: none;
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--accent-medium);
    font-size: 1.1rem;
    width: 36px;
    height: 36px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .attach-btn:hover:not(:disabled) {
    background: var(--bg-elevated);
    color: var(--accent-light);
  }

  .compose-input {
    flex: 1;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 0.5rem 0.75rem;
    color: var(--text-primary);
    font-size: 0.9rem;
    resize: none;
    min-height: 36px;
    max-height: 120px;
    outline: none;
    transition: border-color 0.2s;
  }

  .compose-input:focus {
    border-color: var(--accent);
  }

  .compose-input::placeholder {
    color: var(--text-dim);
  }

  .send-btn {
    border-radius: 8px;
  }
</style>
