<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Avatar from "$lib/Avatar.svelte";
  import type { FollowEntry, FollowerEntry } from "$lib/types";
  import {
    shortId,
    getDisplayName,
    getCachedAvatarTicket,
    copyToClipboard,
  } from "$lib/utils";

  let follows = $state<FollowEntry[]>([]);
  let followers = $state<FollowerEntry[]>([]);
  let mutedPubkeys = $state<string[]>([]);
  let blockedPubkeys = $state<string[]>([]);
  let newPubkey = $state("");
  let loading = $state(true);
  let status = $state("");
  let copyFeedback = $state("");
  let pendingUnfollowPubkey = $state<string | null>(null);
  let activeTab = $state<"following" | "followers">("following");
  let editingAlias = $state<string | null>(null);
  let aliasInput = $state("");

  async function copyWithFeedback(text: string, label: string) {
    await copyToClipboard(text);
    copyFeedback = label;
    setTimeout(() => (copyFeedback = ""), 1500);
  }

  async function init() {
    try {
      await invoke("get_node_id"); // wait for node ready
      await Promise.all([
        loadFollows(),
        loadFollowers(),
        loadMuted(),
        loadBlocked(),
      ]);
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

  async function loadMuted() {
    try {
      mutedPubkeys = await invoke("get_muted_pubkeys");
    } catch (e) {
      console.error("Failed to load muted:", e);
    }
  }

  async function loadBlocked() {
    try {
      blockedPubkeys = await invoke("get_blocked_pubkeys");
    } catch (e) {
      console.error("Failed to load blocked:", e);
    }
  }

  async function unmute(pubkey: string) {
    try {
      await invoke("unmute_user", { pubkey });
      await loadMuted();
    } catch (e) {
      status = `Error: ${e}`;
    }
  }

  async function unblock(pubkey: string) {
    try {
      await invoke("unblock_user", { pubkey });
      await loadBlocked();
    } catch (e) {
      status = `Error: ${e}`;
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

  function handleGlobalKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      if (pendingUnfollowPubkey) cancelUnfollow();
      else if (editingAlias) editingAlias = null;
    }
  }

  async function saveAlias() {
    if (!editingAlias) return;
    try {
      const alias = aliasInput.trim() || null;
      await invoke("update_follow_alias", { pubkey: editingAlias, alias });
      await loadFollows();
    } catch (e) {
      status = `Error: ${e}`;
    }
    editingAlias = null;
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

    window.addEventListener("keydown", handleGlobalKey);
    return () => {
      window.removeEventListener("keydown", handleGlobalKey);
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
              <Avatar
                pubkey={f.pubkey}
                {name}
                ticket={getCachedAvatarTicket(f.pubkey)}
              />
              <div class="follow-identity">
                {#if f.alias}
                  <span class="display-name">{f.alias}</span>
                {:else if name !== shortId(f.pubkey)}
                  <span class="display-name">{name}</span>
                {/if}
                <code>{shortId(f.pubkey)}</code>
              </div>
            {/await}
          </a>
          <div class="follow-actions">
            <button
              class="alias-btn"
              onclick={(e) => {
                e.preventDefault();
                editingAlias = f.pubkey;
                aliasInput = f.alias ?? "";
              }}
            >
              {f.alias ? "Edit alias" : "Set alias"}
            </button>
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

    {#if editingAlias}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="modal-overlay"
        onclick={() => (editingAlias = null)}
        role="presentation"
      >
        <!-- svelte-ignore a11y_interactive_supports_focus -->
        <div
          class="modal"
          onclick={(e) => e.stopPropagation()}
          role="dialog"
          aria-label="Set alias"
        >
          <p>Set a local alias for this user</p>
          <input
            class="alias-input"
            bind:value={aliasInput}
            placeholder="Alias (leave empty to clear)"
            onkeydown={(e) => {
              if (e.key === "Enter") saveAlias();
            }}
          />
          <div class="modal-actions">
            <button class="modal-cancel" onclick={() => (editingAlias = null)}
              >Cancel</button
            >
            <button class="modal-confirm save" onclick={saveAlias}>Save</button>
          </div>
        </div>
      </div>
    {/if}
  {:else}
    <div class="follow-list">
      {#each followers as f (f.pubkey)}
        <div class="follow-item">
          <a href="/user/{f.pubkey}" class="follow-info">
            {#await getDisplayName(f.pubkey, "") then name}
              <Avatar
                pubkey={f.pubkey}
                {name}
                ticket={getCachedAvatarTicket(f.pubkey)}
              />
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

  {#if mutedPubkeys.length > 0}
    <details class="moderation-section">
      <summary class="moderation-header muted">
        Muted ({mutedPubkeys.length})
      </summary>
      <div class="follow-list">
        {#each mutedPubkeys as pubkey (pubkey)}
          <div class="follow-item">
            <a href="/user/{pubkey}" class="follow-info">
              {#await getDisplayName(pubkey, "") then name}
                <Avatar
                  {pubkey}
                  {name}
                  ticket={getCachedAvatarTicket(pubkey)}
                />
                <div class="follow-identity">
                  {#if name !== shortId(pubkey)}
                    <span class="display-name">{name}</span>
                  {/if}
                  <code>{shortId(pubkey)}</code>
                </div>
              {/await}
            </a>
            <div class="follow-actions">
              <button class="unmute-btn" onclick={() => unmute(pubkey)}>
                Unmute
              </button>
            </div>
          </div>
        {/each}
      </div>
    </details>
  {/if}

  {#if blockedPubkeys.length > 0}
    <details class="moderation-section">
      <summary class="moderation-header blocked">
        Blocked ({blockedPubkeys.length})
      </summary>
      <div class="follow-list">
        {#each blockedPubkeys as pubkey (pubkey)}
          <div class="follow-item">
            <a href="/user/{pubkey}" class="follow-info">
              {#await getDisplayName(pubkey, "") then name}
                <Avatar
                  {pubkey}
                  {name}
                  ticket={getCachedAvatarTicket(pubkey)}
                />
                <div class="follow-identity">
                  {#if name !== shortId(pubkey)}
                    <span class="display-name">{name}</span>
                  {/if}
                  <code>{shortId(pubkey)}</code>
                </div>
              {/await}
            </a>
            <div class="follow-actions">
              <button class="unblock-btn" onclick={() => unblock(pubkey)}>
                Unblock
              </button>
            </div>
          </div>
        {/each}
      </div>
    </details>
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

  .moderation-section {
    margin-top: 1.5rem;
    border-top: 1px solid #2a2a4a;
    padding-top: 0.75rem;
  }

  .moderation-header {
    cursor: pointer;
    font-size: 0.8rem;
    font-weight: 600;
    padding: 0.4rem 0;
    list-style: none;
    user-select: none;
  }

  .moderation-header::-webkit-details-marker {
    display: none;
  }

  .moderation-header::before {
    content: "\25B6";
    display: inline-block;
    margin-right: 0.4rem;
    font-size: 0.65rem;
    transition: transform 0.15s;
  }

  details[open] > .moderation-header::before {
    transform: rotate(90deg);
  }

  .moderation-header.muted {
    color: #f59e0b;
  }

  .moderation-header.blocked {
    color: #f87171;
  }

  .unmute-btn {
    background: transparent;
    color: #f59e0b;
    border: 1px solid #f59e0b40;
    border-radius: 4px;
    padding: 0.3rem 0.75rem;
    font-size: 0.8rem;
    cursor: pointer;
  }

  .unmute-btn:hover {
    background: #f59e0b20;
  }

  .unblock-btn {
    background: transparent;
    color: #f87171;
    border: 1px solid #f8717140;
    border-radius: 4px;
    padding: 0.3rem 0.75rem;
    font-size: 0.8rem;
    cursor: pointer;
  }

  .unblock-btn:hover {
    background: #f8717120;
  }

  .alias-btn {
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

  .alias-btn:hover {
    background: #3a3a5a;
  }

  .alias-input {
    width: 100%;
    background: #0f0f23;
    border: 1px solid #2a2a4a;
    border-radius: 6px;
    padding: 0.6rem 0.75rem;
    color: #e0e0e0;
    font-family: inherit;
    font-size: 0.9rem;
    box-sizing: border-box;
    margin-bottom: 1rem;
  }

  .alias-input:focus {
    outline: none;
    border-color: #a78bfa;
  }
</style>
