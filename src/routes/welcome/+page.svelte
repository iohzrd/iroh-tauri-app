<script lang="ts">
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { copyToClipboard, detectImageMime } from "$lib/utils";

  let step = $state(0);
  let nodeId = $state("");
  let displayName = $state("");
  let bio = $state("");
  let avatarPreview = $state<string | null>(null);
  let avatarHash = $state<string | null>(null);
  let avatarTicket = $state<string | null>(null);
  let saving = $state(false);
  let uploading = $state(false);
  let copyFeedback = $state(false);
  let fileInput = $state<HTMLInputElement>(null!);

  onMount(async () => {
    try {
      nodeId = await invoke("get_node_id");
      const profile = await invoke("get_my_profile");
      if (profile) {
        goto("/");
        return;
      }
    } catch {
      setTimeout(() => location.reload(), 500);
    }
  });

  async function copyNodeId() {
    await copyToClipboard(nodeId);
    copyFeedback = true;
    setTimeout(() => (copyFeedback = false), 1500);
  }

  async function handleAvatarUpload(e: Event) {
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
      const blob = new Blob([new Uint8Array(buffer)], {
        type: detectImageMime(new Uint8Array(buffer)),
      });
      avatarPreview = URL.createObjectURL(blob);
    } catch (e) {
      console.error("Failed to upload avatar:", e);
    }
    uploading = false;
    input.value = "";
  }

  async function saveProfile() {
    if (!displayName.trim()) return;
    saving = true;
    try {
      await invoke("save_my_profile", {
        displayName: displayName.trim(),
        bio: bio.trim(),
        avatarHash,
        avatarTicket,
      });
      goto("/");
    } catch (e) {
      console.error("Failed to save profile:", e);
    }
    saving = false;
  }
</script>

<input
  type="file"
  accept="image/*"
  class="hidden"
  bind:this={fileInput}
  onchange={handleAvatarUpload}
/>

<div class="welcome">
  {#if step === 0}
    <div class="step">
      <h1>Welcome</h1>
      <p class="subtitle">
        A peer-to-peer social network. No servers, no middlemen.
      </p>
      <p class="desc">
        Your identity is a cryptographic key pair stored on your device. You own
        your data.
      </p>
      {#if nodeId}
        <div class="node-id-section">
          <p class="label">Your Node ID</p>
          <button class="node-id" onclick={copyNodeId} title="Copy">
            {nodeId.slice(0, 16)}...{nodeId.slice(-8)}
          </button>
          {#if copyFeedback}
            <span class="copied">Copied!</span>
          {/if}
        </div>
      {/if}
      <button class="primary-btn" onclick={() => (step = 1)}>
        Set Up Profile
      </button>
    </div>
  {:else}
    <div class="step">
      <h2>Create Your Profile</h2>

      <div class="avatar-section">
        <button
          class="avatar-upload"
          onclick={() => fileInput?.click()}
          disabled={uploading}
        >
          {#if avatarPreview}
            <img src={avatarPreview} alt="Avatar" />
          {:else}
            <span class="avatar-placeholder">
              {uploading ? "..." : "+"}
            </span>
          {/if}
        </button>
        <span class="avatar-hint">Add a photo</span>
      </div>

      <label class="field">
        <span class="field-label">Display Name</span>
        <input
          type="text"
          bind:value={displayName}
          placeholder="Your name"
          maxlength="50"
        />
      </label>

      <label class="field">
        <span class="field-label">Bio</span>
        <textarea
          bind:value={bio}
          placeholder="Tell people about yourself (optional)"
          rows="3"
          maxlength="300"
        ></textarea>
      </label>

      <div class="actions">
        <button class="secondary-btn" onclick={() => (step = 0)}>Back</button>
        <button
          class="primary-btn"
          onclick={saveProfile}
          disabled={!displayName.trim() || saving}
        >
          {saving ? "Saving..." : "Get Started"}
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .hidden {
    display: none;
  }

  .welcome {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-height: 70vh;
    padding: 2rem 1rem;
  }

  .step {
    max-width: 360px;
    width: 100%;
    text-align: center;
  }

  h1 {
    font-size: 1.8rem;
    color: #e0e0e0;
    margin: 0 0 0.5rem;
  }

  h2 {
    font-size: 1.3rem;
    color: #e0e0e0;
    margin: 0 0 1.5rem;
  }

  .subtitle {
    color: #a78bfa;
    font-size: 0.95rem;
    margin: 0 0 1rem;
  }

  .desc {
    color: #888;
    font-size: 0.85rem;
    line-height: 1.6;
    margin: 0 0 1.5rem;
  }

  .node-id-section {
    margin-bottom: 2rem;
  }

  .label {
    color: #666;
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin: 0 0 0.3rem;
  }

  .node-id {
    background: #16213e;
    border: 1px solid #2a2a4a;
    border-radius: 6px;
    padding: 0.4rem 0.8rem;
    color: #c4b5fd;
    font-family: monospace;
    font-size: 0.8rem;
    cursor: pointer;
  }

  .node-id:hover {
    background: #1e2a4a;
  }

  .copied {
    display: block;
    color: #22c55e;
    font-size: 0.7rem;
    margin-top: 0.3rem;
  }

  .avatar-section {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 1.5rem;
  }

  .avatar-upload {
    width: 80px;
    height: 80px;
    border-radius: 50%;
    border: 2px dashed #3a3a5a;
    background: #16213e;
    cursor: pointer;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .avatar-upload:hover {
    border-color: #7c3aed;
  }

  .avatar-upload img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .avatar-placeholder {
    color: #666;
    font-size: 1.5rem;
  }

  .avatar-hint {
    color: #666;
    font-size: 0.75rem;
  }

  .field {
    display: block;
    text-align: left;
    margin-bottom: 1rem;
  }

  .field-label {
    display: block;
    color: #888;
    font-size: 0.8rem;
    margin-bottom: 0.3rem;
  }

  .field input,
  .field textarea {
    width: 100%;
    background: #16213e;
    border: 1px solid #2a2a4a;
    border-radius: 6px;
    padding: 0.5rem 0.75rem;
    color: #e0e0e0;
    font-size: 0.9rem;
    font-family: inherit;
    outline: none;
    box-sizing: border-box;
    transition: border-color 0.2s;
  }

  .field input:focus,
  .field textarea:focus {
    border-color: #7c3aed;
  }

  .field textarea {
    resize: vertical;
    min-height: 60px;
  }

  .field input::placeholder,
  .field textarea::placeholder {
    color: #555;
  }

  .actions {
    display: flex;
    gap: 0.75rem;
    margin-top: 1.5rem;
  }

  .primary-btn {
    flex: 1;
    background: #7c3aed;
    color: white;
    border: none;
    border-radius: 8px;
    padding: 0.7rem 1.5rem;
    font-size: 0.95rem;
    font-weight: 600;
    cursor: pointer;
    font-family: inherit;
    transition: background 0.15s;
  }

  .primary-btn:hover:not(:disabled) {
    background: #6d28d9;
  }

  .primary-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .secondary-btn {
    background: none;
    border: 1px solid #3a3a5a;
    color: #888;
    border-radius: 8px;
    padding: 0.7rem 1.5rem;
    font-size: 0.95rem;
    cursor: pointer;
    font-family: inherit;
    transition:
      color 0.15s,
      border-color 0.15s;
  }

  .secondary-btn:hover {
    color: #c4b5fd;
    border-color: #7c3aed;
  }
</style>
