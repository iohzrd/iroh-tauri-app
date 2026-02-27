<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import PostCard from "$lib/PostCard.svelte";
  import Lightbox from "$lib/Lightbox.svelte";
  import { createBlobCache, setBlobContext } from "$lib/blobs";
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
  setBlobContext(blobs);

  async function init() {
    try {
      nodeId = await invoke("get_node_id");
      await loadMentions();
      await invoke("mark_mentions_read");
      loading = false;
    } catch {
      setTimeout(init, 500);
    }
  }

  async function loadMentions() {
    try {
      const result: Post[] = await invoke("get_mentions", {
        limit: 20,
        before: null,
      });
      posts = result;
      hasMore = result.length >= 20;
    } catch (e) {
      console.error("Failed to load mentions:", e);
    }
  }

  async function loadMore() {
    if (loadingMore || !hasMore || posts.length === 0) return;
    loadingMore = true;
    try {
      const oldest = posts[posts.length - 1];
      const more: Post[] = await invoke("get_mentions", {
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
      console.error("Failed to load more mentions:", e);
    }
    loadingMore = false;
  }

  $effect(() => {
    return setupInfiniteScroll(
      sentinel,
      () => hasMore,
      () => loadingMore,
      loadMore,
    );
  });

  onMount(() => {
    init();
    const unlisteners: Promise<UnlistenFn>[] = [];
    unlisteners.push(
      listen("mentioned-in-post", () => {
        loadMentions();
        invoke("mark_mentions_read");
      }),
    );
    return () => {
      unlisteners.forEach((p) => p.then((fn) => fn()));
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

<h2 class="page-title">Activity</h2>

{#if loading}
  <div class="loading">
    <div class="spinner"></div>
    <p>Loading activity...</p>
  </div>
{:else if posts.length === 0}
  <div class="empty">
    <p>No activity yet.</p>
    <p class="hint">
      When someone mentions you in a post, it will appear here.
    </p>
  </div>
{:else}
  <div class="activity">
    {#each posts as post (post.id)}
      <PostCard
        {post}
        {nodeId}
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
