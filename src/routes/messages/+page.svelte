<script lang="ts">
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Avatar from "$lib/Avatar.svelte";
  import Timeago from "$lib/Timeago.svelte";
  import type { ConversationMeta } from "$lib/types";
  import { shortId, getDisplayName, getCachedAvatarTicket } from "$lib/utils";

  let conversations = $state<ConversationMeta[]>([]);
  let loading = $state(true);
  let nodeId = $state("");
  let names = $state<Record<string, string>>({});
  let newPubkey = $state("");
  let newError = $state("");

  async function loadConversations() {
    try {
      conversations = await invoke("get_conversations");
    } catch (e) {
      console.error("Failed to load conversations:", e);
    }
  }

  async function resolveNames(convos: ConversationMeta[]) {
    const updated: Record<string, string> = {};
    for (const c of convos) {
      updated[c.peer_pubkey] = await getDisplayName(c.peer_pubkey, nodeId);
    }
    names = updated;
  }

  async function init() {
    try {
      nodeId = await invoke("get_node_id");
      await loadConversations();
      await resolveNames(conversations);
      loading = false;
    } catch {
      setTimeout(init, 500);
    }
  }

  function startConversation() {
    const key = newPubkey.trim();
    if (!key) return;
    if (key === nodeId) {
      newError = "Cannot message yourself";
      setTimeout(() => (newError = ""), 2000);
      return;
    }
    newPubkey = "";
    newError = "";
    goto(`/messages/${key}`);
  }

  onMount(() => {
    init();
    const unlisteners: Promise<UnlistenFn>[] = [];
    unlisteners.push(
      listen("dm-received", () => {
        loadConversations().then(() => resolveNames(conversations));
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
    <p>Loading messages...</p>
  </div>
{:else}
  <h2 class="page-title">Messages</h2>

  <div class="new-conversation">
    <input
      type="text"
      class="new-input"
      placeholder="Paste a Node ID to start a conversation..."
      bind:value={newPubkey}
      onkeydown={(e) => e.key === "Enter" && startConversation()}
    />
    <button
      class="btn-accent new-btn"
      onclick={startConversation}
      disabled={!newPubkey.trim()}
    >
      Chat
    </button>
  </div>
  {#if newError}
    <p class="new-error">{newError}</p>
  {/if}

  {#if conversations.length === 0}
    <div class="empty">
      <p>No conversations yet.</p>
      <p class="hint">
        Send a direct message from a user's profile to start a conversation.
      </p>
    </div>
  {:else}
    <div class="conversation-list">
      {#each conversations as convo (convo.peer_pubkey)}
        <a href="/messages/{convo.peer_pubkey}" class="conversation-row">
          <Avatar
            pubkey={convo.peer_pubkey}
            name={names[convo.peer_pubkey] || shortId(convo.peer_pubkey)}
            ticket={getCachedAvatarTicket(convo.peer_pubkey)}
            size={44}
          />
          <div class="conversation-info">
            <div class="conversation-header">
              <span class="conversation-name">
                {names[convo.peer_pubkey] || shortId(convo.peer_pubkey)}
              </span>
              {#if convo.last_message_at > 0}
                <span class="conversation-time">
                  <Timeago timestamp={convo.last_message_at} />
                </span>
              {/if}
            </div>
            <div class="conversation-preview">
              {#if convo.last_message_preview}
                <span class="preview-text">{convo.last_message_preview}</span>
              {:else}
                <span class="preview-text empty-preview">No messages</span>
              {/if}
              {#if convo.unread_count > 0}
                <span class="unread-badge">{convo.unread_count}</span>
              {/if}
            </div>
          </div>
        </a>
      {/each}
    </div>
  {/if}
{/if}

<style>
  .page-title {
    color: var(--accent-medium);
  }

  .new-conversation {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }

  .new-input {
    flex: 1;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.5rem 0.75rem;
    color: var(--text-primary);
    font-size: 0.8rem;
    outline: none;
    transition: border-color 0.2s;
  }

  .new-input:focus {
    border-color: var(--accent);
  }

  .new-input::placeholder {
    color: var(--text-dim);
  }

  .new-error {
    color: var(--color-error-light);
    font-size: 0.8rem;
    margin: -0.5rem 0 0.75rem;
  }

  .conversation-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .conversation-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.75rem;
    border-radius: 8px;
    text-decoration: none;
    color: inherit;
    transition: background 0.15s;
  }

  .conversation-row:hover {
    background: var(--bg-surface);
  }

  .conversation-info {
    flex: 1;
    min-width: 0;
  }

  .conversation-header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 0.5rem;
  }

  .conversation-name {
    font-weight: 600;
    font-size: 0.9rem;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .conversation-time {
    color: var(--text-tertiary);
    font-size: 0.75rem;
    flex-shrink: 0;
  }

  .conversation-preview {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-top: 0.2rem;
  }

  .preview-text {
    color: var(--text-secondary);
    font-size: 0.8rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .empty-preview {
    font-style: italic;
    color: var(--text-dim);
  }

  .unread-badge {
    background: var(--accent);
    color: white;
    font-size: 0.7rem;
    font-weight: 700;
    border-radius: 999px;
    min-width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0 6px;
    flex-shrink: 0;
  }
</style>
