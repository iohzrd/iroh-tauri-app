import { invoke } from "@tauri-apps/api/core";
import type { MediaAttachment } from "$lib/types";

export function createBlobCache() {
  const cache = new Map<string, string>();

  async function getBlobUrl(attachment: MediaAttachment): Promise<string> {
    const cached = cache.get(attachment.hash);
    if (cached) return cached;
    const bytes: number[] = await invoke("fetch_blob_bytes", {
      ticket: attachment.ticket,
    });
    const blob = new Blob([new Uint8Array(bytes)], {
      type: attachment.mime_type,
    });
    const url = URL.createObjectURL(blob);
    cache.set(attachment.hash, url);
    return url;
  }

  async function downloadFile(att: MediaAttachment): Promise<void> {
    const url = await getBlobUrl(att);
    const a = document.createElement("a");
    a.href = url;
    a.download = att.filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
  }

  function revokeAll(): void {
    for (const url of cache.values()) URL.revokeObjectURL(url);
    cache.clear();
  }

  function revokeStale(activeHashes: Set<string>): void {
    for (const [hash, url] of cache) {
      if (!activeHashes.has(hash)) {
        URL.revokeObjectURL(url);
        cache.delete(hash);
      }
    }
  }

  return { getBlobUrl, downloadFile, revokeAll, revokeStale };
}
