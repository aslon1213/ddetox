<script lang="ts">
  import { getStatus, DaemonOfflineError, type Status } from "$lib/api";
  import StatusBar from "$lib/components/StatusBar.svelte";
  import BlocklistEditor from "$lib/components/BlocklistEditor.svelte";
  import SessionControl from "$lib/components/SessionControl.svelte";
  import ActiveSessions from "$lib/components/ActiveSessions.svelte";

  const POLL_MS = 2000;

  // The daemon is the source of truth; this is a cache refreshed every poll and
  // after every mutation. We never mutate it optimistically.
  let status = $state<Status | null>(null);
  let offline = $state(false);
  let actionError = $state<string | null>(null);

  // A committed focus-lock session locks removals (daemon enforces; UI reflects).
  let locked = $derived(status?.session?.committed === true);

  async function refresh() {
    try {
      status = await getStatus();
      offline = false;
    } catch (e) {
      if (e instanceof DaemonOfflineError) {
        offline = true;
      } else {
        actionError = e instanceof Error ? e.message : String(e);
      }
    }
  }

  $effect(() => {
    refresh();
    const id = setInterval(refresh, POLL_MS);
    return () => clearInterval(id);
  });
</script>

<h1>Status</h1>

<StatusBar
  {status}
  {offline}
  error={actionError}
  onDismissError={() => (actionError = null)}
/>

<ActiveSessions />

<BlocklistEditor
  {status}
  {locked}
  onChanged={refresh}
  onError={(msg) => (actionError = msg)}
/>

<SessionControl
  {status}
  onChanged={refresh}
  onError={(msg) => (actionError = msg)}
/>
