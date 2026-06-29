<script lang="ts">
  import { page } from "$app/stores";

  const links = [
    { href: "/", label: "Status" },
    { href: "/library", label: "Library" },
    { href: "/sessions", label: "Sessions" },
    { href: "/calendar", label: "Calendar" },
  ];

  let path = $derived($page.url.pathname);
</script>

<nav class="nav">
  <span class="brand">Blocker</span>
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
    padding: 0.7rem 1.5rem;
    border-bottom: 1px solid #1f1f22;
    background: #111113;
    position: sticky;
    top: 0;
    z-index: 10;
  }
  .brand {
    font-weight: 700;
    letter-spacing: 0.02em;
  }
  .links {
    display: flex;
    gap: 0.4rem;
  }
  .links a {
    color: #b6b6ba;
    text-decoration: none;
    padding: 0.35rem 0.7rem;
    border-radius: 6px;
    font-size: 0.92rem;
  }
  .links a:hover {
    background: #1c1c1f;
    color: #e6e6e8;
  }
  .links a.active {
    background: #1b2a4a;
    color: #cfe0ff;
  }
</style>
