<script lang="ts">
  import { getBlobContext } from "$lib/blobs";
  import type { MediaAttachment } from "$lib/types";
  import { isImage, isVideo, formatSize } from "$lib/utils";

  let {
    media,
    onlightbox,
  }: {
    media: MediaAttachment[];
    onlightbox?: (src: string, alt: string) => void;
  } = $props();

  const { getBlobUrl, downloadFile } = getBlobContext();
</script>

{#if media.length > 0}
  <div class="media-grid" class:grid={media.length > 1}>
    {#each media as att (att.hash)}
      {#if isImage(att.mime_type)}
        {#await getBlobUrl(att)}
          <div class="media-placeholder">Loading...</div>
        {:then url}
          <button
            class="media-img-btn"
            onclick={() => onlightbox?.(url, att.filename)}
          >
            <img src={url} alt={att.filename} class="media-img" />
          </button>
        {:catch}
          <div class="media-placeholder">Failed to load</div>
        {/await}
      {:else if isVideo(att.mime_type)}
        {#await getBlobUrl(att)}
          <div class="media-placeholder">Loading...</div>
        {:then url}
          <video src={url} controls class="media-video">
            <track kind="captions" />
          </video>
        {:catch}
          <div class="media-placeholder">Failed to load</div>
        {/await}
      {:else}
        <button class="media-file" onclick={() => downloadFile(att)}>
          <span>{att.filename}</span>
          <span class="file-size">{formatSize(att.size)}</span>
          <span class="download-label">Download</span>
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .media-grid {
    margin-top: 0.75rem;
  }

  .media-grid.grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 0.5rem;
  }

  .media-img-btn {
    background: none;
    border: none;
    padding: 0;
    cursor: zoom-in;
    display: block;
    width: 100%;
  }

  .media-img {
    width: 100%;
    border-radius: 8px;
    max-height: 400px;
    object-fit: contain;
    background: #0f0f23;
    display: block;
  }

  .media-video {
    width: 100%;
    border-radius: 8px;
    max-height: 400px;
  }

  .media-placeholder {
    background: #0f0f23;
    border-radius: 8px;
    padding: 2rem;
    text-align: center;
    color: #666;
    font-size: 0.8rem;
  }

  .media-file {
    background: #0f0f23;
    border: 1px solid #2a2a4a;
    border-radius: 8px;
    padding: 0.75rem;
    display: flex;
    justify-content: space-between;
    align-items: center;
    color: #c4b5fd;
    font-size: 0.85rem;
    cursor: pointer;
    width: 100%;
    font-family: inherit;
    transition: border-color 0.2s;
  }

  .media-file:hover {
    border-color: #a78bfa;
  }

  .file-size {
    color: #666;
    font-size: 0.75rem;
  }

  .download-label {
    color: #7dd3fc;
    font-size: 0.75rem;
  }
</style>
