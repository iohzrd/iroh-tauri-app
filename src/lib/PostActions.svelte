<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import type { PostCounts, Interaction } from "$lib/types";

  let {
    postId,
    postAuthor,
    onreply,
    onquote,
  }: {
    postId: string;
    postAuthor: string;
    onreply?: () => void;
    onquote?: () => void;
  } = $props();

  let counts = $state<PostCounts>({
    likes: 0,
    replies: 0,
    reposts: 0,
    liked_by_me: false,
    reposted_by_me: false,
  });
  let liking = $state(false);
  let reposting = $state(false);
  let bookmarked = $state(false);
  let bookmarking = $state(false);

  async function loadCounts() {
    try {
      counts = await invoke("get_post_counts", { targetPostId: postId });
    } catch {
      // counts are best-effort
    }
  }

  async function loadBookmark() {
    try {
      bookmarked = await invoke("is_bookmarked", { postId });
    } catch {
      // best-effort
    }
  }

  $effect(() => {
    postId;
    loadCounts();
    loadBookmark();
  });

  onMount(() => {
    const unlisteners: Promise<UnlistenFn>[] = [];
    unlisteners.push(
      listen<Interaction>("interaction-received", (event) => {
        if (event.payload.target_post_id === postId) {
          loadCounts();
        }
      }),
    );
    unlisteners.push(
      listen<{ id: string; author: string }>("interaction-deleted", () => {
        loadCounts();
      }),
    );
    unlisteners.push(
      listen("feed-updated", () => {
        loadCounts();
      }),
    );
    return () => {
      unlisteners.forEach((p) => p.then((fn) => fn()));
    };
  });

  async function toggleLike() {
    if (liking) return;
    liking = true;
    try {
      if (counts.liked_by_me) {
        await invoke("unlike_post", { targetPostId: postId });
        counts.liked_by_me = false;
        counts.likes = Math.max(0, counts.likes - 1);
      } else {
        await invoke("like_post", {
          targetPostId: postId,
          targetAuthor: postAuthor,
        });
        counts.liked_by_me = true;
        counts.likes += 1;
      }
    } catch (e) {
      console.error("Failed to toggle like:", e);
    }
    liking = false;
  }

  async function toggleRepost() {
    if (reposting) return;
    reposting = true;
    try {
      if (counts.reposted_by_me) {
        await invoke("unrepost", { targetPostId: postId });
        counts.reposted_by_me = false;
        counts.reposts = Math.max(0, counts.reposts - 1);
      } else {
        await invoke("repost", {
          targetPostId: postId,
          targetAuthor: postAuthor,
        });
        counts.reposted_by_me = true;
        counts.reposts += 1;
      }
    } catch (e) {
      console.error("Failed to toggle repost:", e);
    }
    reposting = false;
  }

  async function toggleBookmark() {
    if (bookmarking) return;
    bookmarking = true;
    try {
      bookmarked = await invoke("toggle_bookmark", { postId });
    } catch (e) {
      console.error("Failed to toggle bookmark:", e);
    }
    bookmarking = false;
  }
</script>

<div class="post-actions">
  <button
    class="action-btn"
    class:active={counts.liked_by_me}
    onclick={toggleLike}
    disabled={liking}
    aria-label={counts.liked_by_me ? "Unlike" : "Like"}
  >
    <span class="icon">{counts.liked_by_me ? "\u2665" : "\u2661"}</span>
    {#if counts.likes > 0}<span class="count">{counts.likes}</span>{/if}
  </button>

  <button class="action-btn" onclick={onreply} aria-label="Reply">
    <span class="icon">{"\u21A9"}</span>
    {#if counts.replies > 0}<span class="count">{counts.replies}</span>{/if}
  </button>

  <button
    class="action-btn"
    class:active={counts.reposted_by_me}
    onclick={toggleRepost}
    disabled={reposting}
    title="Repost"
    aria-label={counts.reposted_by_me ? "Undo repost" : "Repost"}
  >
    <span class="icon">{"\u21BB"}</span>
    {#if counts.reposts > 0}<span class="count">{counts.reposts}</span>{/if}
  </button>

  <button class="action-btn" onclick={onquote} title="Quote" aria-label="Quote">
    <span class="icon">{"\u275D"}</span>
  </button>

  <button
    class="action-btn bookmark"
    class:active={bookmarked}
    onclick={toggleBookmark}
    disabled={bookmarking}
    title={bookmarked ? "Remove bookmark" : "Bookmark"}
    aria-label={bookmarked ? "Remove bookmark" : "Bookmark"}
  >
    <span class="icon">{bookmarked ? "\u2605" : "\u2606"}</span>
  </button>
</div>

<style>
  .post-actions {
    display: flex;
    gap: 0.25rem;
    margin-top: 0.6rem;
    padding-top: 0.5rem;
    border-top: 1px solid var(--border-faintest);
  }

  .action-btn {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: var(--text-base);
    padding: 0.4rem 0.6rem;
    min-height: 36px;
    border-radius: var(--radius-md);
    transition:
      color var(--transition-fast),
      background var(--transition-fast);
  }

  .action-btn:hover:not(:disabled) {
    color: var(--accent-light);
    background: var(--accent-light-hover-bg);
  }

  .action-btn.active {
    color: var(--color-error-light);
  }

  .action-btn.active:hover:not(:disabled) {
    color: var(--color-error-light);
    background: var(--color-error-light-hover-bg);
  }

  .action-btn.bookmark {
    margin-left: auto;
  }

  .action-btn.bookmark.active {
    color: var(--color-bookmark);
  }

  .action-btn.bookmark.active:hover:not(:disabled) {
    color: var(--color-bookmark);
    background: var(--color-bookmark-hover-bg);
  }

  .icon {
    font-size: var(--text-icon);
    line-height: 1;
  }

  .count {
    font-size: var(--text-sm);
  }
</style>
