<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Timeago from "$lib/Timeago.svelte";

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

  // Cache for fetched blob URLs so we don't re-fetch
  const blobUrlCache = new Map<string, string>();

  // Cache for remote display names
  const displayNameCache = new Map<string, string>();

  // Deterministic color from a pubkey for avatars
  function avatarColor(pubkey: string): string {
    const colors = [
      "#7c3aed",
      "#2563eb",
      "#059669",
      "#d97706",
      "#dc2626",
      "#db2777",
      "#7c3aed",
      "#0891b2",
    ];
    let hash = 0;
    for (let i = 0; i < pubkey.length; i++) {
      hash = pubkey.charCodeAt(i) + ((hash << 5) - hash);
    }
    return colors[Math.abs(hash) % colors.length];
  }

  function getInitials(name: string): string {
    if (!name || name === "You") return "Y";
    const parts = name.trim().split(/\s+/);
    if (parts.length >= 2) return (parts[0][0] + parts[1][0]).toUpperCase();
    return name.slice(0, 2).toUpperCase();
  }

  async function getDisplayName(pubkey: string): Promise<string> {
    if (pubkey === nodeId) return "You";
    const cached = displayNameCache.get(pubkey);
    if (cached !== undefined) return cached;
    try {
      const profile: { display_name: string; bio: string } | null =
        await invoke("get_remote_profile", { pubkey });
      const name =
        profile && profile.display_name
          ? profile.display_name
          : shortId(pubkey);
      displayNameCache.set(pubkey, name);
      return name;
    } catch {
      const name = shortId(pubkey);
      displayNameCache.set(pubkey, name);
      return name;
    }
  }

  async function copyToClipboard(text: string, label: string) {
    await navigator.clipboard.writeText(text);
    copyFeedback = label;
    setTimeout(() => (copyFeedback = ""), 1500);
  }

  function linkify(text: string): string {
    const urlPattern = /https?:\/\/[^\s<>\"')\]]+/g;
    return text.replace(
      urlPattern,
      (url) =>
        `<a href="${url}" target="_blank" rel="noopener noreferrer">${url}</a>`,
    );
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
      posts = await invoke("get_feed", { limit: 50, before: null });
    } catch (e) {
      console.error("Failed to load feed:", e);
    }
  }

  async function syncAll() {
    syncing = true;
    try {
      const follows: { pubkey: string }[] = await invoke("get_follows");
      await Promise.allSettled(
        follows.map((f) =>
          invoke("sync_posts", { pubkey: f.pubkey, before: null, limit: 50 }),
        ),
      );
      await loadFeed();
    } catch (e) {
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

  function shortId(id: string) {
    return id.slice(0, 8) + "..." + id.slice(-4);
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

  onMount(() => {
    init();
    listen("feed-updated", () => {
      displayNameCache.clear();
      loadFeed();
    });
    listen("profile-updated", (event) => {
      const pubkey = event.payload as string;
      displayNameCache.delete(pubkey);
      loadFeed();
    });
    const interval = setInterval(() => {
      syncAll();
    }, 60000);
    return () => clearInterval(interval);
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
    <button class="copy-btn" onclick={() => copyToClipboard(nodeId, "node-id")}>
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
          {#await getDisplayName(post.author)}
            {@const fallback =
              post.author === nodeId ? "You" : shortId(post.author)}
            <div class="avatar" style="background:{avatarColor(post.author)}">
              {getInitials(fallback)}
            </div>
            <span class="author" class:self={post.author === nodeId}>
              {fallback}
            </span>
          {:then name}
            <div class="avatar" style="background:{avatarColor(post.author)}">
              {getInitials(name)}
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

<style>
  /* Loading spinner */
  .loading {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 3rem;
    gap: 1rem;
    color: #888;
  }

  .spinner {
    width: 32px;
    height: 32px;
    border: 3px solid #2a2a4a;
    border-top-color: #a78bfa;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

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
    justify-content: flex-end;
    margin-top: 0.25rem;
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
</style>
