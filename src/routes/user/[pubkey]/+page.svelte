<script lang="ts">
  import { page } from "$app/state";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Lightbox from "$lib/Lightbox.svelte";
  import Avatar from "$lib/Avatar.svelte";
  import PostCard from "$lib/PostCard.svelte";
  import { createBlobCache } from "$lib/blobs";
  import type {
    Post,
    Profile,
    FollowEntry,
    SyncResult,
    SyncStatus,
  } from "$lib/types";
  import { shortId, copyToClipboard, setupInfiniteScroll } from "$lib/utils";

  let pubkey: string = $derived(page.params.pubkey ?? "");
  let nodeId = $state("");
  let profile = $state<Profile | null>(null);
  let posts = $state<Post[]>([]);
  let loading = $state(true);
  let isFollowing = $state(false);
  let toggling = $state(false);
  let copyFeedback = $state(false);
  let hasMore = $state(true);
  let loadingMore = $state(false);
  let replyingTo = $state<Post | null>(null);
  let quotingPost = $state<Post | null>(null);
  let lightboxSrc = $state("");
  let lightboxAlt = $state("");
  let toastMessage = $state("");
  let toastType = $state<"error" | "success">("error");
  let sentinel = $state<HTMLDivElement>(null!);
  let mediaFilter = $state("all");
  let syncStatus = $state<SyncStatus | null>(null);
  let remoteTotal = $state<number | null>(null);
  let fetchingRemote = $state(false);
  let peerOffline = $state(false);
  let pendingDeleteId = $state<string | null>(null);
  let isMuted = $state(false);
  let isBlocked = $state(false);
  let togglingMute = $state(false);
  let togglingBlock = $state(false);

  const FILTERS = [
    { value: "all", label: "All" },
    { value: "images", label: "Images" },
    { value: "videos", label: "Videos" },
    { value: "audio", label: "Audio" },
    { value: "files", label: "Files" },
    { value: "text", label: "Text" },
  ] as const;

  const blobs = createBlobCache();

  function showToast(message: string, type: "error" | "success" = "error") {
    toastMessage = message;
    toastType = type;
    setTimeout(() => (toastMessage = ""), 4000);
  }

  let isSelf = $derived(pubkey === nodeId);
  let displayName = $derived(
    profile?.display_name || (isSelf ? "You" : shortId(pubkey)),
  );

  async function init() {
    try {
      nodeId = await invoke("get_node_id");
      if (isSelf) {
        const myProfile: Profile | null = await invoke("get_my_profile");
        profile = myProfile;
      } else {
        profile = await invoke("get_remote_profile", { pubkey });
      }

      const allPosts: Post[] = await invoke("get_user_posts", {
        pubkey,
        limit: 20,
        before: null,
        mediaFilter: mediaFilter === "all" ? null : mediaFilter,
      });
      posts = allPosts;
      hasMore = allPosts.length >= 20;

      const follows: FollowEntry[] = await invoke("get_follows");
      isFollowing = follows.some((f) => f.pubkey === pubkey);

      if (!isSelf) {
        isMuted = await invoke("is_muted", { pubkey });
        isBlocked = await invoke("is_blocked", { pubkey });
      }

      if (!isSelf) {
        try {
          syncStatus = await invoke("get_sync_status", { pubkey });
        } catch {
          // sync status is informational
        }
      }

      loading = false;
    } catch {
      setTimeout(init, 500);
    }
  }

  async function reloadPosts() {
    try {
      const newPosts: Post[] = await invoke("get_user_posts", {
        pubkey,
        limit: 20,
        before: null,
        mediaFilter: mediaFilter === "all" ? null : mediaFilter,
      });
      posts = newPosts;
      hasMore = newPosts.length >= 20;
    } catch (e) {
      console.error("Failed to reload posts:", e);
    }
  }

  async function reloadProfile() {
    try {
      if (isSelf) {
        profile = await invoke("get_my_profile");
      } else {
        profile = await invoke("get_remote_profile", { pubkey });
      }
    } catch (e) {
      console.error("Failed to reload profile:", e);
    }
  }

  async function loadMore() {
    if (loadingMore || !hasMore || posts.length === 0) return;
    loadingMore = true;
    try {
      const oldest = posts[posts.length - 1];
      const olderPosts: Post[] = await invoke("get_user_posts", {
        pubkey,
        limit: 20,
        before: oldest.timestamp,
        mediaFilter: mediaFilter === "all" ? null : mediaFilter,
      });
      if (olderPosts.length > 0) {
        posts = [...posts, ...olderPosts];
        hasMore = olderPosts.length >= 20;
      } else if (!isSelf && !peerOffline && mediaFilter === "all") {
        await fetchFromRemote();
      } else {
        hasMore = false;
      }
    } catch (e) {
      showToast("Failed to load more posts");
      console.error("Failed to load more:", e);
    }
    loadingMore = false;
  }

  async function fetchFromRemote() {
    fetchingRemote = true;
    try {
      const result: SyncResult = await invoke("fetch_older_posts", {
        pubkey,
      });
      remoteTotal = result.remote_total;
      if (result.posts.length > 0) {
        posts = [...posts, ...result.posts];
        syncStatus = await invoke("get_sync_status", { pubkey });
      }
      hasMore = false;
    } catch {
      peerOffline = true;
      hasMore = false;
    }
    fetchingRemote = false;
  }

  async function toggleFollow() {
    toggling = true;
    try {
      if (isFollowing) {
        await invoke("unfollow_user", { pubkey });
        isFollowing = false;
      } else {
        await invoke("follow_user", { pubkey });
        isFollowing = true;
      }
    } catch (e) {
      showToast(`Failed to ${isFollowing ? "unfollow" : "follow"}`);
      console.error("Toggle follow failed:", e);
    }
    toggling = false;
  }

  async function toggleMute() {
    togglingMute = true;
    try {
      if (isMuted) {
        await invoke("unmute_user", { pubkey });
        isMuted = false;
      } else {
        await invoke("mute_user", { pubkey });
        isMuted = true;
      }
    } catch (e) {
      showToast("Failed to toggle mute");
      console.error("Toggle mute failed:", e);
    }
    togglingMute = false;
  }

  async function toggleBlock() {
    togglingBlock = true;
    try {
      if (isBlocked) {
        await invoke("unblock_user", { pubkey });
        isBlocked = false;
      } else {
        await invoke("block_user", { pubkey });
        isBlocked = true;
        isFollowing = false;
      }
    } catch (e) {
      showToast("Failed to toggle block");
      console.error("Toggle block failed:", e);
    }
    togglingBlock = false;
  }

  async function copyNodeId() {
    await copyToClipboard(pubkey);
    copyFeedback = true;
    setTimeout(() => (copyFeedback = false), 1500);
  }

  function confirmDelete(id: string) {
    pendingDeleteId = id;
  }

  async function executeDelete() {
    if (!pendingDeleteId) return;
    try {
      await invoke("delete_post", { id: pendingDeleteId });
      await reloadPosts();
    } catch (e) {
      showToast("Failed to delete post");
      console.error("Failed to delete post:", e);
    }
    pendingDeleteId = null;
  }

  function cancelDelete() {
    pendingDeleteId = null;
  }

  function handleGlobalKey(e: KeyboardEvent) {
    if (e.key === "Escape" && pendingDeleteId) {
      cancelDelete();
    }
  }

  $effect(() => {
    return setupInfiniteScroll(sentinel, hasMore, loadingMore, loadMore);
  });

  let filterInitialized = false;
  $effect(() => {
    mediaFilter; // track dependency
    if (!filterInitialized) {
      filterInitialized = true;
      return;
    }
    posts = [];
    hasMore = true;
    reloadPosts();
  });

  onMount(() => {
    init();
    const unlisteners: Promise<UnlistenFn>[] = [];
    unlisteners.push(
      listen("feed-updated", () => {
        reloadPosts();
      }),
    );
    unlisteners.push(
      listen("profile-updated", (event) => {
        if (event.payload === pubkey) {
          reloadProfile();
        }
      }),
    );
    window.addEventListener("keydown", handleGlobalKey);
    return () => {
      blobs.revokeAll();
      unlisteners.forEach((p) => p.then((fn) => fn()));
      window.removeEventListener("keydown", handleGlobalKey);
    };
  });
</script>

{#if lightboxSrc}
  <Lightbox
    src={lightboxSrc}
    alt={lightboxAlt}
    onclose={() => {
      lightboxSrc = "";
    }}
  />
{/if}

{#if loading}
  <div class="loading">
    <div class="spinner"></div>
    <p>Loading profile...</p>
  </div>
{:else}
  <a href="/" class="back-link">&larr; Back to feed</a>

  <div class="profile-header">
    <Avatar
      {pubkey}
      name={displayName}
      {isSelf}
      ticket={profile?.avatar_ticket}
      size={56}
    />
    <div class="profile-info">
      <h2>{displayName}</h2>
      {#if profile?.bio}
        <p class="bio">{profile.bio}</p>
      {/if}
    </div>
  </div>

  <div class="id-row">
    <code>{pubkey}</code>
    <button class="copy-btn" onclick={copyNodeId}>
      {copyFeedback ? "Copied!" : "Copy ID"}
    </button>
  </div>

  {#if !isSelf}
    <div class="action-row">
      <button
        class="follow-toggle"
        class:following={isFollowing}
        onclick={toggleFollow}
        disabled={toggling || isBlocked}
      >
        {toggling ? "..." : isFollowing ? "Unfollow" : "Follow"}
      </button>
      <a href="/messages/{pubkey}" class="message-btn">Message</a>
    </div>
    <div class="moderation-row">
      <button
        class="mod-btn mute"
        class:active={isMuted}
        onclick={toggleMute}
        disabled={togglingMute}
      >
        {togglingMute ? "..." : isMuted ? "Unmute" : "Mute"}
      </button>
      <button
        class="mod-btn block"
        class:active={isBlocked}
        onclick={toggleBlock}
        disabled={togglingBlock}
      >
        {togglingBlock ? "..." : isBlocked ? "Unblock" : "Block"}
      </button>
    </div>
  {/if}

  <div class="filter-bar">
    {#each FILTERS as f (f.value)}
      <button
        class="filter-chip"
        class:active={mediaFilter === f.value}
        onclick={() => (mediaFilter = f.value)}
      >
        {f.label}
      </button>
    {/each}
  </div>

  <h3 class="section-title">
    Posts{posts.length > 0 ? ` (${posts.length}${hasMore ? "+" : ""})` : ""}
    {#if syncStatus && !isSelf}
      <span class="sync-info">
        {syncStatus.local_count}{remoteTotal != null ? ` / ${remoteTotal}` : ""} synced
      </span>
    {/if}
  </h3>

  {#if pendingDeleteId}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="modal-overlay" onclick={cancelDelete} role="presentation">
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div
        class="modal"
        onclick={(e) => e.stopPropagation()}
        role="dialog"
        aria-label="Confirm delete"
      >
        <p>Delete this post? This cannot be undone.</p>
        <div class="modal-actions">
          <button class="modal-cancel" onclick={cancelDelete}>Cancel</button>
          <button class="modal-confirm" onclick={executeDelete}>Delete</button>
        </div>
      </div>
    </div>
  {/if}

  <div class="feed">
    {#each posts as post (post.id)}
      <PostCard
        {post}
        {nodeId}
        showAuthor={false}
        showDelete={isSelf}
        {replyingTo}
        {quotingPost}
        getBlobUrl={blobs.getBlobUrl}
        downloadFile={blobs.downloadFile}
        onreply={(p) => {
          replyingTo = replyingTo?.id === p.id ? null : p;
          quotingPost = null;
        }}
        ondelete={confirmDelete}
        onreplied={() => {
          replyingTo = null;
          reloadPosts();
        }}
        onquote={(p) => {
          quotingPost = quotingPost?.id === p.id ? null : p;
          replyingTo = null;
        }}
        onquoted={() => {
          quotingPost = null;
          reloadPosts();
        }}
        onlightbox={(src, alt) => {
          lightboxSrc = src;
          lightboxAlt = alt;
        }}
      />
    {:else}
      <p class="empty">No posts from this user yet.</p>
    {/each}
  </div>

  {#if hasMore && posts.length > 0}
    <div bind:this={sentinel} class="sentinel">
      {#if loadingMore}
        <span class="btn-spinner"></span>
        {#if fetchingRemote}
          Fetching from peer...
        {:else}
          Loading...
        {/if}
      {/if}
    </div>
  {/if}

  {#if peerOffline && !hasMore && posts.length > 0}
    <p class="offline-notice">End of cached posts -- peer is offline</p>
  {/if}
{/if}

{#if toastMessage}
  <div class="toast" class:error={toastType === "error"}>
    {toastMessage}
  </div>
{/if}

<style>
  .back-link {
    display: inline-block;
    color: #a78bfa;
    text-decoration: none;
    font-size: 0.85rem;
    margin-bottom: 1rem;
  }

  .back-link:hover {
    text-decoration: underline;
  }

  .profile-header {
    display: flex;
    align-items: center;
    gap: 1rem;
    margin-bottom: 1rem;
  }

  .profile-info {
    flex: 1;
    min-width: 0;
  }

  .profile-info h2 {
    margin: 0;
    color: #a78bfa;
    font-size: 1.1rem;
  }

  .bio {
    margin: 0.25rem 0 0;
    color: #888;
    font-size: 0.85rem;
  }

  .id-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }

  code {
    background: #0f0f23;
    padding: 0.5rem 0.75rem;
    border-radius: 6px;
    font-size: 0.7rem;
    word-break: break-all;
    color: #7dd3fc;
    flex: 1;
  }

  .copy-btn {
    background: #2a2a4a;
    color: #c4b5fd;
    border: none;
    border-radius: 4px;
    padding: 0.4rem 0.75rem;
    font-size: 0.75rem;
    cursor: pointer;
    white-space: nowrap;
    min-width: 52px;
    text-align: center;
  }

  .copy-btn:hover {
    background: #3a3a5a;
  }

  .action-row {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }

  .follow-toggle {
    flex: 1;
    background: #7c3aed;
    color: white;
    border: none;
    border-radius: 6px;
    padding: 0.5rem;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
  }

  .message-btn {
    background: #2a2a4a;
    color: #c4b5fd;
    border: none;
    border-radius: 6px;
    padding: 0.5rem 1rem;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
    text-decoration: none;
    text-align: center;
    transition: background 0.2s;
  }

  .message-btn:hover {
    background: #3a3a5a;
  }

  .follow-toggle:hover:not(:disabled) {
    background: #6d28d9;
  }

  .follow-toggle.following {
    background: transparent;
    color: #f87171;
    border: 1px solid #f8717140;
  }

  .follow-toggle.following:hover:not(:disabled) {
    background: #f8717120;
  }

  .follow-toggle:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .moderation-row {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }

  .mod-btn {
    flex: 1;
    background: transparent;
    border: 1px solid #2a2a4a;
    border-radius: 6px;
    padding: 0.35rem;
    font-size: 0.8rem;
    font-weight: 500;
    cursor: pointer;
    font-family: inherit;
    transition:
      color 0.15s,
      background 0.15s,
      border-color 0.15s;
  }

  .mod-btn.mute {
    color: #888;
  }

  .mod-btn.mute:hover:not(:disabled) {
    color: #f59e0b;
    border-color: #f59e0b40;
    background: #f59e0b10;
  }

  .mod-btn.mute.active {
    color: #f59e0b;
    border-color: #f59e0b40;
  }

  .mod-btn.mute.active:hover:not(:disabled) {
    background: #f59e0b10;
  }

  .mod-btn.block {
    color: #888;
  }

  .mod-btn.block:hover:not(:disabled) {
    color: #ef4444;
    border-color: #ef444440;
    background: #ef444410;
  }

  .mod-btn.block.active {
    color: #ef4444;
    border-color: #ef444440;
  }

  .mod-btn.block.active:hover:not(:disabled) {
    background: #ef444410;
  }

  .mod-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .filter-bar {
    display: flex;
    gap: 0.4rem;
    margin-bottom: 0.75rem;
    flex-wrap: wrap;
  }

  .filter-chip {
    background: #2a2a4a;
    color: #888;
    border: 1px solid transparent;
    border-radius: 999px;
    padding: 0.3rem 0.75rem;
    font-size: 0.75rem;
    cursor: pointer;
    font-family: inherit;
    transition:
      background 0.2s,
      color 0.2s,
      border-color 0.2s;
  }

  .filter-chip:hover {
    color: #c4b5fd;
    border-color: #3a3a5a;
  }

  .filter-chip.active {
    background: #7c3aed;
    color: white;
    border-color: #7c3aed;
  }

  .section-title {
    color: #888;
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin: 0 0 0.75rem;
  }

  .empty {
    text-align: center;
    color: #666;
    padding: 2rem;
  }

  .sync-info {
    font-size: 0.7rem;
    color: #666;
    font-weight: 400;
    text-transform: none;
    letter-spacing: normal;
    margin-left: 0.5rem;
  }

  .offline-notice {
    text-align: center;
    color: #666;
    font-size: 0.8rem;
    padding: 0.75rem;
    border-top: 1px solid #2a2a4a;
    margin-top: 0.5rem;
  }
</style>
