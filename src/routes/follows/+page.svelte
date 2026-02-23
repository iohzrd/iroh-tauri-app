<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import type { FollowEntry, FollowerEntry } from "$lib/types";
  import {
    avatarColor,
    getInitials,
    shortId,
    getDisplayName,
    copyToClipboard,
  } from "$lib/utils";

  let follows = $state<FollowEntry[]>([]);
  let followers = $state<FollowerEntry[]>([]);
  let newPubkey = $state("");
  let loading = $state(true);
  let status = $state("");
  let copyFeedback = $state("");
  let pendingUnfollowPubkey = $state<string | null>(null);
  let activeTab = $state<"following" | "followers">("following");

  async function copyWithFeedback(text: string, label: string) {
    await copyToClipboard(text);
    copyFeedback = label;
    setTimeout(() => (copyFeedback = ""), 1500);
  }

  async function init() {
    try {
      await invoke("get_node_id"); // wait for node ready
      await loadFollows();
      await loadFollowers();
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

  async function loadFollowers() {
    try {
      followers = await invoke("get_followers");
    } catch (e) {
      console.error("Failed to load followers:", e);
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

  function confirmUnfollow(pubkey: string) {
    pendingUnfollowPubkey = pubkey;
  }

  async function executeUnfollow() {
    if (!pendingUnfollowPubkey) return;
    try {
      await invoke("unfollow_user", { pubkey: pendingUnfollowPubkey });
      await loadFollows();
    } catch (e) {
      status = `Error: ${e}`;
    }
    pendingUnfollowPubkey = null;
  }

  function cancelUnfollow() {
    pendingUnfollowPubkey = null;
  }

  function handleKey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      followUser();
    }
  }

  onMount(() => {
    init();

    const unlisteners: Promise<UnlistenFn>[] = [];
    unlisteners.push(
      listen("follower-changed", () => {
        loadFollowers();
      }),
    );
    unlisteners.push(
      listen("new-follower", () => {
        loadFollowers();
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
    <p>Loading...</p>
  </div>
{:else}
  <div class="tabs">
    <button
      class="tab"
      class:active={activeTab === "following"}
      onclick={() => (activeTab = "following")}
    >
      Following ({follows.length})
    </button>
    <button
      class="tab"
      class:active={activeTab === "followers"}
      onclick={() => (activeTab = "followers")}
    >
      Followers ({followers.length})
    </button>
  </div>

  {#if activeTab === "following"}
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

    {#if pendingUnfollowPubkey}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div class="modal-overlay" onclick={cancelUnfollow} role="presentation">
        <!-- svelte-ignore a11y_interactive_supports_focus -->
        <div
          class="modal"
          onclick={(e) => e.stopPropagation()}
          role="dialog"
          aria-label="Confirm unfollow"
        >
          <p>Unfollow this user? You will stop receiving their posts.</p>
          <div class="modal-actions">
            <button class="modal-cancel" onclick={cancelUnfollow}>Cancel</button
            >
            <button class="modal-confirm" onclick={executeUnfollow}
              >Unfollow</button
            >
          </div>
        </div>
      </div>
    {/if}

    <div class="follow-list">
      {#each follows as f (f.pubkey)}
        <div class="follow-item">
          <a href="/user/{f.pubkey}" class="follow-info">
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
          </a>
          <div class="follow-actions">
            <button
              class="copy-btn"
              onclick={() => copyWithFeedback(f.pubkey, f.pubkey)}
            >
              {copyFeedback === f.pubkey ? "Copied!" : "Copy"}
            </button>
            <button
              class="unfollow-btn"
              onclick={() => confirmUnfollow(f.pubkey)}
            >
              Unfollow
            </button>
          </div>
        </div>
      {:else}
        <p class="empty">
          Not following anyone yet. Paste a Node ID above to follow someone!
        </p>
      {/each}
    </div>
  {:else}
    <div class="follow-list">
      {#each followers as f (f.pubkey)}
        <div class="follow-item">
          <a href="/user/{f.pubkey}" class="follow-info">
            {#await getDisplayName(f.pubkey, "") then name}
              <div class="avatar" style="background:{avatarColor(f.pubkey)}">
                {getInitials(name)}
              </div>
              <div class="follow-identity">
                {#if name !== shortId(f.pubkey)}
                  <span class="display-name">{name}</span>
                {/if}
                <code>{shortId(f.pubkey)}</code>
                <span class="online-status" class:online={f.is_online}>
                  {f.is_online ? "online" : "offline"}
                </span>
              </div>
            {/await}
          </a>
          <div class="follow-actions">
            <button
              class="copy-btn"
              onclick={() => copyWithFeedback(f.pubkey, f.pubkey)}
            >
              {copyFeedback === f.pubkey ? "Copied!" : "Copy"}
            </button>
          </div>
        </div>
      {:else}
        <p class="empty">
          No followers yet. Share your Node ID for others to follow you!
        </p>
      {/each}
    </div>
  {/if}
{/if}

<style>
  .tabs {
    display: flex;
    gap: 0;
    margin-bottom: 1rem;
    border-bottom: 1px solid #2a2a4a;
  }

  .tab {
    flex: 1;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    color: #888;
    font-size: 0.9rem;
    font-weight: 600;
    padding: 0.75rem;
    cursor: pointer;
    transition:
      color 0.2s,
      border-color 0.2s;
  }

  .tab:hover {
    color: #c4b5fd;
  }

  .tab.active {
    color: #a78bfa;
    border-bottom-color: #a78bfa;
  }

  .online-status {
    font-size: 0.7rem;
    color: #666;
  }

  .online-status.online {
    color: #22c55e;
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
    text-decoration: none;
    color: inherit;
    flex: 1;
    min-width: 0;
  }

  .follow-info:hover .display-name {
    text-decoration: underline;
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

  .follow-actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-shrink: 0;
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

  /* Unfollow confirmation modal */
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .modal {
    background: #16213e;
    border: 1px solid #2a2a4a;
    border-radius: 10px;
    padding: 1.5rem;
    max-width: 320px;
    width: 90%;
  }

  .modal p {
    margin: 0 0 1rem;
    text-align: center;
  }

  .modal-actions {
    display: flex;
    gap: 0.5rem;
  }

  .modal-cancel {
    flex: 1;
    background: #2a2a4a;
    color: #c4b5fd;
    border: none;
    border-radius: 6px;
    padding: 0.5rem;
    font-size: 0.9rem;
    cursor: pointer;
  }

  .modal-cancel:hover {
    background: #3a3a5a;
  }

  .modal-confirm {
    flex: 1;
    background: #dc2626;
    color: white;
    border: none;
    border-radius: 6px;
    padding: 0.5rem;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
  }

  .modal-confirm:hover {
    background: #b91c1c;
  }
</style>
