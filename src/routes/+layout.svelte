<script lang="ts">
  import { page } from "$app/state";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { onMount } from "svelte";
  import {
    isPermissionGranted,
    requestPermission,
    sendNotification,
  } from "@tauri-apps/plugin-notification";
  import { onOpenUrl } from "@tauri-apps/plugin-deep-link";
  import { goto } from "$app/navigation";
  import type { NodeStatus, Post, StoredMessage } from "$lib/types";

  const ZOOM_KEY = "app-zoom-level";
  const ZOOM_STEP = 0.2;
  const ZOOM_MIN = 0.2;
  const ZOOM_MAX = 10.0;

  let { children } = $props();
  let status = $state<NodeStatus | null>(null);
  let zoomLevel = $state(1.0);
  let unreadDmCount = $state(0);
  let unreadNotificationCount = $state(0);
  let nodeId = $state("");

  async function applyZoom(level: number) {
    zoomLevel = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, level));
    localStorage.setItem(ZOOM_KEY, String(zoomLevel));
    await getCurrentWebview().setZoom(zoomLevel);
  }

  function handleZoomKeys(e: KeyboardEvent) {
    const mod = e.ctrlKey || e.metaKey;
    if (!mod) return;
    if (e.key === "=" || e.key === "+") {
      e.preventDefault();
      applyZoom(zoomLevel + ZOOM_STEP);
    } else if (e.key === "-") {
      e.preventDefault();
      applyZoom(zoomLevel - ZOOM_STEP);
    } else if (e.key === "0") {
      e.preventDefault();
      applyZoom(1.0);
    }
  }

  function handleDeepLink(url: string) {
    try {
      const parsed = new URL(url);
      if (parsed.protocol !== "iroh-social:") return;
      if (parsed.hostname === "profile") {
        const id = parsed.pathname.slice(1);
        if (id) {
          goto(`/profile/${id}`);
        }
      }
    } catch {
      // malformed URL, ignore
    }
  }

  async function pollStatus() {
    try {
      status = await invoke("get_node_status");
    } catch {
      // Node not ready yet
    }
  }

  async function pollUnread() {
    try {
      unreadDmCount = await invoke("get_unread_dm_count");
    } catch {
      // Node not ready yet
    }
  }

  async function pollUnreadNotifications() {
    try {
      unreadNotificationCount = await invoke("get_unread_notification_count");
    } catch {
      // Node not ready yet
    }
  }

  onMount(() => {
    const saved = localStorage.getItem(ZOOM_KEY);
    if (saved) {
      const parsed = parseFloat(saved);
      if (Number.isFinite(parsed)) {
        applyZoom(parsed);
      }
    }

    window.addEventListener("keydown", handleZoomKeys);
    invoke<string>("get_node_id")
      .then((id) => (nodeId = id))
      .catch(() => {});
    pollStatus();
    pollUnread();
    pollUnreadNotifications();
    const statusInterval = setInterval(pollStatus, 10000);
    const unreadInterval = setInterval(pollUnread, 10000);
    const notificationInterval = setInterval(pollUnreadNotifications, 10000);
    const unlisteners: Promise<UnlistenFn>[] = [];

    async function setupNotifications() {
      let permitted = await isPermissionGranted();
      if (!permitted) {
        const result = await requestPermission();
        permitted = result === "granted";
      }
      unlisteners.push(
        listen<{ from: string; message: StoredMessage }>(
          "dm-received",
          async (event) => {
            pollUnread();
            const senderPubkey = event.payload.from;
            const isViewingConversation =
              page.url.pathname === `/messages/${senderPubkey}`;
            if (!isViewingConversation && permitted) {
              let title = senderPubkey.slice(0, 8);
              try {
                const profile = await invoke<{ display_name: string } | null>(
                  "get_remote_profile",
                  { pubkey: senderPubkey },
                );
                if (profile?.display_name) {
                  title = profile.display_name;
                }
              } catch {
                // keep short pubkey as title
              }
              sendNotification({
                title,
                body: event.payload.message.content || "Sent a message",
              });
            }
          },
        ),
      );
      unlisteners.push(
        listen<Post>("mentioned-in-post", async (event) => {
          pollUnreadNotifications();
          const isViewingActivity = page.url.pathname === "/activity";
          if (!isViewingActivity && permitted) {
            const post = event.payload;
            let title = post.author.slice(0, 8);
            try {
              const profile = await invoke<{ display_name: string } | null>(
                "get_remote_profile",
                { pubkey: post.author },
              );
              if (profile?.display_name) {
                title = profile.display_name;
              }
            } catch {
              // keep short pubkey as title
            }
            sendNotification({
              title: `${title} mentioned you`,
              body: post.content.slice(0, 100) || "Mentioned you in a post",
            });
          }
        }),
      );
      unlisteners.push(
        listen("notification-received", () => {
          pollUnreadNotifications();
        }),
      );
    }

    setupNotifications();

    // Deep link handling
    unlisteners.push(
      onOpenUrl((urls) => {
        for (const url of urls) {
          handleDeepLink(url);
        }
      }),
    );
    unlisteners.push(
      listen<string[]>("deep-link-received", (event) => {
        for (const url of event.payload) {
          handleDeepLink(url);
        }
      }),
    );

    return () => {
      window.removeEventListener("keydown", handleZoomKeys);
      clearInterval(statusInterval);
      clearInterval(unreadInterval);
      clearInterval(notificationInterval);
      unlisteners.forEach((p) => p.then((fn) => fn()));
    };
  });
</script>

<div class="app">
  <nav>
    <a href="/" class:active={page.url.pathname === "/"}>Feed</a>
    <a href="/activity" class:active={page.url.pathname === "/activity"}>
      Notifications
      {#if unreadNotificationCount > 0}
        <span class="unread-badge">{unreadNotificationCount}</span>
      {/if}
    </a>
    {#if nodeId}
      <a
        href="/profile/{nodeId}"
        class:active={page.url.pathname === `/profile/${nodeId}`}>Profile</a
      >
    {/if}
    <a
      href="/messages"
      class:active={page.url.pathname.startsWith("/messages")}
    >
      Messages
      {#if unreadDmCount > 0}
        <span class="unread-badge">{unreadDmCount}</span>
      {/if}
    </a>
    <a href="/follows" class:active={page.url.pathname === "/follows"}
      >Follows</a
    >
    {#if status}
      <span
        class="status-indicator"
        title={status.has_relay
          ? `Relay connected | ${status.follow_count} following | ${status.follower_count} follower(s)`
          : "No relay connection"}
      >
        <span
          class="status-dot"
          class:connected={status.has_relay}
          class:disconnected={!status.has_relay}
        ></span>
      </span>
    {/if}
  </nav>
  {#if status && !status.has_relay}
    <div class="relay-banner">
      <span class="relay-banner-dot"></span>
      <span>Relay disconnected -- messages and sync may not work</span>
    </div>
  {/if}
  <main>
    {@render children()}
  </main>
</div>

<style>
  :global(body) {
    margin: 0;
    font-family:
      "Inter",
      -apple-system,
      BlinkMacSystemFont,
      "Segoe UI",
      sans-serif;
    font-size: 15px;
    line-height: 1.5;
    color: #e0e0e0;
    background-color: #1a1a2e;
  }

  .app {
    max-width: 640px;
    margin: 0 auto;
    min-height: 100vh;
    display: flex;
    flex-direction: column;
  }

  nav {
    display: flex;
    gap: 0;
    align-items: center;
    border-bottom: 1px solid #2a2a4a;
    position: sticky;
    top: 0;
    background: #1a1a2e;
    z-index: 10;
    padding-top: env(safe-area-inset-top);
  }

  nav a {
    flex: 1;
    text-align: center;
    padding: 0.7rem 0.25rem;
    color: #777;
    text-decoration: none;
    font-weight: 600;
    font-size: 0.8rem;
    transition:
      color 0.15s,
      border-color 0.15s;
    border-bottom: 2px solid transparent;
    white-space: nowrap;
  }

  nav a:hover {
    color: #c4b5fd;
  }

  nav a.active {
    color: #a78bfa;
    border-bottom-color: #a78bfa;
  }

  .unread-badge {
    background: #7c3aed;
    color: white;
    font-size: 0.55rem;
    font-weight: 700;
    border-radius: 999px;
    min-width: 14px;
    height: 14px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0 3px;
    margin-left: 1px;
    vertical-align: super;
  }

  .status-indicator {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0 0.5rem;
    flex-shrink: 0;
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .status-dot.connected {
    background: #22c55e;
    box-shadow: 0 0 4px #22c55e80;
  }

  .status-dot.disconnected {
    background: #ef4444;
    box-shadow: 0 0 4px #ef444480;
  }

  .relay-banner {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 1rem;
    background: #7f1d1d;
    border-bottom: 1px solid #991b1b;
    color: #fca5a5;
    font-size: 0.8rem;
    font-weight: 500;
  }

  .relay-banner-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #ef4444;
    flex-shrink: 0;
    animation: pulse-dot 2s infinite;
  }

  @keyframes pulse-dot {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.4;
    }
  }

  main {
    padding: 1rem;
    flex: 1;
  }

  :global(.loading) {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 3rem;
    gap: 1rem;
    color: #888;
  }

  :global(.spinner) {
    width: 32px;
    height: 32px;
    border: 3px solid #2a2a4a;
    border-top-color: #a78bfa;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes -global-spin {
    to {
      transform: rotate(360deg);
    }
  }

  :global(.btn-spinner) {
    display: inline-block;
    width: 14px;
    height: 14px;
    border: 2px solid #c4b5fd40;
    border-top-color: #c4b5fd;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    vertical-align: middle;
  }

  :global(button .btn-spinner) {
    border-color: #ffffff40;
    border-top-color: #fff;
  }

  :global(.sentinel) {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.4rem;
    width: 100%;
    min-height: 1px;
    padding: 0.5rem 0;
    color: #c4b5fd;
    font-size: 0.85rem;
  }

  :global(.modal-overlay) {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  :global(.modal) {
    background: #16213e;
    border: 1px solid #2a2a4a;
    border-radius: 10px;
    padding: 1.5rem;
    max-width: 320px;
    width: 90%;
  }

  :global(.modal p) {
    margin: 0 0 1rem;
    text-align: center;
  }

  :global(.modal-actions) {
    display: flex;
    gap: 0.5rem;
  }

  :global(.modal-cancel) {
    flex: 1;
    background: #2a2a4a;
    color: #c4b5fd;
    border: none;
    border-radius: 6px;
    padding: 0.5rem;
    font-size: 0.9rem;
    cursor: pointer;
  }

  :global(.modal-cancel:hover) {
    background: #3a3a5a;
  }

  :global(.modal-confirm) {
    flex: 1;
    background: #dc2626;
    color: white;
    border: none;
    border-radius: 6px;
    padding: 0.5rem;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
  }

  :global(.modal-confirm:hover) {
    background: #b91c1c;
  }

  :global(.modal-confirm.save) {
    background: #7c3aed;
  }

  :global(.modal-confirm.save:hover) {
    background: #6d28d9;
  }

  :global(.toast) {
    position: fixed;
    bottom: 2rem;
    left: 50%;
    transform: translateX(-50%);
    background: #2a2a4a;
    color: #e0e0e0;
    padding: 0.6rem 1.25rem;
    border-radius: 8px;
    font-size: 0.85rem;
    z-index: 200;
    animation: toastIn 0.3s ease-out;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
  }

  :global(.toast.error) {
    border-left: 3px solid #ef4444;
  }

  @keyframes toastIn {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(10px);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
  }
</style>
