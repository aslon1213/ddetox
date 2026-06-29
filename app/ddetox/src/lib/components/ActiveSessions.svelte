<script lang="ts">
  import { getSessions, getScheduleState, reconcileNow } from "$lib/config";
  import type { Session, ScheduleState } from "$lib/types";

  let sessions = $state<Session[]>([]);
  let schedule = $state<ScheduleState | null>(null);
  let note = $state<string | null>(null);
  let busy = $state(false);

  async function refresh() {
    try {
      [sessions, schedule] = await Promise.all([getSessions(), getScheduleState()]);
    } catch (e) {
      note = e instanceof Error ? e.message : String(e);
    }
  }

  async function reconcile() {
    busy = true;
    note = null;
    try {
      schedule = await reconcileNow();
    } catch (e) {
      // Daemon offline is soft — the background loop keeps retrying.
      note = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    refresh();
    const id = setInterval(refresh, 5000);
    return () => clearInterval(id);
  });

  let activeIds = $derived(new Set(schedule?.active_session_ids ?? []));
  let active = $derived(sessions.filter((s) => activeIds.has(s.id)));
</script>

<section class="panel">
  <div class="head">
    <h2>Active sessions</h2>
    <button onclick={reconcile} disabled={busy}>Reconcile now</button>
  </div>

  {#if active.length > 0}
    <ul class="active">
      {#each active as s (s.id)}
        <li><span class="dot"></span>{s.name}</li>
      {/each}
    </ul>
  {:else}
    <p class="muted">No sessions are active right now.</p>
  {/if}

  {#if schedule}
    <p class="muted small">
      Scheduler is enforcing {schedule.managed_domains.length} domain(s) and
      {schedule.managed_cidrs.length} address(es) on behalf of sessions.
    </p>
  {/if}

  {#if note}
    <p class="note">{note}</p>
  {/if}
</section>

<style>
  .panel {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 1.15rem 1.25rem;
    box-shadow: var(--shadow);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0.6rem;
  }
  h2 {
    font-size: 1rem;
    margin: 0;
  }
  .active {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .active li {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .dot {
    width: 0.55rem;
    height: 0.55rem;
    border-radius: 50%;
    background: #36d27a;
  }
  .muted {
    color: #8a8a8e;
    margin: 0.3rem 0 0;
  }
  .small {
    font-size: 0.82rem;
  }
  .note {
    margin: 0.5rem 0 0;
    font-size: 0.82rem;
    color: #e6a23c;
  }
</style>
