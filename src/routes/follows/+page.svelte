<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import {
    avatarColor,
    getInitials,
    shortId,
    getDisplayName,
    copyToClipboard,
  } from "$lib/utils";

  interface FollowEntry {
    pubkey: string;
    alias: string | null;
    followed_at: number;
  }

  let follows = $state<FollowEntry[]>([]);
  let newPubkey = $state("");
  let loading = $state(true);
  let status = $state("");
  let copyFeedback = $state("");

  async function copyWithFeedback(text: string, label: string) {
    await copyToClipboard(text);
    copyFeedback = label;
    setTimeout(() => (copyFeedback = ""), 1500);
  }

  async function init() {
    try {
      await invoke("get_node_id"); // wait for node ready
      await loadFollows();
      loading = false;
    } catch {
      setTimeout(init, 500);
    }
  }

  async function loadFollows() {
    try {
      follows = await invoke("get_follows");
    } catch (e) {
      console.error("Failed to load follows:", e);
    }
  }

  async function followUser() {
    const pubkey = newPubkey.trim();
    if (!pubkey) return;
    status = "Following...";
    try {
      await invoke("follow_user", { pubkey });
      newPubkey = "";
      await loadFollows();
      status = "Followed!";
      setTimeout(() => (status = ""), 2000);
    } catch (e) {
      status = `Error: ${e}`;
    }
  }

  async function unfollowUser(pubkey: string) {
    try {
      await invoke("unfollow_user", { pubkey });
      await loadFollows();
    } catch (e) {
      status = `Error: ${e}`;
    }
  }

  function handleKey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      followUser();
    }
  }

  onMount(() => {
    init();
  });
</script>

{#if loading}
  <div class="loading">
    <div class="spinner"></div>
    <p>Loading...</p>
  </div>
{:else}
  <h2>Follows</h2>

  <div class="add-follow">
    <input
      bind:value={newPubkey}
      placeholder="Paste a Node ID to follow..."
      onkeydown={handleKey}
    />
    <button onclick={followUser} disabled={!newPubkey.trim()}>Follow</button>
  </div>

  {#if status}
    <p class="status">{status}</p>
  {/if}

  <div class="follow-list">
    {#each follows as f (f.pubkey)}
      <div class="follow-item">
        <div class="follow-info">
          {#await getDisplayName(f.pubkey, "") then name}
            <div class="avatar" style="background:{avatarColor(f.pubkey)}">
              {getInitials(name)}
            </div>
            <div class="follow-identity">
              {#if name !== shortId(f.pubkey)}
                <span class="display-name">{name}</span>
              {/if}
              <code>{shortId(f.pubkey)}</code>
            </div>
          {/await}
          <button
            class="copy-btn"
            onclick={() => copyWithFeedback(f.pubkey, f.pubkey)}
          >
            {copyFeedback === f.pubkey ? "Copied!" : "Copy"}
          </button>
        </div>
        <button class="unfollow-btn" onclick={() => unfollowUser(f.pubkey)}>
          Unfollow
        </button>
      </div>
    {:else}
      <p class="empty">
        Not following anyone yet. Paste a Node ID above to follow someone!
      </p>
    {/each}
  </div>
{/if}

<style>
  h2 {
    color: #a78bfa;
    margin: 0 0 1rem;
  }

  .add-follow {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }

  .add-follow input {
    flex: 1;
    background: #16213e;
    border: 1px solid #2a2a4a;
    border-radius: 6px;
    padding: 0.6rem 0.75rem;
    color: #e0e0e0;
    font-family: inherit;
    font-size: 0.85rem;
  }

  .add-follow input:focus {
    outline: none;
    border-color: #a78bfa;
  }

  .add-follow button {
    background: #7c3aed;
    color: white;
    border: none;
    border-radius: 6px;
    padding: 0.6rem 1rem;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
  }

  .add-follow button:hover:not(:disabled) {
    background: #6d28d9;
  }

  .add-follow button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .follow-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: #16213e;
    border: 1px solid #2a2a4a;
    border-radius: 8px;
    padding: 0.75rem 1rem;
    margin-bottom: 0.5rem;
  }

  .avatar {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 0.7rem;
    font-weight: 700;
    color: white;
    flex-shrink: 0;
    text-transform: uppercase;
  }

  .follow-info {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .follow-identity {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }

  .display-name {
    font-weight: 600;
    color: #c4b5fd;
    font-size: 0.85rem;
  }

  code {
    color: #7dd3fc;
    font-size: 0.85rem;
  }

  .copy-btn {
    background: #2a2a4a;
    color: #c4b5fd;
    border: none;
    border-radius: 4px;
    padding: 0.2rem 0.5rem;
    font-size: 0.7rem;
    cursor: pointer;
    min-width: 48px;
    text-align: center;
  }

  .copy-btn:hover {
    background: #3a3a5a;
  }

  .unfollow-btn {
    background: transparent;
    color: #f87171;
    border: 1px solid #f8717140;
    border-radius: 4px;
    padding: 0.3rem 0.75rem;
    font-size: 0.8rem;
    cursor: pointer;
  }

  .unfollow-btn:hover {
    background: #f8717120;
  }

  .empty {
    text-align: center;
    color: #666;
    padding: 2rem;
  }

  .status {
    text-align: center;
    color: #888;
    font-size: 0.85rem;
    margin: 0.5rem 0;
  }
</style>
