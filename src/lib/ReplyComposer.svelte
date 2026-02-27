<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import MentionAutocomplete from "$lib/MentionAutocomplete.svelte";

  let {
    replyToId,
    replyToAuthor,
    nodeId,
    onsubmitted,
    oncancel,
  }: {
    replyToId: string;
    replyToAuthor: string;
    nodeId: string;
    onsubmitted?: () => void;
    oncancel?: () => void;
  } = $props();

  let content = $state("");
  let posting = $state(false);
  let mentionQuery = $state("");
  let mentionActive = $state(false);
  let mentionAutocomplete: MentionAutocomplete;

  async function submit() {
    if (!content.trim() || posting) return;
    posting = true;
    try {
      await invoke("create_post", {
        content: content.trim(),
        media: null,
        replyTo: replyToId,
        replyToAuthor: replyToAuthor,
      });
      content = "";
      onsubmitted?.();
    } catch (e) {
      console.error("Failed to post reply:", e);
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
      ".reply-composer textarea",
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

<div class="reply-composer" style="position: relative;">
  <MentionAutocomplete
    bind:this={mentionAutocomplete}
    query={mentionQuery}
    selfId={nodeId}
    visible={mentionActive}
    onselect={insertMention}
  />
  <textarea
    bind:value={content}
    placeholder="Write a reply..."
    rows="2"
    onkeydown={handleKey}
    oninput={handleMentionInput}
  ></textarea>
  <div class="reply-actions">
    <button class="cancel-btn" onclick={oncancel}>Cancel</button>
    <button
      class="reply-btn"
      onclick={submit}
      disabled={posting || !content.trim()}
    >
      {posting ? "Posting..." : "Reply"}
    </button>
  </div>
</div>

<style>
  .reply-composer {
    margin-top: 0.6rem;
    padding-top: 0.6rem;
    border-top: 1px solid #2a2a4a40;
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

  .reply-actions {
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

  .reply-btn {
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

  .reply-btn:hover:not(:disabled) {
    background: #6d28d9;
  }

  .reply-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
