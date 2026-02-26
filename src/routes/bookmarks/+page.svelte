<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import PostCard from "$lib/PostCard.svelte";
  import Lightbox from "$lib/Lightbox.svelte";
  import { createBlobCache } from "$lib/blobs";
  import type { Post } from "$lib/types";
  import { setupInfiniteScroll } from "$lib/utils";

  let nodeId = $state("");
  let posts = $state<Post[]>([]);
  let loading = $state(true);
  let hasMore = $state(true);
  let loadingMore = $state(false);
  let sentinel = $state<HTMLDivElement>(null!);
  let lightboxSrc = $state("");
  let lightboxAlt = $state("");

  const blobs = createBlobCache();

  async function init() {
    try {
      nodeId = await invoke("get_node_id");
      await loadBookmarks();
      loading = false;
    } catch {
      setTimeout(init, 500);
    }
  }

  async function loadBookmarks() {
    try {
      const result: Post[] = await invoke("get_bookmarks", {
        limit: 20,
        before: null,
      });
      posts = result;
      hasMore = result.length >= 20;
    } catch (e) {
      console.error("Failed to load bookmarks:", e);
    }
  }

  async function loadMore() {
    if (loadingMore || !hasMore || posts.length === 0) return;
    loadingMore = true;
    try {
      const oldest = posts[posts.length - 1];
      const more: Post[] = await invoke("get_bookmarks", {
        limit: 20,
        before: oldest.timestamp,
      });
      if (more.length === 0) {
        hasMore = false;
      } else {
        posts = [...posts, ...more];
        hasMore = more.length >= 20;
      }
    } catch (e) {
      console.error("Failed to load more bookmarks:", e);
    }
    loadingMore = false;
  }

  $effect(() => {
    return setupInfiniteScroll(sentinel, hasMore, loadingMore, loadMore);
  });

  onMount(() => {
    init();
    return () => {
      blobs.revokeAll();
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

<h2 class="page-title">Bookmarks</h2>

{#if loading}
  <div class="loading">
    <div class="spinner"></div>
    <p>Loading bookmarks...</p>
  </div>
{:else if posts.length === 0}
  <div class="empty">
    <p>No bookmarks yet.</p>
    <p class="hint">Bookmark posts from your feed to save them here.</p>
  </div>
{:else}
  <div class="bookmarks">
    {#each posts as post (post.id)}
      <PostCard
        {post}
        {nodeId}
        getBlobUrl={blobs.getBlobUrl}
        downloadFile={blobs.downloadFile}
        onlightbox={(src, alt) => {
          lightboxSrc = src;
          lightboxAlt = alt;
        }}
      />
    {/each}
  </div>

  {#if hasMore && posts.length > 0}
    <div bind:this={sentinel} class="sentinel">
      {#if loadingMore}
        <span class="btn-spinner"></span> Loading...
      {/if}
    </div>
  {/if}
{/if}

<style>
  .page-title {
    font-size: 1.1rem;
    color: #e0e0e0;
    margin: 0 0 1rem;
  }

  .empty {
    text-align: center;
    padding: 3rem 1rem;
    color: #888;
  }

  .empty .hint {
    font-size: 0.8rem;
    color: #666;
  }
</style>
