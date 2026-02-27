<script lang="ts">
  import { onMount } from "svelte";
  import { hapticNotification } from "$lib/haptics";

  interface Props {
    onscanned: (nodeId: string) => void;
    onclose: () => void;
  }

  let { onscanned, onclose }: Props = $props();

  let error = $state<string | null>(null);
  let scanning = $state(true);
  let cancelFn: (() => Promise<void>) | null = null;

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
      hapticNotification("success");
      onscanned(nodeId);
    } else {
      error = `Not an invite link: ${text}`;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }

  async function stopScanning() {
    if (cancelFn) {
      try {
        await cancelFn();
      } catch {
        // ignore cancel errors
      }
      cancelFn = null;
    }
    document.documentElement.style.background = "";
    document.body.style.background = "";
  }

  onMount(() => {
    (async () => {
      try {
        const { scan, cancel, Format, checkPermissions, requestPermissions } =
          await import("@tauri-apps/plugin-barcode-scanner");

        cancelFn = cancel;

        let perms = await checkPermissions();
        if (perms !== "granted") {
          perms = await requestPermissions();
        }
        if (perms !== "granted") {
          error = "Camera permission denied";
          scanning = false;
          return;
        }

        // Make webview background transparent so native camera preview is visible
        document.documentElement.style.background = "transparent";
        document.body.style.background = "transparent";

        const result = await scan({
          formats: [Format.QRCode],
          windowed: true,
        });
        scanning = false;
        await stopScanning();
        handleScanResult(result.content);
      } catch (e) {
        error = `Scan error: ${e}`;
        scanning = false;
        await stopScanning();
      }
    })();

    return () => {
      stopScanning();
    };
  });
</script>

<svelte:window onkeydown={handleKeydown} />

{#if scanning}
  <div class="scanner-overlay">
    <div class="scanner-top"></div>
    <div class="scanner-middle">
      <div class="scanner-side"></div>
      <div class="scanner-viewfinder"></div>
      <div class="scanner-side"></div>
    </div>
    <div class="scanner-bottom">
      <p class="scanner-hint">Point camera at a QR code</p>
      <button class="scanner-close" onclick={onclose}>Cancel</button>
    </div>
  </div>
{/if}

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
  .scanner-overlay {
    position: fixed;
    inset: 0;
    display: flex;
    flex-direction: column;
    z-index: 200;
  }

  .scanner-top {
    flex: 1;
    background: rgba(0, 0, 0, 0.7);
  }

  .scanner-middle {
    display: flex;
  }

  .scanner-side {
    flex: 1;
    background: rgba(0, 0, 0, 0.7);
  }

  .scanner-viewfinder {
    width: 250px;
    height: 250px;
    border: 3px solid #a78bfa;
    border-radius: 12px;
    flex-shrink: 0;
  }

  .scanner-bottom {
    flex: 1;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    flex-direction: column;
    align-items: center;
    padding-top: 1rem;
  }

  .scanner-hint {
    color: white;
    font-size: 0.9rem;
    margin: 0;
    text-shadow: 0 1px 4px rgba(0, 0, 0, 0.8);
  }

  .scanner-close {
    margin-top: 1.5rem;
    background: rgba(0, 0, 0, 0.6);
    color: white;
    border: 1px solid rgba(255, 255, 255, 0.3);
    border-radius: 8px;
    padding: 0.6rem 2rem;
    font-size: 1rem;
    cursor: pointer;
  }

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
