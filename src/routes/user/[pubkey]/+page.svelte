<script lang="ts">
  import { page } from "$app/state";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Timeago from "$lib/Timeago.svelte";
  import Lightbox from "$lib/Lightbox.svelte";
  import Avatar from "$lib/Avatar.svelte";
  import type {
    MediaAttachment,
    Post,
    Profile,
    FollowEntry,
    SyncResult,
    SyncStatus,
  } from "$lib/types";
  import {
    shortId,
    copyToClipboard,
    linkify,
    isImage,
    isVideo,
    formatSize,
  } from "$lib/utils";

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

  const FILTERS = [
    { value: "all", label: "All" },
    { value: "images", label: "Images" },
    { value: "videos", label: "Videos" },
    { value: "audio", label: "Audio" },
    { value: "files", label: "Files" },
    { value: "text", label: "Text" },
  ] as const;

  const blobUrlCache = new Map<string, string>();

  function revokeAllBlobUrls() {
    for (const url of blobUrlCache.values()) URL.revokeObjectURL(url);
    blobUrlCache.clear();
  }

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
        limit: 50,
        before: null,
        mediaFilter: mediaFilter === "all" ? null : mediaFilter,
      });
      posts = allPosts;
      hasMore = allPosts.length >= 50;

      const follows: FollowEntry[] = await invoke("get_follows");
      isFollowing = follows.some((f) => f.pubkey === pubkey);

      if (!isSelf) {
        try {
          syncStatus = await invoke("get_sync_status", { pubkey });
        } catch {
          // sync status is informational, don't block on failure
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
        limit: 50,
        before: null,
        mediaFilter: mediaFilter === "all" ? null : mediaFilter,
      });
      posts = newPosts;
      hasMore = newPosts.length >= 50;
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
        limit: 50,
        before: oldest.timestamp,
        mediaFilter: mediaFilter === "all" ? null : mediaFilter,
      });
      if (olderPosts.length > 0) {
        posts = [...posts, ...olderPosts];
        hasMore = olderPosts.length >= 50;
      } else if (!isSelf && !peerOffline && mediaFilter === "all") {
        // Local posts exhausted -- try fetching from remote peer
        await fetchFromRemote(oldest.timestamp);
      } else {
        hasMore = false;
      }
    } catch (e) {
      showToast("Failed to load more posts");
      console.error("Failed to load more:", e);
    }
    loadingMore = false;
  }

  async function fetchFromRemote(before: number) {
    fetchingRemote = true;
    try {
      const result: SyncResult = await invoke("fetch_older_posts", {
        pubkey,
        before,
        limit: 50,
      });
      remoteTotal = result.remote_total;
      if (result.posts.length === 0) {
        hasMore = false;
      } else {
        posts = [...posts, ...result.posts];
        hasMore = result.posts.length >= 50;
        // Refresh sync status
        syncStatus = await invoke("get_sync_status", { pubkey });
      }
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

  async function copyNodeId() {
    await copyToClipboard(pubkey);
    copyFeedback = true;
    setTimeout(() => (copyFeedback = false), 1500);
  }

  async function getBlobUrl(attachment: MediaAttachment): Promise<string> {
    const cached = blobUrlCache.get(attachment.hash);
    if (cached) return cached;
    const bytes: number[] = await invoke("fetch_blob_bytes", {
      ticket: attachment.ticket,
    });
    const blob = new Blob([new Uint8Array(bytes)], {
      type: attachment.mime_type,
    });
    const url = URL.createObjectURL(blob);
    blobUrlCache.set(attachment.hash, url);
    return url;
  }

  async function downloadFile(att: MediaAttachment) {
    try {
      const url = await getBlobUrl(att);
      const a = document.createElement("a");
      a.href = url;
      a.download = att.filename;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
    } catch (e) {
      showToast(`Failed to download ${att.filename}`);
      console.error("Download failed:", e);
    }
  }

  let scrollObserver: IntersectionObserver | null = null;

  $effect(() => {
    scrollObserver?.disconnect();
    if (!sentinel) return;
    scrollObserver = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting && hasMore && !loadingMore) {
          loadMore();
        }
      },
      { rootMargin: "0px 0px 200px 0px" },
    );
    scrollObserver.observe(sentinel);
    return () => scrollObserver?.disconnect();
  });

  let prevFilter = "all";
  $effect(() => {
    const current = mediaFilter;
    if (current !== prevFilter) {
      prevFilter = current;
      posts = [];
      hasMore = true;
      reloadPosts();
    }
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
    return () => {
      scrollObserver?.disconnect();
      revokeAllBlobUrls();
      unlisteners.forEach((p) => p.then((fn) => fn()));
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
        disabled={toggling}
      >
        {toggling ? "..." : isFollowing ? "Unfollow" : "Follow"}
      </button>
      <a href="/messages/{pubkey}" class="message-btn">Message</a>
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

  <div class="feed">
    {#each posts as post (post.id)}
      <article class="post">
        <div class="post-header">
          <span class="time"><Timeago timestamp={post.timestamp} /></span>
        </div>
        {#if post.content}
          <p class="post-content">{@html linkify(post.content)}</p>
        {/if}
        {#if post.media && post.media.length > 0}
          <div class="post-media" class:grid={post.media.length > 1}>
            {#each post.media as att (att.hash)}
              {#if isImage(att.mime_type)}
                {#await getBlobUrl(att)}
                  <div class="media-placeholder">Loading...</div>
                {:then url}
                  <button
                    class="media-img-btn"
                    onclick={() => {
                      lightboxSrc = url;
                      lightboxAlt = att.filename;
                    }}
                  >
                    <img src={url} alt={att.filename} class="media-img" />
                  </button>
                {:catch}
                  <div class="media-placeholder">Failed to load</div>
                {/await}
              {:else if isVideo(att.mime_type)}
                {#await getBlobUrl(att)}
                  <div class="media-placeholder">Loading...</div>
                {:then url}
                  <video src={url} controls class="media-video">
                    <track kind="captions" />
                  </video>
                {:catch}
                  <div class="media-placeholder">Failed to load</div>
                {/await}
              {:else}
                <button class="media-file" onclick={() => downloadFile(att)}>
                  <span>{att.filename}</span>
                  <span class="file-size">{formatSize(att.size)}</span>
                  <span class="download-label">Download</span>
                </button>
              {/if}
            {/each}
          </div>
        {/if}
      </article>
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

  .post {
    background: #16213e;
    border: 1px solid #2a2a4a;
    border-radius: 8px;
    padding: 1rem;
    margin-bottom: 0.5rem;
  }

  .post:hover {
    border-color: #3a3a5a;
  }

  .post-header {
    margin-bottom: 0.5rem;
  }

  .time {
    color: #666;
    font-size: 0.8rem;
  }

  .post-content {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .post-content :global(a) {
    color: #7dd3fc;
    text-decoration: none;
  }

  .post-content :global(a:hover) {
    text-decoration: underline;
  }

  .post-media {
    margin-top: 0.75rem;
  }

  .post-media.grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 0.5rem;
  }

  .media-img-btn {
    background: none;
    border: none;
    padding: 0;
    cursor: zoom-in;
    display: block;
    width: 100%;
  }

  .media-img {
    width: 100%;
    border-radius: 6px;
    max-height: 400px;
    object-fit: contain;
    background: #0f0f23;
    display: block;
  }

  .media-video {
    width: 100%;
    border-radius: 6px;
    max-height: 400px;
  }

  .media-placeholder {
    background: #0f0f23;
    border-radius: 6px;
    padding: 2rem;
    text-align: center;
    color: #666;
    font-size: 0.8rem;
  }

  .media-file {
    background: #0f0f23;
    border: 1px solid #2a2a4a;
    border-radius: 6px;
    padding: 0.75rem;
    display: flex;
    justify-content: space-between;
    align-items: center;
    color: #c4b5fd;
    font-size: 0.85rem;
    cursor: pointer;
    width: 100%;
    font-family: inherit;
    transition: border-color 0.2s;
  }

  .media-file:hover {
    border-color: #a78bfa;
  }

  .file-size {
    color: #666;
    font-size: 0.75rem;
  }

  .download-label {
    color: #7dd3fc;
    font-size: 0.75rem;
  }

  .empty {
    text-align: center;
    color: #666;
    padding: 2rem;
  }

  .sentinel {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.4rem;
    width: 100%;
    min-height: 1px;
    padding: 0.5rem 0;
    color: #c4b5fd;
    font-size: 0.85rem;
  }

  .toast {
    position: fixed;
    bottom: 1.5rem;
    left: 50%;
    transform: translateX(-50%);
    background: #2a2a4a;
    color: #e0e0e0;
    padding: 0.6rem 1.25rem;
    border-radius: 8px;
    font-size: 0.85rem;
    z-index: 200;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
  }

  .toast.error {
    border-left: 3px solid #ef4444;
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
