<script lang="ts">
  // The Blocker logo, inlined as SVG so it stays crisp at any size and ships
  // with no extra network request. Mirrors static/logo.svg.
  let { size = 28 }: { size?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 512 512"
  fill="none"
  xmlns="http://www.w3.org/2000/svg"
  aria-hidden="true"
>
  <defs>
    <linearGradient id="bm-bg" x1="0" y1="0" x2="512" y2="512" gradientUnits="userSpaceOnUse">
      <stop stop-color="#3b82f6" />
      <stop offset="1" stop-color="#6366f1" />
    </linearGradient>
    <linearGradient id="bm-shield" x1="256" y1="92" x2="256" y2="432" gradientUnits="userSpaceOnUse">
      <stop stop-color="#ffffff" />
      <stop offset="1" stop-color="#dbe6ff" />
    </linearGradient>
  </defs>
  <rect width="512" height="512" rx="114" fill="url(#bm-bg)" />
  <path
    d="M256 92 L398 142 V268 C398 350 332 404 256 432 C180 404 114 350 114 268 V142 Z"
    fill="url(#bm-shield)"
  />
  <g stroke="#3b5bdb" stroke-width="26" stroke-linecap="round" fill="none">
    <circle cx="256" cy="262" r="74" />
    <line x1="206" y1="212" x2="306" y2="312" />
  </g>
</svg>
