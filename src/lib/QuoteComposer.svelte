<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { Post } from "$lib/types";
  import { shortId, getDisplayName } from "$lib/utils";
  import MentionAutocomplete from "$lib/MentionAutocomplete.svelte";

  let {
    quotedPost,
    nodeId,
    onsubmitted,
    oncancel,
  }: {
    quotedPost: Post;
    nodeId: string;
    onsubmitted?: () => void;
    oncancel?: () => void;
  } = $props();

  let content = $state("");
  let posting = $state(false);
  let mentionQuery = $state("");
  let mentionActive = $state(false);
  let mentionAutocomplete: MentionAutocomplete;

  let preview = $derived(
    quotedPost.content.length > 120
      ? quotedPost.content.slice(0, 120) + "..."
      : quotedPost.content,
  );

  async function submit() {
    if (posting) return;
    posting = true;
    try {
      await invoke("create_post", {
        content: content.trim(),
        media: null,
        replyTo: null,
        replyToAuthor: null,
        quoteOf: quotedPost.id,
        quoteOfAuthor: quotedPost.author,
      });
      content = "";
      onsubmitted?.();
    } catch (e) {
      console.error("Failed to post quote:", e);
    }
    posting = false;
  }

  function handleMentionInput(e: Event) {
    const textarea = e.target as HTMLTextAreaElement;
    const cursorPos = textarea.selectionStart;
    const textBeforeCursor = textarea.value.slice(0, cursorPos);
    const match = textBeforeCursor.match(/@(\w*)$/);
    if (match) {
      mentionActive = true;
      mentionQuery = match[1];
    } else {
      mentionActive = false;
      mentionQuery = "";
    }
  }

  function insertMention(pubkey: string) {
    const textarea = document.querySelector(
      ".quote-composer textarea",
    ) as HTMLTextAreaElement;
    const cursorPos = textarea.selectionStart;
    const textBeforeCursor = content.slice(0, cursorPos);
    const textAfterCursor = content.slice(cursorPos);
    const match = textBeforeCursor.match(/@(\w*)$/);
    if (match) {
      const beforeMention = textBeforeCursor.slice(0, match.index);
      content = `${beforeMention}@${pubkey} ${textAfterCursor}`;
    }
    mentionActive = false;
    mentionQuery = "";
    textarea.focus();
  }

  function handleKey(e: KeyboardEvent) {
    if (mentionAutocomplete?.handleKey(e)) return;
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    } else if (e.key === "Escape") {
      oncancel?.();
    }
  }
</script>

<div class="quote-composer" style="position: relative;">
  <div class="quoted-preview">
    {#await getDisplayName(quotedPost.author, nodeId)}
      <span class="quote-author">{shortId(quotedPost.author)}</span>
    {:then name}
      <span class="quote-author">{name}</span>
    {/await}
    {#if preview}
      <span class="quote-text">{preview}</span>
    {:else}
      <span class="quote-text empty">[no text]</span>
    {/if}
  </div>
  <MentionAutocomplete
    bind:this={mentionAutocomplete}
    query={mentionQuery}
    selfId={nodeId}
    visible={mentionActive}
    onselect={insertMention}
  />
  <textarea
    bind:value={content}
    placeholder="Add your commentary (optional)..."
    rows="2"
    onkeydown={handleKey}
    oninput={handleMentionInput}
  ></textarea>
  <div class="quote-actions">
    <button class="cancel-btn" onclick={oncancel}>Cancel</button>
    <button class="quote-btn" onclick={submit} disabled={posting}>
      {posting ? "Posting..." : "Quote"}
    </button>
  </div>
</div>

<style>
  .quote-composer {
    margin-top: 0.6rem;
    padding-top: 0.6rem;
    border-top: 1px solid #2a2a4a40;
  }

  .quoted-preview {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    padding: 0.5rem 0.7rem;
    margin-bottom: 0.5rem;
    background: #0f0f23;
    border-left: 2px solid #7c3aed;
    border-radius: 0 6px 6px 0;
    font-size: 0.8rem;
  }

  .quote-author {
    color: #c4b5fd;
    font-weight: 600;
    font-size: 0.75rem;
  }

  .quote-text {
    color: #888;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .quote-text.empty {
    font-style: italic;
    color: #555;
  }

  textarea {
    width: 100%;
    background: #0f0f23;
    border: 1px solid #2a2a4a;
    border-radius: 8px;
    padding: 0.6rem 0.75rem;
    color: #e0e0e0;
    font-family: inherit;
    font-size: 0.85rem;
    resize: vertical;
    box-sizing: border-box;
    transition: border-color 0.2s;
  }

  textarea:focus {
    outline: none;
    border-color: #a78bfa;
  }

  .quote-actions {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.4rem;
    justify-content: flex-end;
  }

  .cancel-btn {
    background: #2a2a4a;
    color: #888;
    border: none;
    border-radius: 6px;
    padding: 0.35rem 0.85rem;
    font-size: 0.8rem;
    cursor: pointer;
    font-family: inherit;
    transition:
      color 0.15s,
      background 0.15s;
  }

  .cancel-btn:hover {
    color: #c4b5fd;
    background: #3a3a5a;
  }

  .quote-btn {
    background: #7c3aed;
    color: white;
    border: none;
    border-radius: 6px;
    padding: 0.35rem 0.85rem;
    font-size: 0.8rem;
    font-weight: 600;
    cursor: pointer;
    font-family: inherit;
    transition: background 0.15s;
  }

  .quote-btn:hover:not(:disabled) {
    background: #6d28d9;
  }

  .quote-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
