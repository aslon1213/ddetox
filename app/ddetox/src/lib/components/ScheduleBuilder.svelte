<script lang="ts">
  import { minToHHMM, hhmmToMin } from "$lib/config";
  import { WEEKDAY_LABELS, type CivilDate, type Recurrence, type RecurrenceKind, type ScheduleRule } from "$lib/types";

  let { rules = $bindable() }: { rules: ScheduleRule[] } = $props();

  function todayCivil(): CivilDate {
    const d = new Date();
    return { year: d.getFullYear(), month: d.getMonth() + 1, day: d.getDate() };
  }
  function civilToInput(d: CivilDate): string {
    return `${d.year}-${String(d.month).padStart(2, "0")}-${String(d.day).padStart(2, "0")}`;
  }
  function inputToCivil(s: string): CivilDate | null {
    const m = s.match(/^(\d{4})-(\d{2})-(\d{2})$/);
    return m ? { year: +m[1], month: +m[2], day: +m[3] } : null;
  }

  const KINDS: { value: RecurrenceKind; label: string }[] = [
    { value: "everyday", label: "Every day" },
    { value: "weekdays", label: "Specific weekdays" },
    { value: "month_days", label: "Day-of-month range" },
    { value: "every_n_days", label: "Every N days" },
    { value: "date_range", label: "Fixed date range" },
  ];

  function defaultRecurrence(kind: RecurrenceKind): Recurrence {
    switch (kind) {
      case "everyday":
        return { kind: "everyday" };
      case "weekdays":
        return { kind: "weekdays", days: [0, 1, 2, 3, 4] };
      case "month_days":
        return { kind: "month_days", start_day: 1, end_day: 10 };
      case "every_n_days":
        return { kind: "every_n_days", n: 2, anchor: todayCivil() };
      case "date_range":
        return { kind: "date_range", start: todayCivil(), end: todayCivil() };
    }
  }

  function commit() {
    rules = [...rules];
  }

  function addRule() {
    rules = [...rules, { recurrence: { kind: "everyday" }, windows: [] }];
  }
  function removeRule(i: number) {
    rules = rules.filter((_, idx) => idx !== i);
  }
  function setKind(i: number, kind: RecurrenceKind) {
    rules[i].recurrence = defaultRecurrence(kind);
    commit();
  }
  function toggleDay(i: number, day: number) {
    const r = rules[i].recurrence;
    if (r.kind !== "weekdays") return;
    r.days = r.days.includes(day) ? r.days.filter((d) => d !== day) : [...r.days, day].sort();
    commit();
  }
  function addWindow(i: number) {
    rules[i].windows = [...rules[i].windows, { start_min: 9 * 60, end_min: 17 * 60 }];
    commit();
  }
  function removeWindow(i: number, w: number) {
    rules[i].windows = rules[i].windows.filter((_, idx) => idx !== w);
    commit();
  }
  function setWindowTime(i: number, w: number, field: "start_min" | "end_min", value: string) {
    const min = hhmmToMin(value);
    if (min === null) return;
    rules[i].windows[w][field] = min;
    commit();
  }
</script>

<div class="builder">
  {#each rules as rule, i (i)}
    <div class="rule">
      <div class="ruletop">
        <select
          value={rule.recurrence.kind}
          onchange={(e) => setKind(i, e.currentTarget.value as RecurrenceKind)}
        >
          {#each KINDS as k (k.value)}
            <option value={k.value}>{k.label}</option>
          {/each}
        </select>
        <button class="ghost" onclick={() => removeRule(i)} title="Remove rule">Remove</button>
      </div>

      {#if rule.recurrence.kind === "weekdays"}
        <div class="weekdays">
          {#each WEEKDAY_LABELS as label, d (d)}
            <button
              class="day"
              class:on={rule.recurrence.days.includes(d)}
              onclick={() => toggleDay(i, d)}
            >
              {label}
            </button>
          {/each}
        </div>
      {:else if rule.recurrence.kind === "month_days"}
        <div class="inline">
          <label>From day <input type="number" min="1" max="31" bind:value={rule.recurrence.start_day} onchange={commit} /></label>
          <label>to <input type="number" min="1" max="31" bind:value={rule.recurrence.end_day} onchange={commit} /></label>
          <span class="muted">of every month</span>
        </div>
      {:else if rule.recurrence.kind === "every_n_days"}
        <div class="inline">
          <label>Every <input type="number" min="1" max="365" bind:value={rule.recurrence.n} onchange={commit} /> day(s)</label>
          <label>from
            <input
              type="date"
              value={civilToInput(rule.recurrence.anchor)}
              onchange={(e) => { const c = inputToCivil(e.currentTarget.value); if (c && rule.recurrence.kind === "every_n_days") { rule.recurrence.anchor = c; commit(); } }}
            />
          </label>
        </div>
      {:else if rule.recurrence.kind === "date_range"}
        <div class="inline">
          <label>From
            <input type="date" value={civilToInput(rule.recurrence.start)}
              onchange={(e) => { const c = inputToCivil(e.currentTarget.value); if (c && rule.recurrence.kind === "date_range") { rule.recurrence.start = c; commit(); } }} />
          </label>
          <label>to
            <input type="date" value={civilToInput(rule.recurrence.end)}
              onchange={(e) => { const c = inputToCivil(e.currentTarget.value); if (c && rule.recurrence.kind === "date_range") { rule.recurrence.end = c; commit(); } }} />
          </label>
        </div>
      {/if}

      <div class="windows">
        <div class="winhead">
          <span class="muted">{rule.windows.length === 0 ? "All day" : "Time windows"}</span>
          <button class="ghost small" onclick={() => addWindow(i)}>+ window</button>
        </div>
        {#each rule.windows as win, w (w)}
          <div class="winrow">
            <input type="time" value={minToHHMM(win.start_min)} onchange={(e) => setWindowTime(i, w, "start_min", e.currentTarget.value)} />
            <span>–</span>
            <input type="time" value={minToHHMM(win.end_min)} onchange={(e) => setWindowTime(i, w, "end_min", e.currentTarget.value)} />
            {#if win.end_min <= win.start_min}<span class="warn">end must be after start</span>{/if}
            <button class="ghost small" onclick={() => removeWindow(i, w)}>×</button>
          </div>
        {/each}
      </div>
    </div>
  {/each}

  <button class="ghost" onclick={addRule}>+ Add schedule rule</button>
  {#if rules.length === 0}
    <p class="muted">No rules — this session never activates. Add a rule above.</p>
  {/if}
</div>

<style>
  .builder {
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
  }
  .rule {
    background: #141416;
    border: 1px solid #232327;
    border-radius: 8px;
    padding: 0.8rem;
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
  }
  .ruletop {
    display: flex;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .weekdays {
    display: flex;
    gap: 0.3rem;
    flex-wrap: wrap;
  }
  .day {
    padding: 0.3rem 0.55rem;
    font-size: 0.8rem;
  }
  .day.on {
    background: #1b2a4a;
    border-color: #3a5b9a;
    color: #cfe0ff;
  }
  .inline {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    flex-wrap: wrap;
    font-size: 0.85rem;
  }
  .inline input[type="number"] {
    width: 4rem;
  }
  .windows {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  .winhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .winrow {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.85rem;
  }
  .ghost {
    background: transparent;
  }
  .small {
    font-size: 0.78rem;
    padding: 0.25rem 0.5rem;
  }
  .muted {
    color: #8a8a8e;
    font-size: 0.85rem;
  }
  .warn {
    color: #e6a23c;
    font-size: 0.75rem;
  }
</style>
