<script lang="ts">
  import { page } from "$app/state";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Timeago from "$lib/Timeago.svelte";
  import Lightbox from "$lib/Lightbox.svelte";
  import Avatar from "$lib/Avatar.svelte";
  import PostActions from "$lib/PostActions.svelte";
  import ReplyComposer from "$lib/ReplyComposer.svelte";
  import type { MediaAttachment, Post } from "$lib/types";
  import {
    shortId,
    getDisplayName,
    getCachedAvatarTicket,
    linkify,
    isImage,
    isVideo,
    formatSize,
  } from "$lib/utils";

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

  const blobUrlCache = new Map<string, string>();

  function revokeAllBlobUrls() {
    for (const url of blobUrlCache.values()) URL.revokeObjectURL(url);
    blobUrlCache.clear();
  }

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
      revokeAllBlobUrls();
      unlisteners.forEach((p) => p.then((fn) => fn()));
    };
  });
</script>

{#snippet renderMedia(media: MediaAttachment[])}
  <div class="post-media" class:grid={media.length > 1}>
    {#each media as att (att.hash)}
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
{/snippet}

{#snippet renderPost(p: Post, isParent: boolean)}
  <article class="post" class:parent={isParent}>
    <div class="post-header">
      {#await getDisplayName(p.author, nodeId)}
        {@const fallback = p.author === nodeId ? "You" : shortId(p.author)}
        <a href="/user/{p.author}" class="author-link">
          <Avatar
            pubkey={p.author}
            name={fallback}
            isSelf={p.author === nodeId}
            ticket={getCachedAvatarTicket(p.author)}
          />
          <span class="author" class:self={p.author === nodeId}>
            {fallback}
          </span>
        </a>
      {:then name}
        <a href="/user/{p.author}" class="author-link">
          <Avatar
            pubkey={p.author}
            {name}
            isSelf={p.author === nodeId}
            ticket={getCachedAvatarTicket(p.author)}
          />
          <span class="author" class:self={p.author === nodeId}>
            {name}
          </span>
        </a>
      {/await}
      <span class="time"><Timeago timestamp={p.timestamp} /></span>
    </div>
    {#if p.content}
      <p class="post-content">{@html linkify(p.content)}</p>
    {/if}
    {#if p.media && p.media.length > 0}
      {@render renderMedia(p.media)}
    {/if}
    <PostActions
      postId={p.id}
      postAuthor={p.author}
      onreply={() => {
        if (isParent) {
          replySection?.scrollIntoView({ behavior: "smooth" });
        } else {
          goto(`/post/${p.id}`);
        }
      }}
    />
  </article>
{/snippet}

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
    {@render renderPost(post, true)}

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
      {@render renderPost(reply, false)}
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

  .post.parent {
    border-color: #3a3a5a;
    margin-bottom: 1rem;
  }

  .post-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.5rem;
  }

  .author-link {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    text-decoration: none;
    color: inherit;
  }

  .author-link:hover .author {
    text-decoration: underline;
  }

  .author {
    font-weight: 600;
    font-size: 0.85rem;
    color: #c4b5fd;
  }

  .author.self {
    color: #a78bfa;
  }

  .time {
    color: #666;
    font-size: 0.8rem;
    margin-left: auto;
    white-space: nowrap;
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
