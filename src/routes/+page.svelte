<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let nodeInfo = $state<{ node_id: string; addrs: string } | null>(null);
  let loading = $state(true);

  // Blob state
  let blobContent = $state("");
  let blobResult = $state<{ hash: string; ticket: string } | null>(null);

  // Fetch blob state
  let fetchTicket = $state("");
  let fetchedContent = $state("");

  // Echo state
  let targetNodeId = $state("");
  let echoMessage = $state("");
  let echoResponse = $state("");

  // Status
  let statusMsg = $state("");

  async function loadNodeInfo() {
    try {
      nodeInfo = await invoke("get_node_info");
      loading = false;
    } catch (e) {
      // Node may still be starting up, retry
      setTimeout(loadNodeInfo, 500);
    }
  }

  async function addBlob() {
    if (!blobContent.trim()) return;
    statusMsg = "Adding blob...";
    try {
      blobResult = await invoke("add_blob", { content: blobContent });
      statusMsg = "Blob added!";
    } catch (e) {
      statusMsg = `Error: ${e}`;
    }
  }

  async function fetchBlob() {
    if (!fetchTicket.trim()) return;
    statusMsg = "Fetching blob...";
    try {
      fetchedContent = await invoke("fetch_blob", { ticket: fetchTicket });
      statusMsg = "Blob fetched!";
    } catch (e) {
      statusMsg = `Error: ${e}`;
    }
  }

  async function sendEcho() {
    if (!targetNodeId.trim() || !echoMessage.trim()) return;
    statusMsg = "Sending message...";
    try {
      echoResponse = await invoke("send_message", {
        nodeId: targetNodeId,
        message: echoMessage,
      });
      statusMsg = "Message sent!";
    } catch (e) {
      statusMsg = `Error: ${e}`;
    }
  }

  $effect(() => {
    loadNodeInfo();
  });
</script>

<main>
  <h1>Iroh P2P</h1>

  {#if loading}
    <p class="status">Starting iroh node...</p>
  {:else if nodeInfo}
    <section class="card">
      <h2>Node Info</h2>
      <div class="field">
        <label>Node ID</label>
        <code class="selectable">{nodeInfo.node_id}</code>
      </div>
    </section>
  {/if}

  <section class="card">
    <h2>Share a Blob</h2>
    <div class="input-group">
      <textarea
        bind:value={blobContent}
        placeholder="Enter content to share..."
        rows="3"
      ></textarea>
      <button onclick={addBlob} disabled={loading}>Add Blob</button>
    </div>
    {#if blobResult}
      <div class="result">
        <div class="field">
          <label>Hash</label>
          <code>{blobResult.hash}</code>
        </div>
        <div class="field">
          <label>Ticket (share this)</label>
          <code class="selectable ticket">{blobResult.ticket}</code>
        </div>
      </div>
    {/if}
  </section>

  <section class="card">
    <h2>Fetch a Blob</h2>
    <div class="input-group">
      <input
        bind:value={fetchTicket}
        placeholder="Paste a blob ticket..."
      />
      <button onclick={fetchBlob} disabled={loading}>Fetch</button>
    </div>
    {#if fetchedContent}
      <div class="result">
        <label>Content</label>
        <pre>{fetchedContent}</pre>
      </div>
    {/if}
  </section>

  <section class="card">
    <h2>Echo Message</h2>
    <div class="input-group">
      <input
        bind:value={targetNodeId}
        placeholder="Target Node ID..."
      />
      <input
        bind:value={echoMessage}
        placeholder="Message..."
      />
      <button onclick={sendEcho} disabled={loading}>Send</button>
    </div>
    {#if echoResponse}
      <div class="result">
        <label>Response</label>
        <pre>{echoResponse}</pre>
      </div>
    {/if}
  </section>

  {#if statusMsg}
    <p class="status">{statusMsg}</p>
  {/if}
</main>

<style>
  :root {
    font-family: "Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    font-size: 15px;
    line-height: 1.5;
    color: #e0e0e0;
    background-color: #1a1a2e;
  }

  main {
    max-width: 640px;
    margin: 0 auto;
    padding: 2rem 1.5rem;
  }

  h1 {
    text-align: center;
    font-size: 1.8rem;
    margin-bottom: 1.5rem;
    color: #a78bfa;
  }

  h2 {
    font-size: 1.1rem;
    margin: 0 0 0.75rem 0;
    color: #c4b5fd;
  }

  .card {
    background: #16213e;
    border: 1px solid #2a2a4a;
    border-radius: 10px;
    padding: 1.25rem;
    margin-bottom: 1rem;
  }

  .field {
    margin-bottom: 0.5rem;
  }

  .field label,
  .result label {
    display: block;
    font-size: 0.8rem;
    color: #888;
    margin-bottom: 0.25rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  code {
    display: block;
    background: #0f0f23;
    padding: 0.5rem 0.75rem;
    border-radius: 6px;
    font-size: 0.85rem;
    word-break: break-all;
    color: #7dd3fc;
  }

  .selectable {
    user-select: all;
    cursor: pointer;
  }

  .ticket {
    font-size: 0.7rem;
    max-height: 80px;
    overflow-y: auto;
  }

  .input-group {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  input,
  textarea {
    background: #0f0f23;
    border: 1px solid #2a2a4a;
    border-radius: 6px;
    padding: 0.6rem 0.75rem;
    color: #e0e0e0;
    font-size: 0.9rem;
    font-family: inherit;
    resize: vertical;
  }

  input:focus,
  textarea:focus {
    outline: none;
    border-color: #a78bfa;
  }

  button {
    background: #7c3aed;
    color: white;
    border: none;
    border-radius: 6px;
    padding: 0.6rem 1.2rem;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.2s;
  }

  button:hover:not(:disabled) {
    background: #6d28d9;
  }

  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .result {
    margin-top: 0.75rem;
    padding-top: 0.75rem;
    border-top: 1px solid #2a2a4a;
  }

  pre {
    background: #0f0f23;
    padding: 0.75rem;
    border-radius: 6px;
    font-size: 0.85rem;
    white-space: pre-wrap;
    word-break: break-all;
    color: #7dd3fc;
    margin: 0.25rem 0 0 0;
  }

  .status {
    text-align: center;
    color: #888;
    font-size: 0.85rem;
    margin-top: 1rem;
  }
</style>
