<script lang="ts">
  import { page } from "$app/state";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Lightbox from "$lib/Lightbox.svelte";
  import PostCard from "$lib/PostCard.svelte";
  import ReplyComposer from "$lib/ReplyComposer.svelte";
  import { createBlobCache } from "$lib/blobs";
  import type { Post } from "$lib/types";

  let postId: string = $derived(page.params.id ?? "");
  let nodeId = $state("");
  let post = $state<Post | null>(null);
  let replies = $state<Post[]>([]);
  let loading = $state(true);
  let lightboxSrc = $state("");
  let lightboxAlt = $state("");
  let hasMore = $state(true);
  let loadingMore = $state(false);
  let sentinel = $state<HTMLDivElement>(null!);
  let replySection = $state<HTMLDivElement>(null!);

  const blobs = createBlobCache();

  async function init() {
    try {
      nodeId = await invoke("get_node_id");
      await loadPost();
      await loadReplies();
      loading = false;
    } catch {
      setTimeout(init, 500);
    }
  }

  async function loadPost() {
    post = await invoke("get_post", { id: postId });
  }

  async function loadReplies() {
    try {
      const result: Post[] = await invoke("get_replies", {
        targetPostId: postId,
        limit: 50,
        before: null,
      });
      replies = result;
      hasMore = result.length >= 50;
    } catch (e) {
      console.error("Failed to load replies:", e);
    }
  }

  async function loadMoreReplies() {
    if (loadingMore || !hasMore || replies.length === 0) return;
    loadingMore = true;
    try {
      const oldest = replies[replies.length - 1];
      const more: Post[] = await invoke("get_replies", {
        targetPostId: postId,
        limit: 50,
        before: oldest.timestamp,
      });
      if (more.length === 0) {
        hasMore = false;
      } else {
        replies = [...replies, ...more];
        hasMore = more.length >= 50;
      }
    } catch (e) {
      console.error("Failed to load more replies:", e);
    }
    loadingMore = false;
  }

  let scrollObserver: IntersectionObserver | null = null;

  $effect(() => {
    scrollObserver?.disconnect();
    if (!sentinel) return;
    scrollObserver = new IntersectionObserver(
      (entries) => {
        if (entries[0].isIntersecting && hasMore && !loadingMore) {
          loadMoreReplies();
        }
      },
      { rootMargin: "0px 0px 200px 0px" },
    );
    scrollObserver.observe(sentinel);
    return () => scrollObserver?.disconnect();
  });

  onMount(() => {
    init();
    const unlisteners: Promise<UnlistenFn>[] = [];
    unlisteners.push(
      listen("feed-updated", () => {
        loadReplies();
      }),
    );
    return () => {
      scrollObserver?.disconnect();
      blobs.revokeAll();
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
    <p>Loading thread...</p>
  </div>
{:else}
  <a href="/" class="back-link">&larr; Back to feed</a>

  {#if post}
    <div class="parent-post">
      <PostCard
        {post}
        {nodeId}
        showReplyContext={true}
        getBlobUrl={blobs.getBlobUrl}
        downloadFile={blobs.downloadFile}
        onreply={() => {
          replySection?.scrollIntoView({ behavior: "smooth" });
        }}
        onlightbox={(src, alt) => {
          lightboxSrc = src;
          lightboxAlt = alt;
        }}
      />
    </div>

    <div class="reply-section" bind:this={replySection}>
      <h3 class="section-title">
        Replies{replies.length > 0 ? ` (${replies.length})` : ""}
      </h3>

      <ReplyComposer
        replyToId={post.id}
        replyToAuthor={post.author}
        onsubmitted={loadReplies}
      />
    </div>
  {:else}
    <div class="not-found">
      <p>Post not found in local cache.</p>
      <p class="hint">
        The post may not have been synced yet. Try viewing it from the author's
        profile.
      </p>
    </div>
  {/if}

  <div class="replies">
    {#each replies as reply (reply.id)}
      <PostCard
        post={reply}
        {nodeId}
        showReplyContext={false}
        getBlobUrl={blobs.getBlobUrl}
        downloadFile={blobs.downloadFile}
        onlightbox={(src, alt) => {
          lightboxSrc = src;
          lightboxAlt = alt;
        }}
      />
    {:else}
      {#if post}
        <p class="empty">No replies yet.</p>
      {/if}
    {/each}
  </div>

  {#if hasMore && replies.length > 0}
    <div bind:this={sentinel} class="sentinel">
      {#if loadingMore}
        <span class="btn-spinner"></span> Loading...
      {/if}
    </div>
  {/if}
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

  .parent-post :global(.post) {
    border-color: #3a3a5a;
    margin-bottom: 1rem;
  }

  .reply-section {
    margin-bottom: 1rem;
  }

  .section-title {
    color: #888;
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin: 0 0 0.5rem;
  }

  .empty {
    text-align: center;
    color: #666;
    padding: 1rem;
    font-size: 0.85rem;
  }

  .not-found {
    text-align: center;
    padding: 2rem;
    color: #888;
  }

  .not-found .hint {
    font-size: 0.8rem;
    color: #666;
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
</style>
