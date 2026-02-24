<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { avatarColor, getInitials, detectImageMime } from "$lib/utils";

  interface Props {
    pubkey: string;
    name: string;
    isSelf?: boolean;
    ticket?: string | null;
    size?: number;
  }

  let {
    pubkey,
    name,
    isSelf = false,
    ticket = null,
    size = 32,
  }: Props = $props();

  let imgUrl = $state<string | null>(null);
  let loadedTicket = $state<string | null>(null);

  const avatarBlobCache = new Map<string, string>();

  $effect(() => {
    if (!ticket || ticket === loadedTicket) return;
    const cached = avatarBlobCache.get(ticket);
    if (cached) {
      imgUrl = cached;
      loadedTicket = ticket;
      return;
    }
    const currentTicket = ticket;
    invoke("fetch_blob_bytes", { ticket: currentTicket })
      .then((bytes: unknown) => {
        const data = new Uint8Array(bytes as number[]);
        const blob = new Blob([data], {
          type: detectImageMime(data),
        });
        const url = URL.createObjectURL(blob);
        avatarBlobCache.set(currentTicket, url);
        imgUrl = url;
        loadedTicket = currentTicket;
      })
      .catch(() => {
        loadedTicket = currentTicket;
      });
  });
</script>

{#if imgUrl}
  <img
    src={imgUrl}
    alt={name}
    class="avatar-img"
    style="width:{size}px;height:{size}px"
  />
{:else}
  <div
    class="avatar"
    style="background:{avatarColor(
      pubkey,
    )};width:{size}px;height:{size}px;font-size:{size * 0.35}px"
  >
    {getInitials(name, isSelf)}
  </div>
{/if}

<style>
  .avatar-img {
    border-radius: 50%;
    object-fit: cover;
    flex-shrink: 0;
  }

  .avatar {
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 700;
    color: white;
    flex-shrink: 0;
    text-transform: uppercase;
  }
</style>
