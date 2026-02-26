<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { Post } from "$lib/types";
  import { getDisplayName } from "$lib/utils";

  let {
    replyToId,
    nodeId,
  }: {
    replyToId: string;
    nodeId: string;
  } = $props();

  let context = $state<{ author: string; preview: string } | null>(null);

  $effect(() => {
    loadReplyContext(replyToId);
  });

  async function loadReplyContext(parentId: string) {
    try {
      const parent: Post | null = await invoke("get_post", { id: parentId });
      if (parent) {
        const name = await getDisplayName(parent.author, nodeId);
        const preview =
          parent.content.length > 100
            ? parent.content.slice(0, 100) + "..."
            : parent.content;
        context = { author: name, preview };
      }
    } catch {
      // parent not available locally
    }
  }
</script>

{#if context}
  <a href="/post/{replyToId}" class="reply-context-block">
    <span class="reply-icon">{"\u21A9"}</span>
    <span class="reply-author">{context.author}</span>
    {#if context.preview}
      <span class="reply-preview">{context.preview}</span>
    {/if}
  </a>
{:else}
  <a href="/post/{replyToId}" class="reply-context">
    {"\u21A9"} in reply to a post
  </a>
{/if}

<style>
  .reply-context {
    display: block;
    margin-bottom: 0.35rem;
    font-size: 0.75rem;
    color: #666;
    text-decoration: none;
  }

  .reply-context:hover {
    color: #a78bfa;
    text-decoration: underline;
  }

  .reply-context-block {
    display: flex;
    align-items: baseline;
    gap: 0.3rem;
    margin-bottom: 0.5rem;
    padding: 0.35rem 0.6rem;
    background: #0f0f23;
    border-left: 2px solid #3a3a5a;
    border-radius: 0 6px 6px 0;
    font-size: 0.75rem;
    color: #888;
    text-decoration: none;
    overflow: hidden;
  }

  .reply-context-block:hover {
    border-left-color: #a78bfa;
    color: #a78bfa;
  }

  .reply-icon {
    flex-shrink: 0;
    color: #666;
  }

  .reply-author {
    color: #c4b5fd;
    font-weight: 600;
    flex-shrink: 0;
  }

  .reply-preview {
    color: #666;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
