<script lang="ts">
  import {
    getStats,
    getStatus,
    DaemonOfflineError,
    type Stats,
    type Status,
  } from "$lib/api";
  import { nowUnix, formatCountdown, formatClock } from "$lib/time";

  const POLL_MS = 3000;

  let stats = $state<Stats | null>(null);
  let status = $state<Status | null>(null);
  let offline = $state(false);
  let error = $state<string | null>(null);

  // 1Hz tick so the "collecting for" uptime ticks without re-polling.
  let now = $state(nowUnix());
  $effect(() => {
    const id = setInterval(() => (now = nowUnix()), 1000);
    return () => clearInterval(id);
  });

  async function refresh() {
    try {
      [stats, status] = await Promise.all([getStats(), getStatus()]);
      offline = false;
      error = null;
    } catch (e) {
      if (e instanceof DaemonOfflineError) offline = true;
      else error = e instanceof Error ? e.message : String(e);
    }
  }

  $effect(() => {
    refresh();
    const id = setInterval(refresh, POLL_MS);
    return () => clearInterval(id);
  });

  let maxCount = $derived(stats?.top.reduce((m, d) => Math.max(m, d.count), 0) ?? 0);
  let uptime = $derived(stats ? Math.max(0, now - stats.since_unix) : 0);
  // Stats only accrue when the root daemon's DNS sinkhole is enforcing.
  let inactive = $derived(status != null && !status.privileged);
</script>

<h1>Statistics</h1>

{#if offline}
  <div class="card note">Daemon offline — retrying…</div>
{:else if error}
  <div class="card note err">{error}</div>
{:else if stats}
  <div class="cards">
    <div class="card stat">
      <span class="label">Queries blocked</span>
      <span class="value">{stats.total_blocked.toLocaleString()}</span>
    </div>
    <div class="card stat">
      <span class="label">Domains hit</span>
      <span class="value">{stats.unique_domains.toLocaleString()}</span>
    </div>
    <div class="card stat">
      <span class="label">Collecting for</span>
      <span class="value small">{formatCountdown(uptime)}</span>
    </div>
  </div>

  {#if inactive}
    <div class="card note">
      The daemon isn't running as root, so the DNS sinkhole is inactive and no
      queries are being counted. Statistics accrue once the privileged daemon is
      enforcing.
    </div>
  {/if}

  <section class="card">
    <h2>Top blocked</h2>
    {#if stats.top.length === 0}
      <p class="muted">No blocked queries recorded yet.</p>
    {:else}
      <ul class="bars">
        {#each stats.top as row (row.entry)}
          <li>
            <div class="row">
              <span class="entry" title={row.entry}>{row.entry}</span>
              <span class="count">{row.count.toLocaleString()}</span>
            </div>
            <div class="track">
              <div
                class="fill"
                style="width: {maxCount > 0 ? (row.count / maxCount) * 100 : 0}%"
              ></div>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <section class="card">
    <h2>Recent activity</h2>
    {#if stats.recent.length === 0}
      <p class="muted">Nothing blocked recently.</p>
    {:else}
      <ul class="recent">
        {#each stats.recent as r, i (r.unix + "-" + r.name + "-" + i)}
          <li>
            <span class="dot"></span>
            <span class="name" title={r.name}>{r.name}</span>
            <span class="time">{formatClock(r.unix)}</span>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{:else}
  <div class="card note">Loading…</div>
{/if}

<style>
  .cards {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 1rem;
  }
  @media (max-width: 560px) {
    .cards {
      grid-template-columns: 1fr;
    }
  }
  .stat {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }
  .stat .label {
    font-size: 0.78rem;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .stat .value {
    font-size: 1.9rem;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.02em;
  }
  .stat .value.small {
    font-size: 1.35rem;
  }
  h2 {
    font-size: 1rem;
    margin: 0 0 0.9rem;
  }
  .bars {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
  }
  .bars .row {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 1rem;
    margin-bottom: 0.3rem;
  }
  .entry {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.86rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    color: var(--text-dim);
    font-variant-numeric: tabular-nums;
    flex: 0 0 auto;
  }
  .track {
    height: 7px;
    border-radius: 99px;
    background: var(--surface-3);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    border-radius: 99px;
    background: var(--accent-grad);
    transition: width 0.3s ease;
  }
  .recent {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  .recent li {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.4rem 0;
    border-top: 1px solid var(--border);
  }
  .recent li:first-child {
    border-top: none;
  }
  .recent .dot {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--bad);
    flex: 0 0 auto;
  }
  .recent .name {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.85rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .recent .time {
    margin-left: auto;
    color: var(--text-faint);
    font-size: 0.8rem;
    font-variant-numeric: tabular-nums;
    flex: 0 0 auto;
  }
  .muted {
    color: var(--text-dim);
    margin: 0;
  }
  .note {
    color: var(--text-dim);
  }
  .note.err {
    color: var(--warn);
  }
</style>
