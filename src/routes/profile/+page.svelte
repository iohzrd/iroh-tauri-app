<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import type { Profile } from "$lib/types";
  import {
    avatarColor,
    getInitials,
    copyToClipboard,
    detectImageMime,
  } from "$lib/utils";

  let nodeId = $state("");
  let displayName = $state("");
  let bio = $state("");
  let avatarHash = $state<string | null>(null);
  let avatarTicket = $state<string | null>(null);
  let avatarPreview = $state<string | null>(null);
  let loading = $state(true);
  let saving = $state(false);
  let uploading = $state(false);
  let status = $state("");
  let copyFeedback = $state(false);
  let fileInput = $state<HTMLInputElement>(null!);

  // Dirty-state tracking: saved values from last load/save
  let savedDisplayName = $state("");
  let savedBio = $state("");
  let savedAvatarHash = $state<string | null>(null);
  let isDirty = $derived(
    displayName !== savedDisplayName ||
      bio !== savedBio ||
      avatarHash !== savedAvatarHash,
  );

  async function copyNodeId() {
    await copyToClipboard(nodeId);
    copyFeedback = true;
    setTimeout(() => (copyFeedback = false), 1500);
  }

  async function loadAvatarPreview(ticket: string) {
    try {
      const bytes: number[] = await invoke("fetch_blob_bytes", { ticket });
      const data = new Uint8Array(bytes);
      const blob = new Blob([data], { type: detectImageMime(data) });
      avatarPreview = URL.createObjectURL(blob);
    } catch (e) {
      console.error("Failed to load avatar:", e);
    }
  }

  async function init() {
    try {
      nodeId = await invoke("get_node_id");
      const profile: Profile | null = await invoke("get_my_profile");
      if (profile) {
        displayName = profile.display_name;
        bio = profile.bio;
        avatarHash = profile.avatar_hash;
        avatarTicket = profile.avatar_ticket;
        savedDisplayName = profile.display_name;
        savedBio = profile.bio;
        savedAvatarHash = profile.avatar_hash;
        if (profile.avatar_ticket) {
          await loadAvatarPreview(profile.avatar_ticket);
        }
      }
      loading = false;
    } catch {
      setTimeout(init, 500);
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
      avatarHash = result.hash;
      avatarTicket = result.ticket;
      if (avatarPreview) URL.revokeObjectURL(avatarPreview);
      avatarPreview = URL.createObjectURL(file);
    } catch (e) {
      status = `Upload failed: ${e}`;
    }
    uploading = false;
    input.value = "";
  }

  function removeAvatar() {
    avatarHash = null;
    avatarTicket = null;
    if (avatarPreview) {
      URL.revokeObjectURL(avatarPreview);
      avatarPreview = null;
    }
  }

  async function save() {
    saving = true;
    displayName = displayName.trim();
    bio = bio.trim();
    try {
      await invoke("save_my_profile", {
        displayName,
        bio,
        avatarHash,
        avatarTicket,
      });
      savedDisplayName = displayName;
      savedBio = bio;
      savedAvatarHash = avatarHash;
      status = "Saved!";
      setTimeout(() => (status = ""), 2000);
    } catch (e) {
      status = `Error: ${e}`;
    }
    saving = false;
  }

  onMount(() => {
    init();
    return () => {
      if (avatarPreview) URL.revokeObjectURL(avatarPreview);
    };
  });
</script>

{#if loading}
  <div class="loading">
    <div class="spinner"></div>
    <p>Loading...</p>
  </div>
{:else}
  <h2>Your Profile</h2>

  <div class="field">
    <span class="label">Node ID</span>
    <div class="id-row">
      <code>{nodeId}</code>
      <button class="copy-btn" onclick={copyNodeId}>
        {copyFeedback ? "Copied!" : "Copy"}
      </button>
    </div>
  </div>

  <div class="form">
    <div class="field">
      <span class="label">Avatar</span>
      <div class="avatar-row">
        {#if avatarPreview}
          <img src={avatarPreview} alt="Avatar" class="avatar-preview" />
        {:else}
          <div class="avatar-fallback" style="background:{avatarColor(nodeId)}">
            {getInitials(displayName || "You", !displayName)}
          </div>
        {/if}
        <div class="avatar-actions">
          <button
            class="avatar-btn"
            onclick={() => fileInput.click()}
            disabled={uploading}
          >
            {uploading ? "Uploading..." : avatarHash ? "Change" : "Upload"}
          </button>
          {#if avatarHash}
            <button class="avatar-btn remove" onclick={removeAvatar}>
              Remove
            </button>
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
      <span class="label">Display Name</span>
      <input bind:value={displayName} placeholder="Anonymous" />
    </div>

    <div class="field">
      <span class="label">Bio</span>
      <textarea
        bind:value={bio}
        placeholder="Tell the world about yourself..."
        rows="3"
      ></textarea>
    </div>

    <button class="save-btn" onclick={save} disabled={saving || !isDirty}>
      {saving ? "Saving..." : "Save Profile"}
    </button>

    {#if status}
      <p class="status">{status}</p>
    {/if}
  </div>
{/if}

<style>
  h2 {
    color: #a78bfa;
    margin: 0 0 1rem;
  }

  .field {
    margin-bottom: 1rem;
  }

  .label {
    display: block;
    font-size: 0.8rem;
    color: #888;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 0.25rem;
  }

  .id-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  code {
    background: #0f0f23;
    padding: 0.5rem 0.75rem;
    border-radius: 6px;
    font-size: 0.75rem;
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

  .form {
    background: #16213e;
    border: 1px solid #2a2a4a;
    border-radius: 8px;
    padding: 1.25rem;
  }

  .avatar-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .avatar-preview {
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

  input,
  textarea {
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

  input:focus,
  textarea:focus {
    outline: none;
    border-color: #a78bfa;
  }

  .save-btn {
    width: 100%;
    background: #7c3aed;
    color: white;
    border: none;
    border-radius: 6px;
    padding: 0.6rem;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
  }

  .save-btn:hover:not(:disabled) {
    background: #6d28d9;
  }

  .save-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .status {
    text-align: center;
    color: #888;
    font-size: 0.85rem;
    margin-top: 0.75rem;
  }
</style>
