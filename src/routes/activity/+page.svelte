<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import PostCard from "$lib/PostCard.svelte";
  import Lightbox from "$lib/Lightbox.svelte";
  import Avatar from "$lib/Avatar.svelte";
  import Timeago from "$lib/Timeago.svelte";
  import { createBlobCache, setBlobContext } from "$lib/blobs";
  import type { AppNotification, Post } from "$lib/types";
  import { setupInfiniteScroll, getDisplayName, shortId } from "$lib/utils";

  let nodeId = $state("");
  let notifications = $state<AppNotification[]>([]);
  let loading = $state(true);
  let hasMore = $state(true);
  let loadingMore = $state(false);
  let sentinel = $state<HTMLDivElement>(null!);
  let lightboxSrc = $state("");
  let lightboxAlt = $state("");
  let filter = $state("all");
  let postCache = $state<Record<string, Post>>({});
  let nameCache = $state<Record<string, string>>({});

  const FILTERS = [
    { value: "all", label: "All" },
    { value: "mention", label: "Mentions" },
    { value: "like", label: "Likes" },
    { value: "reply", label: "Replies" },
    { value: "quote", label: "Quotes" },
    { value: "follower", label: "Followers" },
  ] as const;

  const blobs = createBlobCache();
  setBlobContext(blobs);

  let filtered = $derived(
    filter === "all"
      ? notifications
      : notifications.filter((n) => n.kind === filter),
  );

  async function resolveName(pubkey: string): Promise<string> {
    if (nameCache[pubkey]) return nameCache[pubkey];
    const name = await getDisplayName(pubkey, nodeId);
    nameCache = { ...nameCache, [pubkey]: name };
    return name;
  }

  async function fetchPostsForNotifications(notifs: AppNotification[]) {
    const ids = new Set<string>();
    for (const n of notifs) {
      if (n.post_id && !postCache[n.post_id]) ids.add(n.post_id);
      if (n.target_post_id && !postCache[n.target_post_id])
        ids.add(n.target_post_id);
    }
    const actors = new Set<string>();
    for (const n of notifs) {
      if (!nameCache[n.actor]) actors.add(n.actor);
    }
    const postPromises = [...ids].map(async (id) => {
      try {
        const post: Post | null = await invoke("get_post", { id });
        if (post) {
          postCache = { ...postCache, [id]: post };
        }
      } catch {
        // post may have been deleted
      }
    });
    const namePromises = [...actors].map((pubkey) => resolveName(pubkey));
    await Promise.all([...postPromises, ...namePromises]);
  }

  async function init() {
    try {
      nodeId = await invoke("get_node_id");
      await loadNotifications();
      await invoke("mark_notifications_read");
      loading = false;
    } catch {
      setTimeout(init, 500);
    }
  }

  async function loadNotifications() {
    try {
      const result: AppNotification[] = await invoke("get_notifications", {
        limit: 30,
      });
      notifications = result;
      hasMore = result.length >= 30;
      await fetchPostsForNotifications(result);
    } catch (e) {
      console.error("Failed to load notifications:", e);
    }
  }

  async function loadMore() {
    if (loadingMore || !hasMore || notifications.length === 0) return;
    loadingMore = true;
    try {
      const oldest = notifications[notifications.length - 1];
      const more: AppNotification[] = await invoke("get_notifications", {
        limit: 30,
        before: oldest.timestamp,
      });
      if (more.length === 0) {
        hasMore = false;
      } else {
        notifications = [...notifications, ...more];
        hasMore = more.length >= 30;
        await fetchPostsForNotifications(more);
      }
    } catch (e) {
      console.error("Failed to load more notifications:", e);
    }
    loadingMore = false;
  }

  function postForNotification(n: AppNotification): Post | undefined {
    if (n.kind === "like") {
      return n.target_post_id ? postCache[n.target_post_id] : undefined;
    }
    return n.post_id ? postCache[n.post_id] : undefined;
  }

  function notifLabel(kind: string): string {
    switch (kind) {
      case "mention":
        return "mentioned you";
      case "like":
        return "liked your post";
      case "reply":
        return "replied to your post";
      case "quote":
        return "quoted your post";
      case "follower":
        return "started following you";
      default:
        return "";
    }
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
      listen("notification-received", () => {
        loadNotifications();
        invoke("mark_notifications_read");
      }),
    );
    unlisteners.push(
      listen("mentioned-in-post", () => {
        loadNotifications();
        invoke("mark_notifications_read");
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

<h2 class="page-title">Notifications</h2>

<div class="filter-bar">
  {#each FILTERS as f (f.value)}
    <button
      class="filter-chip"
      class:active={filter === f.value}
      onclick={() => (filter = f.value)}
    >
      {f.label}
    </button>
  {/each}
</div>

{#if loading}
  <div class="loading">
    <div class="spinner"></div>
    <p>Loading notifications...</p>
  </div>
{:else if filtered.length === 0}
  <div class="empty">
    <p>No notifications yet.</p>
    <p class="hint">
      Mentions, likes, replies, quotes, and new followers will appear here.
    </p>
  </div>
{:else}
  <div class="notifications">
    {#each filtered as notif (notif.id)}
      {@const post = postForNotification(notif)}
      <div class="notif" class:unread={!notif.read}>
        <div class="notif-header">
          <a href="/profile/{notif.actor}" class="notif-actor">
            <Avatar
              pubkey={notif.actor}
              name={nameCache[notif.actor] || shortId(notif.actor)}
              size={28}
            />
            <span class="actor-name"
              >{nameCache[notif.actor] || shortId(notif.actor)}</span
            >
          </a>
          <span class="notif-label">{notifLabel(notif.kind)}</span>
          <Timeago timestamp={notif.timestamp} />
        </div>

        {#if notif.kind === "follower"}
          <a href="/profile/{notif.actor}" class="follower-link">View profile</a
          >
        {:else if post}
          <div class="notif-post">
            <PostCard
              {post}
              {nodeId}
              onlightbox={(src, alt) => {
                lightboxSrc = src;
                lightboxAlt = alt;
              }}
            />
          </div>
        {:else}
          <p class="notif-deleted">Post no longer available</p>
        {/if}
      </div>
    {/each}
  </div>

  {#if hasMore && notifications.length > 0}
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
    margin: 0 0 0.75rem;
  }

  .filter-bar {
    display: flex;
    gap: 0.4rem;
    margin-bottom: 1rem;
    flex-wrap: wrap;
  }

  .filter-chip {
    background: #2a2a4a;
    color: #888;
    border: 1px solid transparent;
    border-radius: 999px;
    padding: 0.3rem 0.75rem;
    font-size: 0.75rem;
    cursor: pointer;
    font-family: inherit;
    transition:
      background 0.2s,
      color 0.2s,
      border-color 0.2s;
  }

  .filter-chip:hover {
    color: #c4b5fd;
    border-color: #3a3a5a;
  }

  .filter-chip.active {
    background: #7c3aed;
    color: white;
    border-color: #7c3aed;
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

  .notif {
    border-bottom: 1px solid #1a1a3a;
    padding: 0.75rem 0;
  }

  .notif.unread {
    border-left: 3px solid #7c3aed;
    padding-left: 0.75rem;
  }

  .notif-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.85rem;
    color: #888;
    margin-bottom: 0.5rem;
    flex-wrap: wrap;
  }

  .notif-actor {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    text-decoration: none;
    color: inherit;
  }

  .notif-actor:hover .actor-name {
    text-decoration: underline;
  }

  .actor-name {
    color: #c4b5fd;
    font-weight: 500;
  }

  .notif-label {
    color: #888;
  }

  .notif-post {
    margin-top: 0.25rem;
  }

  .notif-deleted {
    color: #555;
    font-size: 0.8rem;
    font-style: italic;
    margin: 0.25rem 0 0;
  }

  .follower-link {
    display: inline-block;
    color: #a78bfa;
    font-size: 0.8rem;
    text-decoration: none;
    margin-top: 0.25rem;
  }

  .follower-link:hover {
    text-decoration: underline;
  }
</style>
