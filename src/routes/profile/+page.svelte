<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import type { Profile } from "$lib/types";
  import { copyToClipboard } from "$lib/utils";

  let nodeId = $state("");
  let displayName = $state("");
  let bio = $state("");
  let loading = $state(true);
  let saving = $state(false);
  let status = $state("");
  let copyFeedback = $state(false);

  async function copyNodeId() {
    await copyToClipboard(nodeId);
    copyFeedback = true;
    setTimeout(() => (copyFeedback = false), 1500);
  }

  async function init() {
    try {
      nodeId = await invoke("get_node_id");
      const profile: Profile | null = await invoke("get_my_profile");
      if (profile) {
        displayName = profile.display_name;
        bio = profile.bio;
      }
      loading = false;
    } catch {
      setTimeout(init, 500);
    }
  }

  async function save() {
    saving = true;
    try {
      await invoke("save_my_profile", { displayName, bio });
      status = "Saved!";
      setTimeout(() => (status = ""), 2000);
    } catch (e) {
      status = `Error: ${e}`;
    }
    saving = false;
  }

  onMount(() => {
    init();
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

    <button class="save-btn" onclick={save} disabled={saving}>
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
