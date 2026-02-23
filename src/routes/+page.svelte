<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Timeago from "$lib/Timeago.svelte";
  import {
    avatarColor,
    getInitials,
    shortId,
    getDisplayName,
    clearDisplayNameCache,
    evictDisplayName,
    copyToClipboard,
  } from "$lib/utils";

  interface MediaAttachment {
    hash: string;
    ticket: string;
    mime_type: string;
    filename: string;
    size: number;
  }

  interface Post {
    id: string;
    author: string;
    content: string;
    timestamp: number;
    media: MediaAttachment[];
  }

  interface PendingAttachment {
    hash: string;
    ticket: string;
    mime_type: string;
    filename: string;
    size: number;
    previewUrl: string;
  }

  const MAX_POST_LENGTH = 500;

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

  // Cache for fetched blob URLs so we don't re-fetch
  const blobUrlCache = new Map<string, string>();

  function collectMediaHashes(postList: Post[]): Set<string> {
    const hashes = new Set<string>();
    for (const p of postList) {
      if (p.media) {
        for (const m of p.media) hashes.add(m.hash);
      }
    }
    return hashes;
  }

  function revokeStaleBlobUrls(newPosts: Post[]) {
    const activeHashes = collectMediaHashes(newPosts);
    for (const [hash, url] of blobUrlCache) {
      if (!activeHashes.has(hash)) {
        URL.revokeObjectURL(url);
        blobUrlCache.delete(hash);
      }
    }
  }

  function revokeAllBlobUrls() {
    for (const url of blobUrlCache.values()) {
      URL.revokeObjectURL(url);
    }
    blobUrlCache.clear();
  }

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

  function escapeHtml(text: string): string {
    return text
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  function linkify(text: string): string {
    // Extract URLs before escaping, then rebuild with escaped non-URL segments
    const urlPattern = /https?:\/\/[^\s<>"')\]]+/g;
    const parts: string[] = [];
    let lastIndex = 0;
    let match;
    while ((match = urlPattern.exec(text)) !== null) {
      if (match.index > lastIndex) {
        parts.push(escapeHtml(text.slice(lastIndex, match.index)));
      }
      const url = match[0];
      parts.push(
        `<a href="${escapeHtml(url)}" target="_blank" rel="noopener noreferrer">${escapeHtml(url)}</a>`,
      );
      lastIndex = urlPattern.lastIndex;
    }
    if (lastIndex < text.length) {
      parts.push(escapeHtml(text.slice(lastIndex)));
    }
    return parts.join("");
  }

  async function init() {
    try {
      nodeId = await invoke("get_node_id");
      await loadFeed();
      loading = false;
    } catch {
      setTimeout(init, 500);
    }
  }

  async function loadFeed() {
    try {
      const newPosts: Post[] = await invoke("get_feed", {
        limit: 50,
        before: null,
      });
      revokeStaleBlobUrls(newPosts);
      posts = newPosts;
    } catch (e) {
      showToast("Failed to load feed");
      console.error("Failed to load feed:", e);
    }
  }

  async function syncAll() {
    syncing = true;
    try {
      const follows: { pubkey: string }[] = await invoke("get_follows");
      const results = await Promise.allSettled(
        follows.map((f) =>
          invoke("sync_posts", { pubkey: f.pubkey, before: null, limit: 50 }),
        ),
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
      // Clean up preview URLs
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

  function formatSize(bytes: number) {
    if (bytes < 1024) return bytes + " B";
    if (bytes < 1048576) return (bytes / 1024).toFixed(1) + " KB";
    return (bytes / 1048576).toFixed(1) + " MB";
  }

  function isImage(mime: string) {
    return mime.startsWith("image/");
  }

  function isVideo(mime: string) {
    return mime.startsWith("video/");
  }

  function handleKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submitPost();
    }
  }

  function scrollToTop() {
    window.scrollTo({ top: 0, behavior: "smooth" });
  }

  function handleScroll() {
    showScrollTop = window.scrollY > 400;
  }

  onMount(() => {
    init();
    const unlisteners: Promise<UnlistenFn>[] = [];
    unlisteners.push(
      listen("feed-updated", () => {
        clearDisplayNameCache();
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
    const interval = setInterval(() => {
      syncAll();
    }, 60000);
    return () => {
      clearInterval(interval);
      window.removeEventListener("scroll", handleScroll);
      unlisteners.forEach((p) => p.then((fn) => fn()));
      revokeAllBlobUrls();
    };
  });
</script>

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
      <article class="post">
        <div class="post-header">
          {#await getDisplayName(post.author, nodeId)}
            {@const fallback =
              post.author === nodeId ? "You" : shortId(post.author)}
            <div class="avatar" style="background:{avatarColor(post.author)}">
              {getInitials(fallback, post.author === nodeId)}
            </div>
            <span class="author" class:self={post.author === nodeId}>
              {fallback}
            </span>
          {:then name}
            <div class="avatar" style="background:{avatarColor(post.author)}">
              {getInitials(name, post.author === nodeId)}
            </div>
            <span class="author" class:self={post.author === nodeId}>
              {name}
            </span>
          {/await}
          <div class="post-header-right">
            <span class="time"><Timeago timestamp={post.timestamp} /></span>
            {#if post.author === nodeId}
              <button class="delete-btn" onclick={() => confirmDelete(post.id)}>
                &times;
              </button>
            {/if}
          </div>
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
                  <img src={url} alt={att.filename} class="media-img" />
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
                <div class="media-file">
                  <span>{att.filename}</span>
                  <span class="file-size">{formatSize(att.size)}</span>
                </div>
              {/if}
            {/each}
          </div>
        {/if}
      </article>
    {:else}
      <p class="empty">No posts yet. Write something or follow someone!</p>
    {/each}
  </div>

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

  .node-id {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.75rem 1rem;
    background: #16213e;
    border-radius: 8px;
    margin-bottom: 1rem;
  }

  .node-id .label {
    color: #888;
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .node-id code {
    color: #7dd3fc;
    font-size: 0.85rem;
    flex: 1;
  }

  .copy-btn {
    background: #2a2a4a;
    color: #c4b5fd;
    border: none;
    border-radius: 4px;
    padding: 0.25rem 0.5rem;
    font-size: 0.75rem;
    cursor: pointer;
    min-width: 52px;
    text-align: center;
  }

  .copy-btn:hover {
    background: #3a3a5a;
  }

  /* Compose */
  .compose {
    margin-bottom: 1.5rem;
  }

  .compose textarea {
    width: 100%;
    background: #16213e;
    border: 1px solid #2a2a4a;
    border-radius: 8px;
    padding: 0.75rem;
    color: #e0e0e0;
    font-family: inherit;
    font-size: 0.95rem;
    resize: vertical;
    box-sizing: border-box;
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
    border-radius: 6px;
    padding: 0.6rem 1rem;
    font-size: 0.85rem;
    cursor: pointer;
  }

  .attach-btn:hover:not(:disabled) {
    background: #3a3a5a;
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
    border-radius: 6px;
    padding: 0.6rem;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
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

  /* Delete confirmation modal */
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

  /* Posts */
  .post {
    background: #16213e;
    border: 1px solid #2a2a4a;
    border-radius: 8px;
    padding: 1rem;
    margin-bottom: 0.5rem;
    transition:
      border-color 0.2s,
      transform 0.15s;
    animation: fadeIn 0.3s ease-out;
  }

  .post:hover {
    border-color: #3a3a5a;
  }

  @keyframes fadeIn {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .post-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.5rem;
  }

  .post-header-right {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-left: auto;
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

  .delete-btn {
    background: none;
    border: none;
    color: #666;
    font-size: 1rem;
    cursor: pointer;
    padding: 0 0.25rem;
    line-height: 1;
  }

  .delete-btn:hover {
    color: #ef4444;
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

  .media-img {
    width: 100%;
    border-radius: 6px;
    max-height: 400px;
    object-fit: contain;
    background: #0f0f23;
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
    border-radius: 6px;
    padding: 0.75rem;
    display: flex;
    justify-content: space-between;
    align-items: center;
    color: #c4b5fd;
    font-size: 0.85rem;
  }

  .file-size {
    color: #666;
    font-size: 0.75rem;
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
    border-radius: 6px;
    padding: 0.5rem 1.5rem;
    font-size: 0.85rem;
    cursor: pointer;
  }

  .refresh:hover:not(:disabled) {
    background: #3a3a5a;
  }

  .refresh:disabled {
    opacity: 0.7;
    cursor: default;
  }

  /* Toast notifications */
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
    animation: toastIn 0.3s ease-out;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
  }

  .toast.error {
    border-left: 3px solid #ef4444;
  }

  @keyframes toastIn {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(10px);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
  }

  /* Scroll to top */
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
</style>
