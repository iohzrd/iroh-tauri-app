<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let {
    replyToId,
    replyToAuthor,
    onsubmitted,
    oncancel,
  }: {
    replyToId: string;
    replyToAuthor: string;
    onsubmitted?: () => void;
    oncancel?: () => void;
  } = $props();

  let content = $state("");
  let posting = $state(false);

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

  function handleKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    } else if (e.key === "Escape") {
      oncancel?.();
    }
  }
</script>

<div class="reply-composer">
  <textarea
    bind:value={content}
    placeholder="Write a reply..."
    rows="2"
    onkeydown={handleKey}
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
    margin-top: 0.5rem;
    padding-top: 0.5rem;
    border-top: 1px solid #2a2a4a;
  }

  textarea {
    width: 100%;
    background: #0f0f23;
    border: 1px solid #2a2a4a;
    border-radius: 6px;
    padding: 0.5rem;
    color: #e0e0e0;
    font-family: inherit;
    font-size: 0.85rem;
    resize: vertical;
    box-sizing: border-box;
  }

  textarea:focus {
    outline: none;
    border-color: #a78bfa;
  }

  .reply-actions {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.35rem;
    justify-content: flex-end;
  }

  .cancel-btn {
    background: #2a2a4a;
    color: #888;
    border: none;
    border-radius: 4px;
    padding: 0.3rem 0.75rem;
    font-size: 0.8rem;
    cursor: pointer;
    font-family: inherit;
  }

  .cancel-btn:hover {
    color: #c4b5fd;
    background: #3a3a5a;
  }

  .reply-btn {
    background: #7c3aed;
    color: white;
    border: none;
    border-radius: 4px;
    padding: 0.3rem 0.75rem;
    font-size: 0.8rem;
    font-weight: 600;
    cursor: pointer;
    font-family: inherit;
  }

  .reply-btn:hover:not(:disabled) {
    background: #6d28d9;
  }

  .reply-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
