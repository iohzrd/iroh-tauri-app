<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  let nodeId = $state("");

  onMount(async () => {
    try {
      nodeId = await invoke<string>("get_node_id");
    } catch {
      // Node not ready
    }
  });
</script>

<h2>Settings</h2>

<section class="settings-section">
  <h3>Identity</h3>
  <div class="setting-row">
    <span class="setting-label">Node ID</span>
    <code class="setting-value">{nodeId || "..."}</code>
  </div>
</section>

<style>
  h2 {
    margin: 0 0 1.5rem;
    font-size: var(--text-xl);
    color: var(--text-primary);
  }

  h3 {
    margin: 0 0 0.75rem;
    font-size: var(--text-lg);
    color: var(--text-primary);
  }

  .settings-section {
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: 1rem 1.25rem;
  }

  .setting-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .setting-label {
    color: var(--text-secondary);
    font-weight: 500;
    white-space: nowrap;
  }

  .setting-value {
    color: var(--text-primary);
    font-size: var(--text-sm);
    word-break: break-all;
  }
</style>
