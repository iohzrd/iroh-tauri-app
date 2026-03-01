<script lang="ts">
  import "../app.css";
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
    <a
      href="/settings"
      class="settings-link"
      class:active={page.url.pathname === "/settings"}
      title="Settings"
    >
      &#x2699;
    </a>
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
  .app {
    max-width: 640px;
    margin: 0 auto;
    min-height: 100vh;
    display: flex;
    flex-direction: column;
  }

  nav {
    display: flex;
    align-items: center;
    border-bottom: 1px solid var(--border);
    position: sticky;
    top: 0;
    background: var(--bg-base);
    z-index: var(--z-nav);
    padding-top: env(safe-area-inset-top);
  }

  nav a {
    flex: 1;
    text-align: center;
    padding: 0.7rem 0.25rem;
    color: var(--text-muted);
    text-decoration: none;
    font-weight: 600;
    font-size: var(--text-base);
    transition:
      color var(--transition-fast),
      border-color var(--transition-fast);
    border-bottom: 2px solid transparent;
    white-space: nowrap;
  }

  nav a:hover {
    color: var(--accent-light);
  }

  nav a.active {
    color: var(--accent-medium);
    border-bottom-color: var(--accent-medium);
  }

  .unread-badge {
    font-size: 0.55rem;
    min-width: 14px;
    height: 14px;
    padding: 0 3px;
    margin-left: 1px;
    vertical-align: super;
  }

  .settings-link {
    flex: 0 0 auto;
    padding: 0.5rem;
    font-size: 1.2rem;
    min-width: 36px;
    min-height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-bottom: 2px solid transparent;
  }

  .status-indicator {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0.5rem;
    flex-shrink: 0;
    min-width: 36px;
    min-height: 36px;
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .status-dot.connected {
    background: var(--color-success);
    box-shadow: 0 0 4px var(--glow-success);
  }

  .status-dot.disconnected {
    background: var(--color-error);
    box-shadow: 0 0 4px var(--glow-error);
  }

  .relay-banner {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 1rem;
    background: var(--danger-bg);
    border-bottom: 1px solid var(--danger-border);
    color: var(--danger-text);
    font-size: var(--text-base);
    font-weight: 500;
  }

  .relay-banner-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--color-error);
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
    padding: 1rem 1.5rem;
    padding-bottom: calc(1rem + env(safe-area-inset-bottom));
    flex: 1;
  }
</style>
