<script lang="ts">
  import { onMount } from "svelte";

  interface Props {
    onscanned: (nodeId: string) => void;
    onclose: () => void;
  }

  let { onscanned, onclose }: Props = $props();

  let error = $state<string | null>(null);
  let scanning = $state(true);

  function parseNodeIdFromUrl(url: string): string | null {
    try {
      const parsed = new URL(url);
      if (parsed.protocol !== "iroh-social:") return null;
      if (parsed.hostname !== "user") return null;
      const nodeId = parsed.pathname.slice(1);
      return nodeId || null;
    } catch {
      return null;
    }
  }

  function handleScanResult(text: string) {
    const nodeId = parseNodeIdFromUrl(text);
    if (nodeId) {
      onscanned(nodeId);
    } else {
      error = `Not an invite link: ${text}`;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }

  onMount(() => {
    (async () => {
      try {
        const { scan, Format, checkPermissions, requestPermissions } =
          await import("@tauri-apps/plugin-barcode-scanner");

        let perms = await checkPermissions();
        if (perms !== "granted") {
          perms = await requestPermissions();
        }
        if (perms !== "granted") {
          error = "Camera permission denied";
          scanning = false;
          return;
        }

        const result = await scan({
          formats: [Format.QRCode],
          windowed: true,
        });
        scanning = false;
        handleScanResult(result.content);
      } catch (e) {
        error = `Scan error: ${e}`;
        scanning = false;
      }
    })();
  });
</script>

<svelte:window onkeydown={handleKeydown} />

{#if error}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="modal-overlay" onclick={onclose} role="presentation">
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div
      class="modal scanner-modal"
      onclick={(e) => e.stopPropagation()}
      role="dialog"
      aria-label="Scan error"
    >
      <p class="scanner-label">{error}</p>
      <div class="modal-actions">
        <button class="modal-cancel" onclick={onclose}>Close</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .scanner-modal {
    max-width: 340px;
  }

  .scanner-label {
    margin: 0 0 0.75rem;
    color: #888;
    font-size: 0.85rem;
    text-align: center;
  }
</style>
