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