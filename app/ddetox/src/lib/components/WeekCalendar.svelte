<script lang="ts">
  import { getSessions, sessionWindowsOnDay, minToHHMM } from "$lib/config";
  import { WEEKDAY_LABELS, type Session } from "$lib/types";

  let sessions = $state<Session[]>([]);
  let error = $state<string | null>(null);
  let weekOffset = $state(0);

  $effect(() => {
    getSessions()
      .then((s) => (sessions = s))
      .catch((e) => (error = e instanceof Error ? e.message : String(e)));
  });

  function startOfWeek(offset: number): Date {
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    const mon0 = (d.getDay() + 6) % 7; // JS Sunday=0 -> Monday=0
    d.setDate(d.getDate() - mon0 + offset * 7);
    return d;
  }

  // A colored block for one session's window on one day.
  interface Block {
    sessionId: string;
    name: string;
    color: string;
    topPct: number;
    heightPct: number;
    label: string;
  }

  function color(idx: number): string {
    return `hsl(${(idx * 67) % 360} 45% 42%)`;
  }

  let week = $derived(startOfWeek(weekOffset));
  let todayKey = $derived(new Date().toDateString());

  let days = $derived(
    Array.from({ length: 7 }, (_, i) => {
      const date = new Date(week);
      date.setDate(week.getDate() + i);
      const y = date.getFullYear();
      const m = date.getMonth() + 1;
      const d = date.getDate();

      const blocks: Block[] = [];
      sessions.forEach((session, si) => {
        for (const win of sessionWindowsOnDay(session, y, m, d)) {
          blocks.push({
            sessionId: session.id,
            name: session.name || "(unnamed)",
            color: color(si),
            topPct: (win.start_min / 1440) * 100,
            heightPct: ((win.end_min - win.start_min) / 1440) * 100,
            label: `${minToHHMM(win.start_min)}–${minToHHMM(win.end_min)}`,
          });
        }
      });

      return {
        label: WEEKDAY_LABELS[i],
        dateLabel: `${m}/${d}`,
        isToday: date.toDateString() === todayKey,
        blocks,
      };
    }),
  );

  const HOUR_MARKS = [0, 3, 6, 9, 12, 15, 18, 21];
</script>

<div class="toolbar">
  <button onclick={() => (weekOffset -= 1)}>‹ Prev</button>
  <button onclick={() => (weekOffset = 0)} disabled={weekOffset === 0}>This week</button>
  <button onclick={() => (weekOffset += 1)}>Next ›</button>
  <span class="range">
    week of {week.getMonth() + 1}/{week.getDate()}/{week.getFullYear()}
  </span>
</div>

{#if error}<p class="error">{error}</p>{/if}

<div class="calendar">
  <div class="axis">
    <div class="corner"></div>
    <div class="hours">
      {#each HOUR_MARKS as h (h)}
        <div class="hour" style="top:{(h / 24) * 100}%">{String(h).padStart(2, "0")}:00</div>
      {/each}
    </div>
  </div>

  {#each days as day (day.label)}
    <div class="day">
      <div class="dayhead" class:today={day.isToday}>
        {day.label}<span class="date">{day.dateLabel}</span>
      </div>
      <div class="track">
        {#each HOUR_MARKS as h (h)}
          <div class="gridline" style="top:{(h / 24) * 100}%"></div>
        {/each}
        {#each day.blocks as block (block.sessionId + block.topPct)}
          <div
            class="block"
            style="top:{block.topPct}%; height:{block.heightPct}%; background:{block.color}"
            title="{block.name} · {block.label}"
          >
            <span class="bname">{block.name}</span>
            <span class="btime">{block.label}</span>
          </div>
        {/each}
      </div>
    </div>
  {/each}
</div>

{#if sessions.length === 0}
  <p class="muted">No sessions to show. Create sessions to see them here.</p>
{/if}

<style>
  .toolbar {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .range {
    margin-left: auto;
    color: #9a9a9e;
    font-size: 0.85rem;
  }
  .error {
    color: #ff9b9b;
    font-size: 0.85rem;
  }
  .calendar {
    display: grid;
    grid-template-columns: 48px repeat(7, 1fr);
    gap: 2px;
    background: #161618;
    border-radius: 10px;
    padding: 0.6rem;
  }
  .corner {
    height: 34px;
  }
  .axis .hours,
  .track {
    position: relative;
    height: 480px;
  }
  .hour {
    position: absolute;
    right: 4px;
    transform: translateY(-50%);
    font-size: 0.68rem;
    color: #6f6f73;
  }
  .dayhead {
    height: 34px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    font-size: 0.82rem;
    color: #c6c6ca;
  }
  .dayhead.today {
    color: #cfe0ff;
  }
  .date {
    font-size: 0.68rem;
    color: #7a7a7e;
  }
  .track {
    background: #121214;
    border-radius: 5px;
    overflow: hidden;
  }
  .gridline {
    position: absolute;
    left: 0;
    right: 0;
    height: 1px;
    background: #1f1f22;
  }
  .block {
    position: absolute;
    left: 2px;
    right: 2px;
    border-radius: 4px;
    padding: 2px 4px;
    overflow: hidden;
    color: #f3f3f5;
    box-sizing: border-box;
    min-height: 12px;
  }
  .bname {
    display: block;
    font-size: 0.68rem;
    font-weight: 600;
    white-space: nowrap;
    text-overflow: ellipsis;
    overflow: hidden;
  }
  .btime {
    display: block;
    font-size: 0.62rem;
    opacity: 0.85;
  }
  .muted {
    color: #8a8a8e;
  }
</style>
