<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import Avatar from "$lib/Avatar.svelte";
  import Timeago from "$lib/Timeago.svelte";
  import PostActions from "$lib/PostActions.svelte";
  import MediaGrid from "$lib/MediaGrid.svelte";
  import ReplyContextBlock from "$lib/ReplyContextBlock.svelte";
  import QuotedPostEmbed from "$lib/QuotedPostEmbed.svelte";
  import { useDisplayName } from "$lib/name.svelte";
  import type { Post } from "$lib/types";
  import { getCachedAvatarTicket, renderContent } from "$lib/utils";

  let {
    post,
    nodeId,
    showAuthor = true,
    showDelete = false,
    showReplyContext = true,
    onreply,
    ondelete,
    onquote,
    onlightbox,
  }: {
    post: Post;
    nodeId: string;
    showAuthor?: boolean;
    showDelete?: boolean;
    showReplyContext?: boolean;
    onreply?: (post: Post) => void;
    ondelete?: (id: string) => void;
    onquote?: (post: Post) => void;
    onlightbox?: (src: string, alt: string) => void;
  } = $props();

  // Repost-only: a quote with no original content
  let quotedPost = $state<Post | null>(null);
  let isRepostOnly = $derived(
    post.quote_of && !post.content && post.media.length === 0,
  );
  let displayPost = $derived(isRepostOnly && quotedPost ? quotedPost : post);

  // Only fetch quoted post when repost-only (QuotedPostEmbed handles the normal case)
  $effect(() => {
    if (isRepostOnly && post.quote_of) {
      invoke("get_post", { id: post.quote_of })
        .then((qp) => {
          quotedPost = qp as Post | null;
        })
        .catch(() => {});
    }
  });

  // Reactive name resolution (replaces 4 separate $effect/$state blocks)
  const author = useDisplayName(
    () => displayPost.author,
    () => nodeId,
  );
  const repostAuthor = useDisplayName(
    () => post.author,
    () => nodeId,
  );
</script>

<article class="post">
  {#if isRepostOnly && showAuthor}
    <div class="repost-label">
      <a href="/profile/{post.author}" class="repost-author"
        >{repostAuthor.name}</a
      >
      <span>reposted</span>
    </div>
  {/if}

  <div class="post-header">
    {#if showAuthor}
      <a href="/profile/{displayPost.author}" class="author-link">
        <Avatar
          pubkey={displayPost.author}
          name={author.name}
          isSelf={displayPost.author === nodeId}
          ticket={getCachedAvatarTicket(displayPost.author)}
        />
        <span class="author" class:self={displayPost.author === nodeId}>
          {author.name}
        </span>
      </a>
    {/if}
    <div class="post-header-right">
      <a href="/post/{displayPost.id}" class="time-link">
        <Timeago timestamp={displayPost.timestamp} />
      </a>
      {#if showDelete && post.author === nodeId && ondelete}
        <button class="delete-btn" onclick={() => ondelete(post.id)}>
          &times;
        </button>
      {/if}
    </div>
  </div>

  {#if showReplyContext && post.reply_to}
    <ReplyContextBlock replyToId={post.reply_to} {nodeId} />
  {/if}

  {#if isRepostOnly && quotedPost}
    {#if quotedPost.content}
      <p class="post-content">
        {@html renderContent(quotedPost.content, nodeId)}
      </p>
    {/if}
    <MediaGrid media={quotedPost.media} {onlightbox} />
  {:else}
    {#if post.content}
      <p class="post-content">{@html renderContent(post.content, nodeId)}</p>
    {/if}
    <MediaGrid media={post.media} {onlightbox} />
    {#if post.quote_of}
      <QuotedPostEmbed quoteOfId={post.quote_of} {nodeId} />
    {/if}
  {/if}

  <PostActions
    postId={post.id}
    postAuthor={post.author}
    onreply={() => onreply?.(post)}
    onquote={() => onquote?.(post)}
  />
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

  .post-content :global(a.mention) {
    color: #c4b5fd;
    font-weight: 600;
  }
</style>
