<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import NavBar from "$lib/components/NavBar.svelte";
  let { children } = $props();

  // The tray "Statistics…" item asks the webview to navigate. Dynamically import
  // the Tauri event API so this is a no-op in a plain browser (dev) where it's
  // unavailable.
  onMount(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    import("@tauri-apps/api/event")
      .then(({ listen }) => listen<string>("navigate", (e) => goto(e.payload)))
      .then((un) => {
        if (disposed) un();
        else unlisten = un;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  });
</script>

<NavBar />
<main class="app">
  {@render children()}
</main>

<style>
  :global(:root) {
    color-scheme: dark;

    /* Surfaces & lines */
    --bg: #0b0c10;
    --surface: #15161d;
    --surface-2: #1b1d26;
    --surface-3: #232634;
    --border: #262a36;
    --border-strong: #353b4d;

    /* Text */
    --text: #e8e9ef;
    --text-dim: #9aa0b2;
    --text-faint: #6b7183;

    /* Accent & status */
    --accent: #5b8def;
    --accent-2: #6366f1;
    --accent-grad: linear-gradient(135deg, #3b82f6, #6366f1);
    --accent-soft: #1a294c;
    --accent-soft-text: #cfe0ff;
    --good: #36d27a;
    --good-soft: #0f2a1c;
    --good-text: #9be7b4;
    --warn: #e6a23c;
    --bad: #ff5c6a;
    --bad-soft: #2a1216;
    --bad-text: #ffb4bb;

    /* Shape */
    --radius: 12px;
    --radius-sm: 8px;
    --shadow: 0 12px 34px rgba(0, 0, 0, 0.36);

    font-family:
      Inter,
      -apple-system,
      BlinkMacSystemFont,
      "Segoe UI",
      sans-serif;
    color: var(--text);
    background: var(--bg);
  }
  :global(body) {
    margin: 0;
    min-height: 100vh;
    background:
      radial-gradient(900px 480px at 50% -220px, rgba(91, 141, 239, 0.14) 0%, transparent 70%),
      var(--bg);
  }
  :global(input),
  :global(select),
  :global(button) {
    font: inherit;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    padding: 0.5rem 0.75rem;
    background: var(--surface-2);
    color: var(--text);
    transition:
      border-color 0.12s ease,
      background 0.12s ease,
      box-shadow 0.12s ease;
  }
  :global(input::placeholder) {
    color: var(--text-faint);
  }
  :global(button) {
    cursor: pointer;
    background: var(--surface-3);
  }
  :global(button:hover:not(:disabled)) {
    border-color: var(--border-strong);
  }
  :global(button:disabled) {
    cursor: not-allowed;
    opacity: 0.5;
  }
  :global(input:focus),
  :global(select:focus),
  :global(button:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  /* Shared button intents, usable from any component. */
  :global(button.primary) {
    background: var(--accent-grad);
    border: none;
    color: #fff;
    font-weight: 600;
    box-shadow: 0 6px 16px rgba(59, 130, 246, 0.28);
  }
  :global(button.primary:hover:not(:disabled)) {
    filter: brightness(1.06);
  }
  :global(button.danger) {
    background: #5e1f24;
    border-color: #7d2a30;
    color: var(--bad-text);
    font-weight: 600;
  }
  :global(h1) {
    font-size: 1.5rem;
    margin: 0;
    letter-spacing: -0.01em;
  }
  :global(.card) {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 1.15rem 1.25rem;
    box-shadow: var(--shadow);
  }
  .app {
    max-width: 860px;
    margin: 0 auto;
    padding: 1.5rem 1.5rem 3rem;
    display: flex;
    flex-direction: column;
    gap: 1.35rem;
  }
</style>
