<script lang="ts">
  interface Props {
    src: string;
    alt: string;
    onclose: () => void;
  }

  let { src, alt, onclose }: Props = $props();

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="lightbox-overlay" onclick={onclose} role="presentation">
  <img {src} {alt} class="lightbox-img" onclick={(e) => e.stopPropagation()} />
</div>

<style>
  .lightbox-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.92);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 300;
    cursor: pointer;
  }

  .lightbox-img {
    max-width: 95vw;
    max-height: 95vh;
    object-fit: contain;
    cursor: default;
    border-radius: 4px;
  }
</style>
