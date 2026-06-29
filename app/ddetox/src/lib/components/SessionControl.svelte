<script lang="ts">
  import type { Status } from "$lib/api";
  import { startSession, stopSession } from "$lib/api";
  import { nowUnix, formatClock } from "$lib/time";

  // The real daemon (dp_course) returns "not implemented" for StartSession /
  // StopSession in this build. Flip this to `true` (or probe a capability) once
  // the daemon ships sessions — the controls below are already wired.
  const SESSIONS_SUPPORTED = false;

  let {
    status,
    onChanged,
    onError,
  }: {
    status: Status | null;
    onChanged: () => Promise<void>;
    onError: (msg: string) => void;
  } = $props();

  const PRESETS = [
    { label: "30 min", minutes: 30 },
    { label: "1 hour", minutes: 60 },
    { label: "2 hours", minutes: 120 },
    { label: "4 hours", minutes: 240 },
  ];

  let minutes = $state(60);
  let committed = $state(false);
  let busy = $state(false);

  let active = $derived(status?.session != null);
  let disabled = $derived(busy || !SESSIONS_SUPPORTED);

  async function run(fn: () => Promise<void>) {
    busy = true;
    try {
      await fn();
      await onChanged();
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    } finally {
      busy = false;
    }
  }

  async function onStart() {
    const until = nowUnix() + minutes * 60;
    const wantCommitted = committed;
    await run(() => startSession(until, wantCommitted));
  }

  async function onStop() {
    await run(() => stopSession());
  }
</script>

<section class="session" class:unsupported={!SESSIONS_SUPPORTED}>
  <div class="head">
    <h2>Session</h2>
    {#if !SESSIONS_SUPPORTED}
      <span class="tag">unavailable in this daemon build</span>
    {/if}
  </div>

  {#if active}
    <p class="state">
      A session is reported active{#if status?.session?.committed}
        <strong> (committed)</strong>{/if}.
      {#if status?.session?.until_unix}
        Ends at {formatClock(status.session.until_unix)}.
      {/if}
    </p>
    <button class="stop" disabled={disabled} onclick={onStop}>Stop session</button>
  {:else}
    <div class="picker">
      <div class="presets">
        {#each PRESETS as preset (preset.minutes)}
          <button
            class="preset"
            class:selected={minutes === preset.minutes}
            onclick={() => (minutes = preset.minutes)}
            disabled={disabled}
          >
            {preset.label}
          </button>
        {/each}
      </div>

      <label class="custom">
        Custom minutes
        <input type="number" min="1" max="1440" bind:value={minutes} disabled={disabled} />
      </label>

      <label class="committed">
        <input type="checkbox" bind:checked={committed} disabled={disabled} />
        <span>
          Committed — once started, the daemon will <strong>not</strong> let you stop
          early or remove blocks until it ends.
        </span>
      </label>

      <button class="start" disabled={disabled || minutes < 1} onclick={onStart}>
        Start{committed ? " committed" : ""} session
      </button>
    </div>
  {/if}

  {#if !SESSIONS_SUPPORTED}
    <p class="note">
      Sessions aren't supported by the connected daemon yet. These controls are
      disabled until it implements <code>StartSession</code> / <code>StopSession</code>.
    </p>
  {/if}
</section>

<style>
  .session {
    background: #161618;
    border-radius: 10px;
    padding: 1.1rem 1.2rem;
  }
  .session.unsupported {
    opacity: 0.85;
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    margin-bottom: 0.8rem;
  }
  h2 {
    font-size: 1rem;
    margin: 0;
  }
  .tag {
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: #b6932f;
  }
  .presets {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-bottom: 0.8rem;
  }
  .preset.selected {
    border-color: #5b8def;
    background: #1b2a4a;
    color: #cfe0ff;
  }
  .custom {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.9rem;
    margin-bottom: 0.8rem;
  }
  .custom input {
    width: 6rem;
  }
  .committed {
    display: flex;
    gap: 0.6rem;
    align-items: flex-start;
    font-size: 0.85rem;
    color: #b6b6ba;
    margin-bottom: 1rem;
    line-height: 1.4;
  }
  .committed input {
    margin-top: 0.15rem;
  }
  .start {
    width: 100%;
    padding: 0.6rem;
    font-weight: 600;
    background: #1f5e38;
    border-color: #2a7d4b;
    color: #d8f3e4;
  }
  .stop {
    width: 100%;
    padding: 0.6rem;
    font-weight: 600;
    background: #5e1f1f;
    border-color: #7d2a2a;
    color: #f3d8d8;
  }
  button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .state {
    margin: 0 0 0.8rem;
  }
  .note {
    margin: 0.9rem 0 0;
    font-size: 0.8rem;
    color: #7a7a7e;
    line-height: 1.4;
  }
  code {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.78rem;
  }
</style>
