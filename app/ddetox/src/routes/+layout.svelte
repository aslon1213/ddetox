<script lang="ts">
  import NavBar from "$lib/components/NavBar.svelte";
  let { children } = $props();
</script>

<NavBar />
<main class="app">
  {@render children()}
</main>

<style>
  :global(:root) {
    color-scheme: dark;
    font-family:
      Inter,
      -apple-system,
      BlinkMacSystemFont,
      "Segoe UI",
      sans-serif;
    color: #e6e6e8;
    background: #0e0e10;
  }
  :global(body) {
    margin: 0;
  }
  :global(input),
  :global(select),
  :global(button) {
    font: inherit;
    border-radius: 6px;
    border: 1px solid #2a2a2e;
    padding: 0.45rem 0.7rem;
    background: #1c1c1f;
    color: #e6e6e8;
  }
  :global(button) {
    cursor: pointer;
    background: #232327;
  }
  :global(button:hover:not(:disabled)) {
    border-color: #3a3a40;
  }
  :global(button:disabled) {
    cursor: not-allowed;
    opacity: 0.5;
  }
  :global(input:focus),
  :global(select:focus),
  :global(button:focus-visible) {
    outline: 2px solid #5b8def;
    outline-offset: 1px;
  }
  :global(h1) {
    font-size: 1.5rem;
    margin: 0;
  }
  .app {
    max-width: 820px;
    margin: 0 auto;
    padding: 1.5rem 1.5rem 3rem;
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
  }
</style>
