<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import Avatar from "$lib/Avatar.svelte";
  import Timeago from "$lib/Timeago.svelte";
  import PostActions from "$lib/PostActions.svelte";
  import ReplyComposer from "$lib/ReplyComposer.svelte";
  import QuoteComposer from "$lib/QuoteComposer.svelte";
  import type { Post, MediaAttachment } from "$lib/types";
  import {
    shortId,
    getDisplayName,
    getCachedAvatarTicket,
    linkify,
    isImage,
    isVideo,
    formatSize,
  } from "$lib/utils";

  let {
    post,
    nodeId,
    showAuthor = true,
    showDelete = false,
    showReplyContext = true,
    replyingTo = null,
    quotingPost = null,
    onreply,
    ondelete,
    onreplied,
    onquote,
    onquoted,
    onlightbox,
    getBlobUrl,
    downloadFile,
  }: {
    post: Post;
    nodeId: string;
    showAuthor?: boolean;
    showDelete?: boolean;
    showReplyContext?: boolean;
    replyingTo?: Post | null;
    quotingPost?: Post | null;
    onreply?: (post: Post) => void;
    ondelete?: (id: string) => void;
    onreplied?: () => void;
    onquote?: (post: Post) => void;
    onquoted?: () => void;
    onlightbox?: (src: string, alt: string) => void;
    getBlobUrl: (attachment: MediaAttachment) => Promise<string>;
    downloadFile: (attachment: MediaAttachment) => void;
  } = $props();

  let replyContext = $state<{ author: string; preview: string } | null>(null);
  let quotedPost = $state<Post | null>(null);
  let quotedAuthorName = $state<string>("");

  $effect(() => {
    if (showReplyContext && post.reply_to) {
      loadReplyContext(post.reply_to);
    }
  });

  $effect(() => {
    if (post.quote_of) {
      loadQuotedPost(post.quote_of);
    }
  });

  async function loadReplyContext(parentId: string) {
    try {
      const parent: Post | null = await invoke("get_post", { id: parentId });
      if (parent) {
        const name = await getDisplayName(parent.author, nodeId);
        const preview =
          parent.content.length > 100
            ? parent.content.slice(0, 100) + "..."
            : parent.content;
        replyContext = { author: name, preview };
      }
    } catch {
      // parent not available locally
    }
  }

  async function loadQuotedPost(quotedId: string) {
    try {
      const qp: Post | null = await invoke("get_post", { id: quotedId });
      if (qp) {
        quotedPost = qp;
        quotedAuthorName = await getDisplayName(qp.author, nodeId);
      }
    } catch {
      // quoted post not available locally
    }
  }

  let isRepostOnly = $derived(
    post.quote_of && !post.content && post.media.length === 0,
  );
</script>

<article class="post">
  {#if isRepostOnly && showAuthor}
    <div class="repost-label">
      {#await getDisplayName(post.author, nodeId)}
        <span>{shortId(post.author)} reposted</span>
      {:then name}
        <a href="/user/{post.author}" class="repost-author">{name}</a>
        <span>reposted</span>
      {/await}
    </div>
  {/if}
  <div class="post-header">
    {#if showAuthor}
      {#if isRepostOnly && quotedPost}
        {#await getDisplayName(quotedPost.author, nodeId)}
          {@const fallback =
            quotedPost.author === nodeId ? "You" : shortId(quotedPost.author)}
          <a href="/user/{quotedPost.author}" class="author-link">
            <Avatar
              pubkey={quotedPost.author}
              name={fallback}
              isSelf={quotedPost.author === nodeId}
              ticket={getCachedAvatarTicket(quotedPost.author)}
            />
            <span class="author" class:self={quotedPost.author === nodeId}>
              {fallback}
            </span>
          </a>
        {:then name}
          <a href="/user/{quotedPost.author}" class="author-link">
            <Avatar
              pubkey={quotedPost.author}
              {name}
              isSelf={quotedPost.author === nodeId}
              ticket={getCachedAvatarTicket(quotedPost.author)}
            />
            <span class="author" class:self={quotedPost.author === nodeId}>
              {name}
            </span>
          </a>
        {/await}
      {:else}
        {#await getDisplayName(post.author, nodeId)}
          {@const fallback =
            post.author === nodeId ? "You" : shortId(post.author)}
          <a href="/user/{post.author}" class="author-link">
            <Avatar
              pubkey={post.author}
              name={fallback}
              isSelf={post.author === nodeId}
              ticket={getCachedAvatarTicket(post.author)}
            />
            <span class="author" class:self={post.author === nodeId}>
              {fallback}
            </span>
          </a>
        {:then name}
          <a href="/user/{post.author}" class="author-link">
            <Avatar
              pubkey={post.author}
              {name}
              isSelf={post.author === nodeId}
              ticket={getCachedAvatarTicket(post.author)}
            />
            <span class="author" class:self={post.author === nodeId}>
              {name}
            </span>
          </a>
        {/await}
      {/if}
    {/if}
    <div class="post-header-right">
      <a
        href="/post/{isRepostOnly && quotedPost ? quotedPost.id : post.id}"
        class="time-link"
      >
        <Timeago
          timestamp={isRepostOnly && quotedPost
            ? quotedPost.timestamp
            : post.timestamp}
        />
      </a>
      {#if showDelete && post.author === nodeId && ondelete}
        <button class="delete-btn" onclick={() => ondelete(post.id)}>
          &times;
        </button>
      {/if}
    </div>
  </div>
  {#if post.reply_to && replyContext}
    <a href="/post/{post.reply_to}" class="reply-context-block">
      <span class="reply-icon">{"\u21A9"}</span>
      <span class="reply-author">{replyContext.author}</span>
      {#if replyContext.preview}
        <span class="reply-preview">{replyContext.preview}</span>
      {/if}
    </a>
  {:else if post.reply_to}
    <a href="/post/{post.reply_to}" class="reply-context">
      {"\u21A9"} in reply to a post
    </a>
  {/if}
  {#if isRepostOnly && quotedPost}
    {#if quotedPost.content}
      <p class="post-content">{@html linkify(quotedPost.content)}</p>
    {/if}
    {#if quotedPost.media && quotedPost.media.length > 0}
      <div class="post-media" class:grid={quotedPost.media.length > 1}>
        {#each quotedPost.media as att (att.hash)}
          {#if isImage(att.mime_type)}
            {#await getBlobUrl(att)}
              <div class="media-placeholder">Loading...</div>
            {:then url}
              <button
                class="media-img-btn"
                onclick={() => onlightbox?.(url, att.filename)}
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
  {:else}
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
                onclick={() => onlightbox?.(url, att.filename)}
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
    {#if post.quote_of && quotedPost}
      <a href="/post/{quotedPost.id}" class="quoted-post">
        <div class="quoted-header">
          <Avatar
            pubkey={quotedPost.author}
            name={quotedAuthorName || shortId(quotedPost.author)}
            isSelf={quotedPost.author === nodeId}
            ticket={getCachedAvatarTicket(quotedPost.author)}
            size={20}
          />
          <span class="quoted-author"
            >{quotedAuthorName || shortId(quotedPost.author)}</span
          >
          <Timeago timestamp={quotedPost.timestamp} />
        </div>
        {#if quotedPost.content}
          <p class="quoted-content">
            {quotedPost.content.length > 200
              ? quotedPost.content.slice(0, 200) + "..."
              : quotedPost.content}
          </p>
        {/if}
      </a>
    {:else if post.quote_of}
      <a href="/post/{post.quote_of}" class="quoted-post unavailable">
        Quoted post unavailable
      </a>
    {/if}
  {/if}
  <PostActions
    postId={post.id}
    postAuthor={post.author}
    onreply={() => onreply?.(post)}
    onquote={() => onquote?.(post)}
  />
  {#if replyingTo?.id === post.id}
    <ReplyComposer
      replyToId={post.id}
      replyToAuthor={post.author}
      onsubmitted={() => onreplied?.()}
      oncancel={() => onreply?.(post)}
    />
  {/if}
  {#if quotingPost?.id === post.id}
    <QuoteComposer
      quotedPost={post}
      {nodeId}
      onsubmitted={() => onquoted?.()}
      oncancel={() => onquote?.(post)}
    />
  {/if}
</article>

<style>
  .post {
    background: #16213e;
    border: 1px solid #2a2a4a;
    border-radius: 12px;
    padding: 0.875rem 1rem;
    margin-bottom: 0.4rem;
    transition: border-color 0.2s;
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

  .repost-label {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    margin-bottom: 0.4rem;
    font-size: 0.75rem;
    color: #666;
  }

  .repost-author {
    color: #c4b5fd;
    text-decoration: none;
    font-weight: 600;
  }

  .repost-author:hover {
    text-decoration: underline;
  }

  .post-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.4rem;
  }

  .post-header-right {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin-left: auto;
    flex-shrink: 0;
  }

  .author-link {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    text-decoration: none;
    color: inherit;
    min-width: 0;
  }

  .author-link:hover .author {
    text-decoration: underline;
  }

  .author {
    font-weight: 600;
    font-size: 0.9rem;
    color: #c4b5fd;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .author.self {
    color: #a78bfa;
  }

  .delete-btn {
    background: none;
    border: none;
    color: #555;
    font-size: 1.1rem;
    cursor: pointer;
    padding: 0.15rem 0.3rem;
    line-height: 1;
    border-radius: 4px;
    transition:
      color 0.15s,
      background 0.15s;
  }

  .delete-btn:hover {
    color: #ef4444;
    background: #ef444415;
  }

  .time-link {
    color: #666;
    font-size: 0.75rem;
    white-space: nowrap;
    text-decoration: none;
  }

  .time-link:hover {
    color: #999;
    text-decoration: underline;
  }

  .reply-context {
    display: block;
    margin-bottom: 0.35rem;
    font-size: 0.75rem;
    color: #666;
    text-decoration: none;
  }

  .reply-context:hover {
    color: #a78bfa;
    text-decoration: underline;
  }

  .reply-context-block {
    display: flex;
    align-items: baseline;
    gap: 0.3rem;
    margin-bottom: 0.5rem;
    padding: 0.35rem 0.6rem;
    background: #0f0f23;
    border-left: 2px solid #3a3a5a;
    border-radius: 0 6px 6px 0;
    font-size: 0.75rem;
    color: #888;
    text-decoration: none;
    overflow: hidden;
  }

  .reply-context-block:hover {
    border-left-color: #a78bfa;
    color: #a78bfa;
  }

  .reply-icon {
    flex-shrink: 0;
    color: #666;
  }

  .reply-author {
    color: #c4b5fd;
    font-weight: 600;
    flex-shrink: 0;
  }

  .reply-preview {
    color: #666;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .post-content {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
    font-size: 0.95rem;
    line-height: 1.55;
    color: #e8e8e8;
  }

  .post-content :global(a) {
    color: #7dd3fc;
    text-decoration: none;
  }

  .post-content :global(a:hover) {
    text-decoration: underline;
  }

  .quoted-post {
    display: block;
    margin-top: 0.6rem;
    padding: 0.6rem 0.75rem;
    background: #0f0f23;
    border: 1px solid #2a2a4a;
    border-radius: 8px;
    text-decoration: none;
    color: inherit;
    transition: border-color 0.2s;
  }

  .quoted-post:hover {
    border-color: #3a3a5a;
  }

  .quoted-post.unavailable {
    color: #555;
    font-size: 0.8rem;
    font-style: italic;
  }

  .quoted-header {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin-bottom: 0.3rem;
    font-size: 0.75rem;
    color: #888;
  }

  .quoted-author {
    color: #c4b5fd;
    font-weight: 600;
  }

  .quoted-content {
    margin: 0;
    font-size: 0.85rem;
    line-height: 1.4;
    color: #aaa;
    white-space: pre-wrap;
    word-break: break-word;
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
    border-radius: 8px;
    max-height: 400px;
    object-fit: contain;
    background: #0f0f23;
    display: block;
  }

  .media-video {
    width: 100%;
    border-radius: 8px;
    max-height: 400px;
  }

  .media-placeholder {
    background: #0f0f23;
    border-radius: 8px;
    padding: 2rem;
    text-align: center;
    color: #666;
    font-size: 0.8rem;
  }

  .media-file {
    background: #0f0f23;
    border: 1px solid #2a2a4a;
    border-radius: 8px;
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
</style>
