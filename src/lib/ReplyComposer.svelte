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

<div class="reply-composer">
  <MentionAutocomplete
    bind:this={mentionAutocomplete}
    query={mentionQuery}
    selfId={nodeId}
    visible={mentionActive}
    onselect={insertMention}
  />
  <textarea
    class="textarea-base"
    bind:value={content}
    placeholder="Write a reply..."
    rows="2"
    onkeydown={handleKey}
    oninput={handleMentionInput}
  ></textarea>
  <div class="reply-actions">
    <button class="btn-cancel" onclick={oncancel}>Cancel</button>
    <button
      class="btn-accent reply-btn"
      onclick={submit}
      disabled={posting || !content.trim()}
    >
      {posting ? "Posting..." : "Reply"}
    </button>
  </div>
</div>

<style>
  .reply-composer {
    position: relative;
    margin-top: 0.6rem;
    padding-top: 0.6rem;
    border-top: 1px solid var(--border-faint);
  }

  .reply-actions {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.4rem;
    justify-content: flex-end;
  }

  .reply-btn {
    padding: 0.35rem 0.85rem;
    font-size: 0.8rem;
  }
</style>
