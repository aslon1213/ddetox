<script lang="ts">
  import type { Status } from "$lib/api";
  import { formatCountdown, formatClock, nowUnix } from "$lib/time";

  let {
    status,
    offline,
    error,
    onDismissError,
  }: {
    status: Status | null;
    offline: boolean;
    error: string | null;
    onDismissError: () => void;
  } = $props();

  // 1Hz tick so any session countdown re-renders without re-polling.
  let now = $state(nowUnix());
  $effect(() => {
    const id = setInterval(() => (now = nowUnix()), 1000);
    return () => clearInterval(id);
  });

  let sessionEnd = $derived(status?.session?.until_unix);
  let secondsLeft = $derived(sessionEnd ? sessionEnd - now : 0);
</script>

<header class="statusbar">
  {#if offline}
    <div class="banner offline" role="status">
      <span class="dot"></span>
      Daemon offline — retrying…
    </div>
  {:else if status}
    <div class="banner online" role="status">
      <span class="dot"></span>
      <strong>Connected</strong>
      <span class="meta">blockerd v{status.daemon_version}</span>
      <span class="meta">pid {status.pid}</span>
      <span class="badge" class:warn={!status.privileged}>
        {status.privileged ? "root" : "unprivileged"}
      </span>
      {#if status.block_page}
        <span class="badge page" title="Blocked sites resolve to a local block page">
          block page
        </span>
      {/if}

      {#if status.session}
        <span class="session">
          session active
          {#if sessionEnd}
            · {formatCountdown(secondsLeft)} left (until {formatClock(sessionEnd)})
          {/if}
        </span>
      {:else}
        <span class="session idle">no active session</span>
      {/if}
    </div>
  {:else}
    <div class="banner connecting" role="status">
      <span class="dot"></span>
      Connecting…
    </div>
  {/if}

  {#if error}
    <div class="banner error" role="alert">
      <span class="msg">{error}</span>
      <button class="dismiss" onclick={onDismissError} aria-label="Dismiss">×</button>
    </div>
  {/if}
</header>

<style>
  .statusbar {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .banner {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.6rem 0.9rem;
    border-radius: 8px;
    font-size: 0.9rem;
    flex-wrap: wrap;
  }
  .dot {
    width: 0.6rem;
    height: 0.6rem;
    border-radius: 50%;
    flex: 0 0 auto;
  }
  .offline {
    background: var(--bad-soft);
    color: var(--bad-text);
  }
  .offline .dot {
    background: var(--bad);
    animation: pulse 1s infinite;
  }
  .online {
    background: var(--good-soft);
    color: var(--good-text);
  }
  .online .dot {
    background: var(--good);
  }
  .connecting {
    background: var(--surface-2);
    color: var(--text-dim);
  }
  .connecting .dot {
    background: var(--text-faint);
  }
  .meta {
    opacity: 0.8;
    font-variant-numeric: tabular-nums;
  }
  .badge {
    text-transform: uppercase;
    font-size: 0.68rem;
    letter-spacing: 0.05em;
    padding: 0.12rem 0.42rem;
    border-radius: 5px;
    background: #1f5e38;
    color: #cfeede;
  }
  .badge.warn {
    background: #5e4a1f;
    color: #efe0b8;
  }
  .badge.page {
    background: var(--accent-soft);
    color: var(--accent-soft-text);
  }
  .session {
    margin-left: auto;
    font-weight: 600;
  }
  .session.idle {
    font-weight: 400;
    opacity: 0.7;
  }
  .error {
    background: #3a2a10;
    color: #ffd9a0;
    justify-content: space-between;
  }
  .dismiss {
    background: transparent;
    border: none;
    color: inherit;
    font-size: 1.2rem;
    line-height: 1;
    cursor: pointer;
    padding: 0 0.3rem;
  }
  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.3;
    }
  }
</style>
