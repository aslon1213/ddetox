<script lang="ts">
  import { page } from "$app/stores";
  import BrandMark from "$lib/components/BrandMark.svelte";

  const links = [
    { href: "/", label: "Status" },
    { href: "/stats", label: "Statistics" },
    { href: "/library", label: "Library" },
    { href: "/sessions", label: "Sessions" },
    { href: "/calendar", label: "Calendar" },
  ];

  let path = $derived($page.url.pathname);
</script>

<nav class="nav">
  <a class="brand" href="/" aria-label="Blocker — Status">
    <BrandMark size={26} />
    <span class="wordmark">Blocker</span>
  </a>
  <div class="links">
    {#each links as link (link.href)}
      <a
        href={link.href}
        class:active={link.href === "/" ? path === "/" : path.startsWith(link.href)}
      >
        {link.label}
      </a>
    {/each}
  </div>
</nav>

<style>
  .nav {
    display: flex;
    align-items: center;
    gap: 1.5rem;
    padding: 0.6rem 1.25rem;
    border-bottom: 1px solid var(--border);
    background: rgba(13, 14, 19, 0.8);
    backdrop-filter: blur(10px);
    position: sticky;
    top: 0;
    z-index: 10;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 0.55rem;
    text-decoration: none;
    color: var(--text);
  }
  .wordmark {
    font-weight: 700;
    font-size: 1.02rem;
    letter-spacing: -0.01em;
    background: var(--accent-grad);
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
  }
  .links {
    display: flex;
    gap: 0.25rem;
    margin-left: auto;
    flex-wrap: wrap;
  }
  .links a {
    color: var(--text-dim);
    text-decoration: none;
    padding: 0.38rem 0.7rem;
    border-radius: var(--radius-sm);
    font-size: 0.9rem;
    transition:
      background 0.12s ease,
      color 0.12s ease;
  }
  .links a:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .links a.active {
    background: var(--accent-soft);
    color: var(--accent-soft-text);
  }
</style>
