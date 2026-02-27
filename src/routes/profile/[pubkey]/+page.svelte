<script lang="ts">
  import { page } from "$app/state";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Lightbox from "$lib/Lightbox.svelte";
  import QrModal from "$lib/QrModal.svelte";
  import Avatar from "$lib/Avatar.svelte";
  import PostCard from "$lib/PostCard.svelte";
  import ReplyComposer from "$lib/ReplyComposer.svelte";
  import QuoteComposer from "$lib/QuoteComposer.svelte";
  import { createBlobCache, setBlobContext } from "$lib/blobs";
  import type {
    Post,
    Profile,
    FollowEntry,
    SyncResult,
    SyncStatus,
  } from "$lib/types";
  import {
    shortId,
    copyToClipboard,
    detectImageMime,
    avatarColor,
    getInitials,
    setupInfiniteScroll,
  } from "$lib/utils";

  let pubkey: string = $derived(page.params.pubkey ?? "");
  let nodeId = $state("");
  let profile = $state<Profile | null>(null);
  let posts = $state<Post[]>([]);
  let loading = $state(true);
  let isFollowing = $state(false);
  let toggling = $state(false);
  let copyFeedback = $state(false);
  let hasMore = $state(true);
  let loadingMore = $state(false);
  let replyingTo = $state<Post | null>(null);
  let quotingPost = $state<Post | null>(null);
  let lightboxSrc = $state("");
  let lightboxAlt = $state("");
  let toastMessage = $state("");
  let toastType = $state<"error" | "success">("error");
  let sentinel = $state<HTMLDivElement>(null!);
  let mediaFilter = $state("all");
  let syncStatus = $state<SyncStatus | null>(null);
  let remoteTotal = $state<number | null>(null);
  let fetchingRemote = $state(false);
  let peerOffline = $state(false);
  let pendingDeleteId = $state<string | null>(null);
  let isMuted = $state(false);
  let isBlocked = $state(false);
  let togglingMute = $state(false);
  let togglingBlock = $state(false);
  let showQr = $state(false);

  // Profile editing (isSelf only)
  let editingProfile = $state(false);
  let editDisplayName = $state("");
  let editBio = $state("");
  let editAvatarHash = $state<string | null>(null);
  let editAvatarTicket = $state<string | null>(null);
  let editAvatarPreview = $state<string | null>(null);
  let editIsPrivate = $state(false);
  let savedDisplayName = $state("");
  let savedBio = $state("");
  let savedAvatarHash = $state<string | null>(null);
  let savedIsPrivate = $state(false);
  let saving = $state(false);
  let uploading = $state(false);
  let fileInput = $state<HTMLInputElement>(null!);
  let isDirty = $derived(
    editDisplayName !== savedDisplayName ||
      editBio !== savedBio ||
      editAvatarHash !== savedAvatarHash ||
      editIsPrivate !== savedIsPrivate,
  );

  const FILTERS = [
    { value: "all", label: "All" },
    { value: "images", label: "Images" },
    { value: "videos", label: "Videos" },
    { value: "audio", label: "Audio" },
    { value: "files", label: "Files" },
    { value: "text", label: "Text" },
  ] as const;

  const blobs = createBlobCache();
  setBlobContext(blobs);

  function showToast(message: string, type: "error" | "success" = "error") {
    toastMessage = message;
    toastType = type;
    setTimeout(() => (toastMessage = ""), 4000);
  }

  let isSelf = $derived(pubkey === nodeId);
  let displayName = $derived(
    profile?.display_name || (isSelf ? "You" : shortId(pubkey)),
  );

  async function init() {
    try {
      nodeId = await invoke("get_node_id");
      if (isSelf) {
        const myProfile: Profile | null = await invoke("get_my_profile");
        profile = myProfile;
      } else {
        profile = await invoke("get_remote_profile", { pubkey });
      }

      const allPosts: Post[] = await invoke("get_user_posts", {
        pubkey,
        limit: 20,
        before: null,
        mediaFilter: mediaFilter === "all" ? null : mediaFilter,
      });
      posts = allPosts;
      hasMore = allPosts.length >= 20;

      const follows: FollowEntry[] = await invoke("get_follows");
      isFollowing = follows.some((f) => f.pubkey === pubkey);

      if (!isSelf) {
        isMuted = await invoke("is_muted", { pubkey });
        isBlocked = await invoke("is_blocked", { pubkey });
      }

      if (!isSelf) {
        try {
          syncStatus = await invoke("get_sync_status", { pubkey });
        } catch {
          // sync status is informational
        }
      }

      loading = false;
    } catch {
      setTimeout(init, 500);
    }
  }

  async function reloadPosts() {
    try {
      const newPosts: Post[] = await invoke("get_user_posts", {
        pubkey,
        limit: 20,
        before: null,
        mediaFilter: mediaFilter === "all" ? null : mediaFilter,
      });
      posts = newPosts;
      hasMore = newPosts.length >= 20;
    } catch (e) {
      console.error("Failed to reload posts:", e);
    }
  }

  async function reloadProfile() {
    try {
      if (isSelf) {
        profile = await invoke("get_my_profile");
      } else {
        profile = await invoke("get_remote_profile", { pubkey });
      }
    } catch (e) {
      console.error("Failed to reload profile:", e);
    }
  }

  async function loadMore() {
    if (loadingMore || !hasMore || posts.length === 0) return;
    loadingMore = true;
    try {
      const oldest = posts[posts.length - 1];
      const olderPosts: Post[] = await invoke("get_user_posts", {
        pubkey,
        limit: 20,
        before: oldest.timestamp,
        mediaFilter: mediaFilter === "all" ? null : mediaFilter,
      });
      if (olderPosts.length > 0) {
        posts = [...posts, ...olderPosts];
        hasMore = olderPosts.length >= 20;
      } else if (!isSelf && !peerOffline && mediaFilter === "all") {
        await fetchFromRemote();
      } else {
        hasMore = false;
      }
    } catch (e) {
      showToast("Failed to load more posts");
      console.error("Failed to load more:", e);
    }
    loadingMore = false;
  }

  async function fetchFromRemote() {
    fetchingRemote = true;
    try {
      const result: SyncResult = await invoke("fetch_older_posts", {
        pubkey,
      });
      remoteTotal = result.remote_total;
      if (result.posts.length > 0) {
        posts = [...posts, ...result.posts];
        syncStatus = await invoke("get_sync_status", { pubkey });
      }
      hasMore = false;
    } catch {
      peerOffline = true;
      hasMore = false;
    }
    fetchingRemote = false;
  }

  async function toggleFollow() {
    toggling = true;
    try {
      if (isFollowing) {
        await invoke("unfollow_user", { pubkey });
        isFollowing = false;
      } else {
        await invoke("follow_user", { pubkey });
        isFollowing = true;
      }
    } catch (e) {
      showToast(`Failed to ${isFollowing ? "unfollow" : "follow"}`);
      console.error("Toggle follow failed:", e);
    }
    toggling = false;
  }

  async function toggleMute() {
    togglingMute = true;
    try {
      if (isMuted) {
        await invoke("unmute_user", { pubkey });
        isMuted = false;
      } else {
        await invoke("mute_user", { pubkey });
        isMuted = true;
      }
    } catch (e) {
      showToast("Failed to toggle mute");
      console.error("Toggle mute failed:", e);
    }
    togglingMute = false;
  }

  async function toggleBlock() {
    togglingBlock = true;
    try {
      if (isBlocked) {
        await invoke("unblock_user", { pubkey });
        isBlocked = false;
      } else {
        await invoke("block_user", { pubkey });
        isBlocked = true;
        isFollowing = false;
      }
    } catch (e) {
      showToast("Failed to toggle block");
      console.error("Toggle block failed:", e);
    }
    togglingBlock = false;
  }

  async function copyNodeId() {
    await copyToClipboard(pubkey);
    copyFeedback = true;
    setTimeout(() => (copyFeedback = false), 1500);
  }

  function startEditing() {
    if (!profile) return;
    editDisplayName = profile.display_name;
    editBio = profile.bio;
    editAvatarHash = profile.avatar_hash;
    editAvatarTicket = profile.avatar_ticket;
    editIsPrivate = profile.is_private;
    savedDisplayName = profile.display_name;
    savedBio = profile.bio;
    savedAvatarHash = profile.avatar_hash;
    savedIsPrivate = profile.is_private;
    if (profile.avatar_ticket) {
      loadEditAvatarPreview(profile.avatar_ticket);
    }
    editingProfile = true;
  }

  function cancelEditing() {
    editingProfile = false;
    if (editAvatarPreview) {
      URL.revokeObjectURL(editAvatarPreview);
      editAvatarPreview = null;
    }
  }

  async function loadEditAvatarPreview(ticket: string) {
    try {
      const bytes: number[] = await invoke("fetch_blob_bytes", { ticket });
      const data = new Uint8Array(bytes);
      const blob = new Blob([data], { type: detectImageMime(data) });
      if (editAvatarPreview) URL.revokeObjectURL(editAvatarPreview);
      editAvatarPreview = URL.createObjectURL(blob);
    } catch (e) {
      console.error("Failed to load avatar:", e);
    }
  }

  async function handleAvatarFile(e: Event) {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    uploading = true;
    try {
      const buffer = await file.arrayBuffer();
      const data = Array.from(new Uint8Array(buffer));
      const result: { hash: string; ticket: string } = await invoke(
        "add_blob_bytes",
        { data },
      );
      editAvatarHash = result.hash;
      editAvatarTicket = result.ticket;
      if (editAvatarPreview) URL.revokeObjectURL(editAvatarPreview);
      editAvatarPreview = URL.createObjectURL(file);
    } catch (err) {
      showToast(`Upload failed: ${err}`);
    }
    uploading = false;
    input.value = "";
  }

  function removeAvatar() {
    editAvatarHash = null;
    editAvatarTicket = null;
    if (editAvatarPreview) {
      URL.revokeObjectURL(editAvatarPreview);
      editAvatarPreview = null;
    }
  }

  async function saveProfile() {
    saving = true;
    editDisplayName = editDisplayName.trim();
    editBio = editBio.trim();
    try {
      await invoke("save_my_profile", {
        displayName: editDisplayName,
        bio: editBio,
        avatarHash: editAvatarHash,
        avatarTicket: editAvatarTicket,
        isPrivate: editIsPrivate,
      });
      savedDisplayName = editDisplayName;
      savedBio = editBio;
      savedAvatarHash = editAvatarHash;
      savedIsPrivate = editIsPrivate;
      await reloadProfile();
      editingProfile = false;
      showToast("Profile saved", "success");
    } catch (err) {
      showToast(`Error: ${err}`);
    }
    saving = false;
  }

  function confirmDelete(id: string) {
    pendingDeleteId = id;
  }

  async function executeDelete() {
    if (!pendingDeleteId) return;
    try {
      await invoke("delete_post", { id: pendingDeleteId });
      await reloadPosts();
    } catch (e) {
      showToast("Failed to delete post");
      console.error("Failed to delete post:", e);
    }
    pendingDeleteId = null;
  }

  function cancelDelete() {
    pendingDeleteId = null;
  }

  function handleGlobalKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      if (pendingDeleteId) cancelDelete();
      else if (editingProfile) cancelEditing();
      else if (showQr) showQr = false;
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

  let filterInitialized = false;
  $effect(() => {
    mediaFilter; // track dependency
    if (!filterInitialized) {
      filterInitialized = true;
      return;
    }
    posts = [];
    hasMore = true;
    reloadPosts();
  });

  onMount(() => {
    init();
    const unlisteners: Promise<UnlistenFn>[] = [];
    unlisteners.push(
      listen("feed-updated", () => {
        reloadPosts();
      }),
    );
    unlisteners.push(
      listen("profile-updated", (event) => {
        if (event.payload === pubkey) {
          reloadProfile();
        }
      }),
    );
    window.addEventListener("keydown", handleGlobalKey);
    return () => {
      if (editAvatarPreview) URL.revokeObjectURL(editAvatarPreview);
      blobs.revokeAll();
      unlisteners.forEach((p) => p.then((fn) => fn()));
      window.removeEventListener("keydown", handleGlobalKey);
    };
  });
</script>

{#if showQr}
  <QrModal nodeId={pubkey} onclose={() => (showQr = false)} />
{/if}

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
    <p>Loading profile...</p>
  </div>
{:else}
  {#if !isSelf}
    <a href="/" class="back-link">&larr; Back to feed</a>
  {/if}

  {#if isSelf && editingProfile}
    <h2 class="edit-heading">Edit Profile</h2>
    <div class="edit-form">
      <div class="field">
        <span class="field-label">Avatar</span>
        <div class="avatar-row">
          {#if editAvatarPreview}
            <img
              src={editAvatarPreview}
              alt="Avatar"
              class="avatar-edit-preview"
            />
          {:else}
            <div
              class="avatar-fallback"
              style="background:{avatarColor(pubkey)}"
            >
              {getInitials(editDisplayName || "You", !editDisplayName)}
            </div>
          {/if}
          <div class="avatar-actions">
            <button
              class="avatar-btn"
              onclick={() => fileInput.click()}
              disabled={uploading}
            >
              {uploading
                ? "Uploading..."
                : editAvatarHash
                  ? "Change"
                  : "Upload"}
            </button>
            {#if editAvatarHash}
              <button class="avatar-btn remove" onclick={removeAvatar}
                >Remove</button
              >
            {/if}
          </div>
          <input
            bind:this={fileInput}
            type="file"
            accept="image/*"
            onchange={handleAvatarFile}
            hidden
          />
        </div>
      </div>

      <div class="field">
        <span class="field-label">Display Name</span>
        <input bind:value={editDisplayName} placeholder="Anonymous" />
      </div>

      <div class="field">
        <span class="field-label">Bio</span>
        <textarea
          bind:value={editBio}
          placeholder="Tell the world about yourself..."
          rows="3"
        ></textarea>
      </div>

      <div class="field">
        <span class="field-label">Privacy</span>
        <label class="toggle-row">
          <span class="toggle-switch" class:on={editIsPrivate}>
            <input type="checkbox" bind:checked={editIsPrivate} />
            <span class="toggle-track">
              <span class="toggle-thumb"></span>
            </span>
          </span>
          <span class="toggle-text">Private profile</span>
        </label>
        <p class="field-hint">
          When enabled, only followers can sync your posts and profile.
        </p>
      </div>

      <div class="edit-actions">
        <button class="cancel-btn" onclick={cancelEditing}>Cancel</button>
        <button
          class="save-btn"
          onclick={saveProfile}
          disabled={saving || !isDirty}
        >
          {saving ? "Saving..." : "Save"}
        </button>
      </div>
    </div>
  {:else}
    <div class="profile-header">
      <Avatar
        {pubkey}
        name={displayName}
        {isSelf}
        ticket={profile?.avatar_ticket}
        size={56}
      />
      <div class="profile-info">
        <h2>{displayName}</h2>
        {#if profile?.is_private}
          <span class="private-badge">Private profile</span>
        {/if}
        {#if profile?.bio}
          <p class="bio">{profile.bio}</p>
        {/if}
      </div>
      {#if isSelf}
        <button class="edit-btn" onclick={startEditing}>Edit</button>
      {/if}
    </div>
  {/if}

  <div class="id-row">
    <code>{pubkey}</code>
    <button class="copy-btn" onclick={copyNodeId}>
      {copyFeedback ? "Copied!" : "Copy ID"}
    </button>
    <button class="copy-btn" onclick={() => (showQr = true)}>QR</button>
  </div>

  {#if !isSelf}
    <div class="action-row">
      <button
        class="follow-toggle"
        class:following={isFollowing}
        onclick={toggleFollow}
        disabled={toggling || isBlocked}
      >
        {#if toggling}<span class="btn-spinner"></span>{:else}{isFollowing
            ? "Unfollow"
            : "Follow"}{/if}
      </button>
      <a href="/messages/{pubkey}" class="message-btn">Message</a>
    </div>
    <div class="moderation-row">
      <button
        class="mod-btn mute"
        class:active={isMuted}
        onclick={toggleMute}
        disabled={togglingMute}
      >
        {#if togglingMute}<span class="btn-spinner"></span>{:else}{isMuted
            ? "Unmute"
            : "Mute"}{/if}
      </button>
      <button
        class="mod-btn block"
        class:active={isBlocked}
        onclick={toggleBlock}
        disabled={togglingBlock}
      >
        {#if togglingBlock}<span class="btn-spinner"></span>{:else}{isBlocked
            ? "Unblock"
            : "Block"}{/if}
      </button>
    </div>
  {/if}

  <div class="filter-bar">
    {#each FILTERS as f (f.value)}
      <button
        class="filter-chip"
        class:active={mediaFilter === f.value}
        onclick={() => (mediaFilter = f.value)}
      >
        {f.label}
      </button>
    {/each}
  </div>

  <h3 class="section-title">
    Posts{posts.length > 0 ? ` (${posts.length}${hasMore ? "+" : ""})` : ""}
    {#if syncStatus && !isSelf}
      <span class="sync-info">
        {syncStatus.local_count}{remoteTotal != null ? ` / ${remoteTotal}` : ""} synced
      </span>
    {/if}
  </h3>

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
        showAuthor={false}
        showDelete={isSelf}
        onreply={(p) => {
          replyingTo = replyingTo?.id === p.id ? null : p;
          quotingPost = null;
        }}
        ondelete={confirmDelete}
        onquote={(p) => {
          quotingPost = quotingPost?.id === p.id ? null : p;
          replyingTo = null;
        }}
        onlightbox={(src, alt) => {
          lightboxSrc = src;
          lightboxAlt = alt;
        }}
      />
      {#if replyingTo?.id === post.id}
        <ReplyComposer
          replyToId={post.id}
          replyToAuthor={post.author}
          {nodeId}
          onsubmitted={() => {
            replyingTo = null;
            reloadPosts();
          }}
          oncancel={() => (replyingTo = null)}
        />
      {/if}
      {#if quotingPost?.id === post.id}
        <QuoteComposer
          quotedPost={post}
          {nodeId}
          onsubmitted={() => {
            quotingPost = null;
            reloadPosts();
          }}
          oncancel={() => (quotingPost = null)}
        />
      {/if}
    {:else}
      <p class="empty">No posts from this user yet.</p>
    {/each}
  </div>

  {#if hasMore && posts.length > 0}
    <div bind:this={sentinel} class="sentinel">
      {#if loadingMore}
        <span class="btn-spinner"></span>
        {#if fetchingRemote}
          Fetching from peer...
        {:else}
          Loading...
        {/if}
      {/if}
    </div>
  {/if}

  {#if peerOffline && !hasMore && posts.length > 0}
    <p class="offline-notice">End of cached posts -- peer is offline</p>
  {/if}
{/if}

{#if toastMessage}
  <div class="toast" class:error={toastType === "error"}>
    {toastMessage}
  </div>
{/if}

<style>
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

  .profile-header {
    display: flex;
    align-items: center;
    gap: 1rem;
    margin-bottom: 1rem;
  }

  .profile-info {
    flex: 1;
    min-width: 0;
  }

  .profile-info h2 {
    margin: 0;
    color: #a78bfa;
    font-size: 1.1rem;
  }

  .bio {
    margin: 0.25rem 0 0;
    color: #888;
    font-size: 0.85rem;
  }

  .private-badge {
    display: inline-block;
    font-size: 0.7rem;
    color: #f59e0b;
    border: 1px solid #f59e0b40;
    border-radius: 4px;
    padding: 0.15rem 0.5rem;
    margin-top: 0.25rem;
  }

  .id-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }

  code {
    background: #0f0f23;
    padding: 0.5rem 0.75rem;
    border-radius: 6px;
    font-size: 0.7rem;
    word-break: break-all;
    color: #7dd3fc;
    flex: 1;
  }

  .copy-btn {
    background: #2a2a4a;
    color: #c4b5fd;
    border: none;
    border-radius: 4px;
    padding: 0.4rem 0.75rem;
    font-size: 0.75rem;
    cursor: pointer;
    white-space: nowrap;
    min-width: 52px;
    text-align: center;
  }

  .copy-btn:hover {
    background: #3a3a5a;
  }

  .action-row {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }

  .follow-toggle {
    flex: 1;
    background: #7c3aed;
    color: white;
    border: none;
    border-radius: 6px;
    padding: 0.5rem;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-height: 2.2rem;
  }

  .message-btn {
    background: #2a2a4a;
    color: #c4b5fd;
    border: none;
    border-radius: 6px;
    padding: 0.5rem 1rem;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
    text-decoration: none;
    text-align: center;
    transition: background 0.2s;
  }

  .message-btn:hover {
    background: #3a3a5a;
  }

  .follow-toggle:hover:not(:disabled) {
    background: #6d28d9;
  }

  .follow-toggle.following {
    background: transparent;
    color: #f87171;
    border: 1px solid #f8717140;
  }

  .follow-toggle.following:hover:not(:disabled) {
    background: #f8717120;
  }

  .follow-toggle:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .moderation-row {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }

  .mod-btn {
    flex: 1;
    background: transparent;
    border: 1px solid #2a2a4a;
    border-radius: 6px;
    padding: 0.35rem;
    font-size: 0.8rem;
    font-weight: 500;
    cursor: pointer;
    font-family: inherit;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-height: 1.8rem;
    transition:
      color 0.15s,
      background 0.15s,
      border-color 0.15s;
  }

  .mod-btn.mute {
    color: #888;
  }

  .mod-btn.mute:hover:not(:disabled) {
    color: #f59e0b;
    border-color: #f59e0b40;
    background: #f59e0b10;
  }

  .mod-btn.mute.active {
    color: #f59e0b;
    border-color: #f59e0b40;
  }

  .mod-btn.mute.active:hover:not(:disabled) {
    background: #f59e0b10;
  }

  .mod-btn.block {
    color: #888;
  }

  .mod-btn.block:hover:not(:disabled) {
    color: #ef4444;
    border-color: #ef444440;
    background: #ef444410;
  }

  .mod-btn.block.active {
    color: #ef4444;
    border-color: #ef444440;
  }

  .mod-btn.block.active:hover:not(:disabled) {
    background: #ef444410;
  }

  .mod-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .filter-bar {
    display: flex;
    gap: 0.4rem;
    margin-bottom: 0.75rem;
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

  .section-title {
    color: #888;
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin: 0 0 0.75rem;
  }

  .empty {
    text-align: center;
    color: #666;
    padding: 2rem;
  }

  .sync-info {
    font-size: 0.7rem;
    color: #666;
    font-weight: 400;
    text-transform: none;
    letter-spacing: normal;
    margin-left: 0.5rem;
  }

  .offline-notice {
    text-align: center;
    color: #666;
    font-size: 0.8rem;
    padding: 0.75rem;
    border-top: 1px solid #2a2a4a;
    margin-top: 0.5rem;
  }

  .edit-btn {
    background: #2a2a4a;
    color: #c4b5fd;
    border: none;
    border-radius: 6px;
    padding: 0.4rem 0.85rem;
    font-size: 0.8rem;
    font-weight: 500;
    cursor: pointer;
    font-family: inherit;
    flex-shrink: 0;
    transition:
      background 0.15s,
      color 0.15s;
  }

  .edit-btn:hover {
    background: #3a3a5a;
    color: #e0d4ff;
  }

  .edit-heading {
    color: #a78bfa;
    margin: 0 0 1rem;
    font-size: 1.1rem;
  }

  .edit-form {
    background: #16213e;
    border: 1px solid #2a2a4a;
    border-radius: 8px;
    padding: 1.25rem;
    margin-bottom: 1rem;
  }

  .field {
    margin-bottom: 1rem;
  }

  .field-label {
    display: block;
    font-size: 0.8rem;
    color: #888;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 0.25rem;
  }

  .avatar-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .avatar-edit-preview {
    width: 56px;
    height: 56px;
    border-radius: 50%;
    object-fit: cover;
    flex-shrink: 0;
  }

  .avatar-fallback {
    width: 56px;
    height: 56px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 1rem;
    font-weight: 700;
    color: white;
    flex-shrink: 0;
    text-transform: uppercase;
  }

  .avatar-actions {
    display: flex;
    gap: 0.5rem;
  }

  .avatar-btn {
    background: #2a2a4a;
    color: #c4b5fd;
    border: none;
    border-radius: 4px;
    padding: 0.3rem 0.75rem;
    font-size: 0.8rem;
    cursor: pointer;
  }

  .avatar-btn:hover:not(:disabled) {
    background: #3a3a5a;
  }

  .avatar-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .avatar-btn.remove {
    color: #f87171;
  }

  .avatar-btn.remove:hover {
    background: #f8717120;
  }

  .edit-form input:not([type="checkbox"]),
  .edit-form textarea {
    width: 100%;
    background: #0f0f23;
    border: 1px solid #2a2a4a;
    border-radius: 6px;
    padding: 0.6rem 0.75rem;
    color: #e0e0e0;
    font-family: inherit;
    font-size: 0.9rem;
    box-sizing: border-box;
    resize: vertical;
  }

  .edit-form input:not([type="checkbox"]):focus,
  .edit-form textarea:focus {
    outline: none;
    border-color: #a78bfa;
  }

  .toggle-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    cursor: pointer;
  }

  .toggle-switch {
    position: relative;
    flex-shrink: 0;
  }

  .toggle-switch input {
    position: absolute;
    opacity: 0;
    width: 0;
    height: 0;
  }

  .toggle-track {
    display: block;
    width: 40px;
    height: 22px;
    background: #2a2a4a;
    border-radius: 11px;
    transition: background 0.2s;
  }

  .toggle-switch.on .toggle-track {
    background: #7c3aed;
  }

  .toggle-thumb {
    display: block;
    width: 16px;
    height: 16px;
    background: #888;
    border-radius: 50%;
    position: relative;
    top: 3px;
    left: 3px;
    transition:
      transform 0.2s,
      background 0.2s;
  }

  .toggle-switch.on .toggle-thumb {
    transform: translateX(18px);
    background: #fff;
  }

  .toggle-text {
    font-size: 0.9rem;
    color: #e0e0e0;
  }

  .field-hint {
    margin: 0.25rem 0 0;
    font-size: 0.75rem;
    color: #666;
  }

  .edit-actions {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.25rem;
  }

  .cancel-btn {
    flex: 1;
    background: #2a2a4a;
    color: #c4b5fd;
    border: none;
    border-radius: 6px;
    padding: 0.6rem;
    font-size: 0.9rem;
    font-weight: 500;
    cursor: pointer;
    font-family: inherit;
  }

  .cancel-btn:hover {
    background: #3a3a5a;
  }

  .save-btn {
    flex: 1;
    background: #7c3aed;
    color: white;
    border: none;
    border-radius: 6px;
    padding: 0.6rem;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
    font-family: inherit;
  }

  .save-btn:hover:not(:disabled) {
    background: #6d28d9;
  }

  .save-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
