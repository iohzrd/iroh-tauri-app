<script lang="ts">
  import { page } from "$app/state";
  let { children } = $props();
</script>

<div class="app">
  <nav>
    <a href="/" class:active={page.url.pathname === "/"}>Feed</a>
    <a href="/profile" class:active={page.url.pathname === "/profile"}
      >Profile</a
    >
    <a href="/follows" class:active={page.url.pathname === "/follows"}
      >Follows</a
    >
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
