import { invoke } from "@tauri-apps/api/core";
import type { Profile } from "$lib/types";

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

// Shared profile cache (name + avatar) and resolver
interface CachedProfile {
  name: string;
  avatarTicket: string | null;
}

const profileCache = new Map<string, CachedProfile>();

export function clearDisplayNameCache() {
  profileCache.clear();
}

export function evictDisplayName(pubkey: string) {
  profileCache.delete(pubkey);
}

export async function getDisplayName(
  pubkey: string,
  selfId: string,
): Promise<string> {
  if (pubkey === selfId) return "You";
  const cached = profileCache.get(pubkey);
  if (cached !== undefined) return cached.name;
  try {
    const profile = (await invoke("get_remote_profile", {
      pubkey,
    })) as Profile | null;
    const name =
      profile && profile.display_name ? profile.display_name : shortId(pubkey);
    profileCache.set(pubkey, {
      name,
      avatarTicket: profile?.avatar_ticket ?? null,
    });
    return name;
  } catch {
    const name = shortId(pubkey);
    profileCache.set(pubkey, { name, avatarTicket: null });
    return name;
  }
}

export function getCachedAvatarTicket(pubkey: string): string | null {
  return profileCache.get(pubkey)?.avatarTicket ?? null;
}

export async function copyToClipboard(text: string): Promise<void> {
  await navigator.clipboard.writeText(text);
}

// Shared helpers extracted from +page.svelte

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

export function linkify(text: string): string {
  const urlPattern = /https?:\/\/[^\s<>"')\]]+/g;
  const parts: string[] = [];
  let lastIndex = 0;
  let match;
  while ((match = urlPattern.exec(text)) !== null) {
    if (match.index > lastIndex) {
      parts.push(escapeHtml(text.slice(lastIndex, match.index)));
    }
    const url = match[0];
    parts.push(
      `<a href="${escapeHtml(url)}" target="_blank" rel="noopener noreferrer">${escapeHtml(url)}</a>`,
    );
    lastIndex = urlPattern.lastIndex;
  }
  if (lastIndex < text.length) {
    parts.push(escapeHtml(text.slice(lastIndex)));
  }
  return parts.join("");
}

export function isImage(mime: string): boolean {
  return mime.startsWith("image/");
}

export function isVideo(mime: string): boolean {
  return mime.startsWith("video/");
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return bytes + " B";
  if (bytes < 1048576) return (bytes / 1024).toFixed(1) + " KB";
  return (bytes / 1048576).toFixed(1) + " MB";
}
