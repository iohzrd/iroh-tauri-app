<script lang="ts">
  import QR from "@svelte-put/qr/svg/QR.svelte";
  import { copyToClipboard } from "$lib/utils";

  interface Props {
    nodeId: string;
    onclose: () => void;
  }

  let { nodeId, onclose }: Props = $props();
  let copyFeedback = $state(false);

  let deepLinkUrl = $derived(`iroh-social://user/${nodeId}`);

  async function copyLink() {
    await copyToClipboard(deepLinkUrl);
    copyFeedback = true;
    setTimeout(() => (copyFeedback = false), 1500);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="modal-overlay" onclick={onclose} role="presentation">
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div
    class="modal qr-modal"
    onclick={(e) => e.stopPropagation()}
    role="dialog"
    aria-label="QR code"
  >
    <p class="qr-label">Scan to follow</p>
    <div class="qr-wrapper">
      <QR
        data={deepLinkUrl}
        moduleFill="#000000"
        anchorOuterFill="#000000"
        anchorInnerFill="#000000"
      />
    </div>
    <div class="qr-url-row">
      <code class="qr-url">{deepLinkUrl}</code>
      <button class="copy-link-btn" onclick={copyLink}>
        {copyFeedback ? "Copied!" : "Copy"}
      </button>
    </div>
    <div class="modal-actions">
      <button class="modal-cancel" onclick={onclose}>Close</button>
    </div>
  </div>
</div>

<style>
  .qr-modal {
    max-width: 300px;
    text-align: center;
  }

  .qr-label {
    color: #888;
    font-size: 0.85rem;
    margin: 0 0 1rem;
  }

  .qr-wrapper {
    background: #ffffff;
    border-radius: 8px;
    padding: 0.75rem;
    display: inline-block;
    margin-bottom: 0.75rem;
    line-height: 0;
  }

  .qr-wrapper :global(svg) {
    width: 200px;
    height: 200px;
  }

  .qr-url-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin-bottom: 1rem;
  }

  .qr-url {
    flex: 1;
    background: #0f0f23;
    padding: 0.4rem 0.6rem;
    border-radius: 5px;
    font-size: 0.6rem;
    color: #7dd3fc;
    word-break: break-all;
    text-align: left;
  }

  .copy-link-btn {
    background: #2a2a4a;
    color: #c4b5fd;
    border: none;
    border-radius: 4px;
    padding: 0.4rem 0.6rem;
    font-size: 0.7rem;
    cursor: pointer;
    white-space: nowrap;
    min-width: 48px;
    text-align: center;
  }

  .copy-link-btn:hover {
    background: #3a3a5a;
  }
</style>
