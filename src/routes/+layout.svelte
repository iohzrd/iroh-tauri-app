<script lang="ts">
  import { page } from "$app/state";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { onMount } from "svelte";
  import type { NodeStatus } from "$lib/types";

  const ZOOM_KEY = "app-zoom-level";
  const ZOOM_STEP = 0.2;
  const ZOOM_MIN = 0.2;
  const ZOOM_MAX = 10.0;

  let { children } = $props();
  let status = $state<NodeStatus | null>(null);
  let zoomLevel = 1.0;
  let unreadDmCount = $state(0);

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

  onMount(() => {
    const saved = localStorage.getItem(ZOOM_KEY);
    if (saved) {
      const parsed = parseFloat(saved);
      if (Number.isFinite(parsed)) {
        applyZoom(parsed);
      }
    }

    window.addEventListener("keydown", handleZoomKeys);
    pollStatus();
    pollUnread();
    const statusInterval = setInterval(pollStatus, 10000);
    const unreadInterval = setInterval(pollUnread, 10000);
    const unlisteners: Promise<UnlistenFn>[] = [];
    unlisteners.push(
      listen("dm-received", () => {
        pollUnread();
      }),
    );
    return () => {
      window.removeEventListener("keydown", handleZoomKeys);
      clearInterval(statusInterval);
      clearInterval(unreadInterval);
      unlisteners.forEach((p) => p.then((fn) => fn()));
    };
  });
</script>

<div class="app">
  <nav>
    <a href="/" class:active={page.url.pathname === "/"}>Feed</a>
    <a href="/profile" class:active={page.url.pathname === "/profile"}
      >Profile</a
    >
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
        {#if status.follow_count > 0}
          <span class="peer-count">{status.follow_count}</span>
        {/if}
      </span>
    {/if}
  </nav>
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
  }

  nav a {
    flex: 1;
    text-align: center;
    padding: 0.75rem;
    color: #888;
    text-decoration: none;
    font-weight: 600;
    font-size: 0.9rem;
    transition:
      color 0.2s,
      border-color 0.2s;
    border-bottom: 2px solid transparent;
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
    font-size: 0.6rem;
    font-weight: 700;
    border-radius: 999px;
    min-width: 16px;
    height: 16px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0 4px;
    margin-left: 2px;
    vertical-align: super;
  }

  .status-indicator {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0 0.75rem;
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

  .peer-count {
    font-size: 0.7rem;
    color: #666;
  }

  main {
    padding: 1rem 1rem 2rem;
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

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
