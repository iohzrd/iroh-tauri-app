import { invoke } from "@tauri-apps/api/core";

const AVATAR_COLORS = [
  "#7c3aed",
  "#2563eb",
  "#059669",
  "#d97706",
  "#dc2626",
  "#db2777",
  "#7c3aed",
  "#0891b2",
];

export function avatarColor(pubkey: string): string {
  let hash = 0;
  for (let i = 0; i < pubkey.length; i++) {
    hash = pubkey.charCodeAt(i) + ((hash << 5) - hash);
  }
  return AVATAR_COLORS[Math.abs(hash) % AVATAR_COLORS.length];
}

export function getInitials(name: string, isSelf = false): string {
  if (!name || isSelf) return "Y";
  const parts = name.trim().split(/\s+/);
  if (parts.length >= 2) return (parts[0][0] + parts[1][0]).toUpperCase();
  return name.slice(0, 2).toUpperCase();
}

export function shortId(id: string): string {
  return id.slice(0, 8) + "..." + id.slice(-4);
}

// Shared display name cache and resolver
const displayNameCache = new Map<string, string>();

export function clearDisplayNameCache() {
  displayNameCache.clear();
}

export function evictDisplayName(pubkey: string) {
  displayNameCache.delete(pubkey);
}

export async function getDisplayName(
  pubkey: string,
  selfId: string,
): Promise<string> {
  if (pubkey === selfId) return "You";
  const cached = displayNameCache.get(pubkey);
  if (cached !== undefined) return cached;
  try {
    const profile = (await invoke("get_remote_profile", { pubkey })) as {
      display_name: string;
      bio: string;
    } | null;
    const name =
      profile && profile.display_name ? profile.display_name : shortId(pubkey);
    displayNameCache.set(pubkey, name);
    return name;
  } catch {
    const name = shortId(pubkey);
    displayNameCache.set(pubkey, name);
    return name;
  }
}

export async function copyToClipboard(text: string): Promise<void> {
  await navigator.clipboard.writeText(text);
}
