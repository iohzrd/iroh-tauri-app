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
  <button class="lightbox-close" onclick={onclose} aria-label="Close lightbox">
    &times;
  </button>
  <img {src} {alt} class="lightbox-img" onclick={(e) => e.stopPropagation()} />
</div>

<style>
  .lightbox-overlay {
    position: fixed;
    inset: 0;
    background: var(--overlay-dark);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: var(--z-lightbox);
    cursor: pointer;
  }

  .lightbox-close {
    position: absolute;
    top: 1rem;
    right: 1rem;
    background: none;
    border: none;
    color: var(--text-on-accent);
    font-size: var(--text-3xl);
    cursor: pointer;
    min-width: 44px;
    min-height: 44px;
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1;
  }

  .lightbox-img {
    max-width: 95vw;
    max-height: 95vh;
    object-fit: contain;
    cursor: default;
    border-radius: var(--radius-sm);
  }
</style>
