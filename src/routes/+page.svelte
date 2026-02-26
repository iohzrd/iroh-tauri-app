<script lang="ts">
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Lightbox from "$lib/Lightbox.svelte";
  import PostCard from "$lib/PostCard.svelte";
  import { createBlobCache } from "$lib/blobs";
  import type { Post, PendingAttachment } from "$lib/types";
  import {
    shortId,
    seedOwnProfile,
    evictDisplayName,
    copyToClipboard,
    isImage,
    isVideo,
    setupInfiniteScroll,
  } from "$lib/utils";

  const MAX_POST_LENGTH = 10_000;

  let nodeId = $state("");
  let loading = $state(true);
  let syncing = $state(false);
  let posts = $state<Post[]>([]);
  let newPost = $state("");
  let posting = $state(false);
  let attachments = $state<PendingAttachment[]>([]);
  let uploading = $state(false);
  let fileInput = $state<HTMLInputElement>(null!);
  let copyFeedback = $state("");
  let pendingDeleteId = $state<string | null>(null);
  let showScrollTop = $state(false);
  let toastMessage = $state("");
  let toastType = $state<"error" | "success">("error");
  let loadingMore = $state(false);
  let hasMore = $state(true);
  let replyingTo = $state<Post | null>(null);
  let quotingPost = $state<Post | null>(null);
  let lightboxSrc = $state("");
  let lightboxAlt = $state("");
  let sentinel = $state<HTMLDivElement>(null!);

  const blobs = createBlobCache();

  function showToast(message: string, type: "error" | "success" = "error") {
    toastMessage = message;
    toastType = type;
    setTimeout(() => (toastMessage = ""), 4000);
  }

  async function copyWithFeedback(text: string, label: string) {
    await copyToClipboard(text);
    copyFeedback = label;
    setTimeout(() => (copyFeedback = ""), 1500);
  }

  async function init() {
    try {
      nodeId = await invoke("get_node_id");
      // Redirect to welcome screen on first run (no profile)
      const profile = await invoke("get_my_profile");
      if (!profile) {
        goto("/welcome");
        return;
      }
      await seedOwnProfile(nodeId);
      await loadFeed();
      loading = false;
    } catch {
      setTimeout(init, 500);
    }
  }

  async function loadFeed() {
    try {
      const newPosts: Post[] = await invoke("get_feed", {
        limit: 20,
        before: null,
      });
      posts = newPosts;
      hasMore = newPosts.length >= 20;
    } catch (e) {
      showToast("Failed to load feed");
      console.error("Failed to load feed:", e);
    }
  }

  async function loadMore() {
    if (loadingMore || !hasMore || posts.length === 0) return;
    loadingMore = true;
    try {
      const oldest = posts[posts.length - 1];
      const olderPosts: Post[] = await invoke("get_feed", {
        limit: 20,
        before: oldest.timestamp,
      });
      if (olderPosts.length === 0) {
        hasMore = false;
      } else {
        posts = [...posts, ...olderPosts];
        hasMore = olderPosts.length >= 20;
      }
    } catch (e) {
      showToast("Failed to load more posts");
      console.error("Failed to load more:", e);
    }
    loadingMore = false;
  }

  async function syncAll() {
    syncing = true;
    try {
      const follows: { pubkey: string }[] = await invoke("get_follows");
      const results = await Promise.allSettled(
        follows.map((f) => invoke("sync_posts", { pubkey: f.pubkey })),
      );
      const failures = results.filter((r) => r.status === "rejected").length;
      if (failures > 0 && failures < follows.length) {
        showToast(`Synced, but ${failures} peer(s) unreachable`);
      } else if (failures > 0 && failures === follows.length) {
        showToast("Could not reach any peers");
      }
      await loadFeed();
    } catch (e) {
      showToast("Sync failed");
      console.error("Failed to sync:", e);
    }
    syncing = false;
  }

  async function handleFiles(e: Event) {
    const input = e.target as HTMLInputElement;
    const files = input.files;
    if (!files || files.length === 0) return;

    uploading = true;
    for (const file of files) {
      try {
        const buffer = await file.arrayBuffer();
        const data = Array.from(new Uint8Array(buffer));
        const result: { hash: string; ticket: string } = await invoke(
          "add_blob_bytes",
          { data },
        );

        const previewUrl = URL.createObjectURL(file);
        attachments = [
          ...attachments,
          {
            hash: result.hash,
            ticket: result.ticket,
            mime_type: file.type || "application/octet-stream",
            filename: file.name,
            size: file.size,
            previewUrl,
          },
        ];
      } catch (e) {
        showToast(`Failed to upload ${file.name}`);
        console.error("Failed to upload file:", file.name, e);
      }
    }
    uploading = false;
    input.value = "";
  }

  function removeAttachment(index: number) {
    const removed = attachments[index];
    if (removed) URL.revokeObjectURL(removed.previewUrl);
    attachments = attachments.filter((_, i) => i !== index);
  }

  async function submitPost() {
    if ((!newPost.trim() && attachments.length === 0) || posting) return;
    posting = true;
    try {
      const media = attachments.map(
        ({ hash, ticket, mime_type, filename, size }) => ({
          hash,
          ticket,
          mime_type,
          filename,
          size,
        }),
      );
      await invoke("create_post", {
        content: newPost,
        media: media.length > 0 ? media : null,
      });
      for (const a of attachments) URL.revokeObjectURL(a.previewUrl);
      newPost = "";
      attachments = [];
      await loadFeed();
    } catch (e) {
      showToast("Failed to create post");
      console.error("Failed to create post:", e);
    }
    posting = false;
  }

  async function confirmDelete(id: string) {
    pendingDeleteId = id;
  }

  async function executeDelete() {
    if (!pendingDeleteId) return;
    try {
      await invoke("delete_post", { id: pendingDeleteId });
      await loadFeed();
    } catch (e) {
      showToast("Failed to delete post");
      console.error("Failed to delete post:", e);
    }
    pendingDeleteId = null;
  }

  function cancelDelete() {
    pendingDeleteId = null;
  }

  function handleKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submitPost();
    }
  }

  function handleGlobalKey(e: KeyboardEvent) {
    if (e.key === "Escape" && pendingDeleteId) {
      cancelDelete();
    }
  }

  function scrollToTop() {
    window.scrollTo({ top: 0, behavior: "smooth" });
  }

  function handleScroll() {
    showScrollTop = window.scrollY > 400;
  }

  // Visibility-aware auto-sync
  let syncInterval: ReturnType<typeof setInterval> | null = null;

  function startAutoSync() {
    if (syncInterval) return;
    syncInterval = setInterval(() => syncAll(), 60000);
  }

  function stopAutoSync() {
    if (syncInterval) {
      clearInterval(syncInterval);
      syncInterval = null;
    }
  }

  function handleVisibility() {
    if (document.hidden) {
      stopAutoSync();
    } else {
      syncAll();
      startAutoSync();
    }
  }

  $effect(() => {
    return setupInfiniteScroll(sentinel, hasMore, loadingMore, loadMore);
  });

  onMount(() => {
    init();
    const unlisteners: Promise<UnlistenFn>[] = [];
    unlisteners.push(
      listen("feed-updated", () => {
        loadFeed();
      }),
    );
    unlisteners.push(
      listen("profile-updated", (event) => {
        const pubkey = event.payload as string;
        evictDisplayName(pubkey);
        loadFeed();
      }),
    );

    window.addEventListener("scroll", handleScroll);
    window.addEventListener("keydown", handleGlobalKey);
    document.addEventListener("visibilitychange", handleVisibility);
    startAutoSync();
    return () => {
      stopAutoSync();
      document.removeEventListener("visibilitychange", handleVisibility);
      window.removeEventListener("keydown", handleGlobalKey);
      window.removeEventListener("scroll", handleScroll);
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

{#if loading}
  <div class="loading">
    <div class="spinner"></div>
    <p>Starting node...</p>
  </div>
{:else}
  <div class="node-id">
    <span class="label">You</span>
    <code>{shortId(nodeId)}</code>
    <button
      class="copy-btn"
      onclick={() => copyWithFeedback(nodeId, "node-id")}
    >
      {copyFeedback === "node-id" ? "Copied!" : "Copy ID"}
    </button>
  </div>

  <div class="compose">
    <textarea
      bind:value={newPost}
      placeholder="What's on your mind?"
      rows="3"
      maxlength={MAX_POST_LENGTH}
      onkeydown={handleKey}
    ></textarea>
    <div class="compose-meta">
      <span class="hint">Shift+Enter for newline</span>
      <span
        class="char-count"
        class:warn={newPost.length > MAX_POST_LENGTH * 0.9}
      >
        {newPost.length}/{MAX_POST_LENGTH}
      </span>
    </div>

    {#if attachments.length > 0}
      <div class="attachment-previews">
        {#each attachments as att, i}
          <div class="attachment-preview">
            {#if isImage(att.mime_type)}
              <img src={att.previewUrl} alt={att.filename} />
            {:else if isVideo(att.mime_type)}
              <video src={att.previewUrl} muted></video>
            {:else}
              <div class="file-icon">{att.filename}</div>
            {/if}
            <button class="remove-btn" onclick={() => removeAttachment(i)}
              >&times;</button
            >
          </div>
        {/each}
      </div>
    {/if}

    <div class="compose-actions">
      <button
        class="attach-btn"
        onclick={() => fileInput.click()}
        disabled={uploading}
      >
        {uploading ? "Uploading..." : "Attach"}
      </button>
      <input
        bind:this={fileInput}
        type="file"
        multiple
        accept="image/*,video/*,audio/*,.pdf,.txt"
        onchange={handleFiles}
        hidden
      />
      <button
        class="post-btn"
        onclick={submitPost}
        disabled={posting || (!newPost.trim() && attachments.length === 0)}
      >
        {posting ? "Posting..." : "Post"}
      </button>
    </div>
  </div>

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
        showDelete={true}
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
          loadFeed();
        }}
        onquote={(p) => {
          quotingPost = quotingPost?.id === p.id ? null : p;
          replyingTo = null;
        }}
        onquoted={() => {
          quotingPost = null;
          loadFeed();
        }}
        onlightbox={(src, alt) => {
          lightboxSrc = src;
          lightboxAlt = alt;
        }}
      />
    {:else}
      <p class="empty">No posts yet. Write something or follow someone!</p>
    {/each}
  </div>

  {#if hasMore && posts.length > 0}
    <div bind:this={sentinel} class="sentinel">
      {#if loadingMore}
        <span class="btn-spinner"></span> Loading...
      {/if}
    </div>
  {/if}

  <button class="refresh" onclick={syncAll} disabled={syncing}>
    {#if syncing}
      <span class="btn-spinner"></span> Syncing...
    {:else}
      Refresh
    {/if}
  </button>
{/if}

{#if toastMessage}
  <div class="toast" class:error={toastType === "error"}>
    {toastMessage}
  </div>
{/if}

{#if showScrollTop}
  <button class="scroll-top" onclick={scrollToTop} aria-label="Scroll to top">
    &#8593;
  </button>
{/if}

<style>
  .node-id {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.6rem 0.85rem;
    background: #16213e;
    border: 1px solid #2a2a4a;
    border-radius: 10px;
    margin-bottom: 1rem;
  }

  .node-id .label {
    color: #888;
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    font-weight: 600;
  }

  .node-id code {
    color: #7dd3fc;
    font-size: 0.8rem;
    flex: 1;
    font-family: "SF Mono", "Fira Code", monospace;
  }

  .copy-btn {
    background: #2a2a4a;
    color: #c4b5fd;
    border: none;
    border-radius: 6px;
    padding: 0.25rem 0.6rem;
    font-size: 0.7rem;
    font-weight: 500;
    cursor: pointer;
    min-width: 52px;
    text-align: center;
    transition:
      background 0.15s,
      color 0.15s;
  }

  .copy-btn:hover {
    background: #3a3a5a;
    color: #e0d4ff;
  }

  .compose {
    margin-bottom: 1.25rem;
  }

  .compose textarea {
    width: 100%;
    background: #16213e;
    border: 1px solid #2a2a4a;
    border-radius: 10px;
    padding: 0.75rem;
    color: #e0e0e0;
    font-family: inherit;
    font-size: 0.95rem;
    resize: vertical;
    box-sizing: border-box;
    transition: border-color 0.2s;
  }

  .compose textarea:focus {
    outline: none;
    border-color: #a78bfa;
  }

  .compose-meta {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-top: 0.25rem;
  }

  .hint {
    font-size: 0.7rem;
    color: #444;
  }

  .char-count {
    font-size: 0.75rem;
    color: #555;
  }

  .char-count.warn {
    color: #f59e0b;
  }

  .compose-actions {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.5rem;
  }

  .attach-btn {
    background: #2a2a4a;
    color: #c4b5fd;
    border: none;
    border-radius: 8px;
    padding: 0.55rem 1rem;
    font-size: 0.85rem;
    cursor: pointer;
    font-family: inherit;
    font-weight: 500;
    transition:
      background 0.15s,
      color 0.15s;
  }

  .attach-btn:hover:not(:disabled) {
    background: #3a3a5a;
    color: #e0d4ff;
  }

  .attach-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .post-btn {
    flex: 1;
    background: #7c3aed;
    color: white;
    border: none;
    border-radius: 8px;
    padding: 0.55rem;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
    font-family: inherit;
    transition: background 0.15s;
  }

  .post-btn:hover:not(:disabled) {
    background: #6d28d9;
  }

  .post-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .attachment-previews {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.5rem;
    flex-wrap: wrap;
  }

  .attachment-preview {
    position: relative;
    width: 80px;
    height: 80px;
    border-radius: 6px;
    overflow: hidden;
    border: 1px solid #2a2a4a;
  }

  .attachment-preview img,
  .attachment-preview video {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .attachment-preview .file-icon {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: #0f0f23;
    color: #888;
    font-size: 0.6rem;
    text-align: center;
    padding: 0.25rem;
    word-break: break-all;
  }

  .remove-btn {
    position: absolute;
    top: 2px;
    right: 2px;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: rgba(0, 0, 0, 0.7);
    color: white;
    border: none;
    font-size: 0.75rem;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    line-height: 1;
  }

  .empty {
    text-align: center;
    color: #666;
    padding: 2rem;
  }

  .refresh {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.4rem;
    margin: 1rem auto;
    background: #2a2a4a;
    color: #c4b5fd;
    border: none;
    border-radius: 8px;
    padding: 0.5rem 1.5rem;
    font-size: 0.85rem;
    font-family: inherit;
    font-weight: 500;
    cursor: pointer;
    transition:
      background 0.15s,
      color 0.15s;
  }

  .refresh:hover:not(:disabled) {
    background: #3a3a5a;
    color: #e0d4ff;
  }

  .refresh:disabled {
    opacity: 0.7;
    cursor: default;
  }

  .scroll-top {
    position: fixed;
    bottom: 1.5rem;
    right: 1.5rem;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    background: #7c3aed;
    color: white;
    border: none;
    font-size: 1.2rem;
    cursor: pointer;
    z-index: 50;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.4);
    transition:
      background 0.2s,
      transform 0.2s;
    animation: fadeIn 0.2s ease-out;
  }

  .scroll-top:hover {
    background: #6d28d9;
    transform: scale(1.1);
  }

  @keyframes fadeIn {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }
</style>
