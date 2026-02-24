<script lang="ts">
  import { page } from "$app/state";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Avatar from "$lib/Avatar.svelte";
  import Timeago from "$lib/Timeago.svelte";
  import type { StoredMessage, Profile } from "$lib/types";
  import { shortId, getDisplayName, getCachedAvatarTicket } from "$lib/utils";

  let pubkey: string = $derived(page.params.pubkey ?? "");
  let nodeId = $state("");
  let peerName = $state("");
  let peerProfile = $state<Profile | null>(null);
  let messages = $state<StoredMessage[]>([]);
  let loading = $state(true);
  let messageText = $state("");
  let sending = $state(false);
  let hasMore = $state(true);
  let loadingMore = $state(false);
  let messagesContainer = $state<HTMLDivElement>(null!);
  let shouldAutoScroll = $state(true);

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
      messages = msgs.reverse();
      hasMore = msgs.length >= 50;
      loading = false;

      await invoke("mark_dm_read", { peerPubkey: pubkey });

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
        messages = [...olderMsgs.reverse(), ...messages];
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

  async function sendMessage() {
    const text = messageText.trim();
    if (!text || sending) return;
    sending = true;
    try {
      const msg: StoredMessage = await invoke("send_dm", {
        to: pubkey,
        content: text,
        media: null,
        replyTo: null,
      });
      messages = [...messages, msg];
      messageText = "";
      requestAnimationFrame(() => scrollToBottom());
    } catch (e) {
      console.error("Failed to send message:", e);
    }
    sending = false;
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
      }),
    );
    return () => {
      unlisteners.forEach((p) => p.then((fn) => fn()));
    };
  });
</script>

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
        >
          <div class="message-bubble">
            <p class="message-text">{msg.content}</p>
            <div class="message-meta">
              <span class="message-time">{formatTime(msg.timestamp)}</span>
              {#if msg.from_pubkey === nodeId}
                {#if msg.delivered}
                  <span class="delivery-status delivered" title="Delivered"
                    >Delivered</span
                  >
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
    </div>

    <div class="compose-bar">
      <textarea
        class="compose-input"
        placeholder="Type a message..."
        bind:value={messageText}
        onkeydown={handleKeydown}
        rows="1"
      ></textarea>
      <button
        class="send-btn"
        onclick={sendMessage}
        disabled={!messageText.trim() || sending}
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
    border-bottom: 1px solid #2a2a4a;
    background: #1a1a2e;
    flex-shrink: 0;
  }

  .back-btn {
    color: #a78bfa;
    text-decoration: none;
    font-size: 1.2rem;
    padding: 0.25rem;
  }

  .back-btn:hover {
    color: #c4b5fd;
  }

  .header-info {
    flex: 1;
    min-width: 0;
  }

  .header-name {
    font-weight: 600;
    font-size: 0.95rem;
    color: #e0e0e0;
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
    color: #888;
    font-size: 0.8rem;
  }

  .btn-spinner {
    display: inline-block;
    width: 14px;
    height: 14px;
    border: 2px solid #c4b5fd40;
    border-top-color: #c4b5fd;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    vertical-align: middle;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .date-separator {
    display: flex;
    justify-content: center;
    padding: 0.75rem 0 0.5rem;
  }

  .date-separator span {
    background: #2a2a4a;
    color: #888;
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
    background: #7c3aed;
    color: white;
    border-bottom-right-radius: 4px;
  }

  .received .message-bubble {
    background: #16213e;
    color: #e0e0e0;
    border: 1px solid #2a2a4a;
    border-bottom-left-radius: 4px;
  }

  .message-text {
    margin: 0;
    white-space: pre-wrap;
    font-size: 0.9rem;
    line-height: 1.4;
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
    color: #a7f3d0;
  }

  .sent .delivery-status.delivered {
    color: #a7f3d0;
  }

  .empty-chat {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #666;
    font-size: 0.9rem;
  }

  .compose-bar {
    display: flex;
    align-items: flex-end;
    gap: 0.5rem;
    padding: 0.75rem 1rem;
    border-top: 1px solid #2a2a4a;
    background: #1a1a2e;
    flex-shrink: 0;
  }

  .compose-input {
    flex: 1;
    background: #16213e;
    border: 1px solid #2a2a4a;
    border-radius: 8px;
    padding: 0.5rem 0.75rem;
    color: #e0e0e0;
    font-size: 0.9rem;
    font-family: inherit;
    resize: none;
    min-height: 36px;
    max-height: 120px;
    outline: none;
    transition: border-color 0.2s;
  }

  .compose-input:focus {
    border-color: #7c3aed;
  }

  .compose-input::placeholder {
    color: #555;
  }

  .send-btn {
    background: #7c3aed;
    color: white;
    border: none;
    border-radius: 8px;
    padding: 0.5rem 1rem;
    font-size: 0.85rem;
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
    font-family: inherit;
  }

  .send-btn:hover:not(:disabled) {
    background: #6d28d9;
  }

  .send-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
</style>
