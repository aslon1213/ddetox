<script lang="ts">
  import {
    getSessions,
    saveSessions,
    getLibrary,
    reconcileNow,
    describeSchedule,
    newId,
  } from "$lib/config";
  import SessionEditor from "$lib/components/SessionEditor.svelte";
  import type { Session, WebsiteItem } from "$lib/types";

  let sessions = $state<Session[]>([]);
  let library = $state<WebsiteItem[]>([]);
  let error = $state<string | null>(null);

  // The editor edits this always-present copy; `editorOpen` controls visibility,
  // which keeps the bound type a plain Session (never null).
  let editing = $state<Session>(blankSession());
  let editorOpen = $state(false);
  let isNew = $state(false);

  function blankSession(): Session {
    return {
      id: newId(),
      name: "",
      item_ids: [],
      enabled: true,
      rules: [{ recurrence: { kind: "everyday" }, windows: [] }],
    };
  }

  function labelFor(ids: string[]): string {
    const names = ids
      .map((id) => library.find((i) => i.id === id)?.label)
      .filter((n): n is string => Boolean(n));
    return names.length ? names.join(", ") : "no sites";
  }

  async function load() {
    try {
      [sessions, library] = await Promise.all([getSessions(), getLibrary()]);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }
  $effect(() => {
    load();
  });

  function startNew() {
    editing = blankSession();
    isNew = true;
    editorOpen = true;
  }
  function startEdit(s: Session) {
    editing = structuredClone($state.snapshot(s));
    isNew = false;
    editorOpen = true;
  }

  async function persist(next: Session[]) {
    sessions = next;
    error = null;
    try {
      await saveSessions(next);
      await reconcileNow();
    } catch (e) {
      // Save succeeds locally; a daemon-offline reconcile is soft (loop retries).
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function onSave(s: Session) {
    const next = sessions.some((x) => x.id === s.id)
      ? sessions.map((x) => (x.id === s.id ? s : x))
      : [...sessions, s];
    editorOpen = false;
    await persist(next);
  }

  async function onDelete(id: string) {
    editorOpen = false;
    await persist(sessions.filter((s) => s.id !== id));
  }
</script>

<div class="head">
  <h1>Sessions</h1>
  {#if !editorOpen}
    <button class="primary" onclick={startNew}>New session</button>
  {/if}
</div>
<p class="lede">
  A session blocks a set of library sites on a schedule. While the app is running,
  active sessions are enforced through the daemon automatically.
</p>

{#if error}<p class="error">{error}</p>{/if}

{#if editorOpen}
  <SessionEditor
    bind:draft={editing}
    {library}
    {onSave}
    onCancel={() => (editorOpen = false)}
    onDelete={isNew ? null : onDelete}
  />
{:else}
  <div class="list">
    {#if sessions.length === 0}
      <p class="muted">No sessions yet. Create one to schedule blocking.</p>
    {/if}
    {#each sessions as s (s.id)}
      <button class="row" onclick={() => startEdit(s)}>
        <div class="info">
          <div class="name">
            {s.name || "(unnamed)"}
            <span class="badge" class:off={!s.enabled}>{s.enabled ? "enabled" : "off"}</span>
          </div>
          <div class="sub">Blocks: {labelFor(s.item_ids)}</div>
          <div class="sched">{describeSchedule(s.rules)}</div>
        </div>
        <span class="chev">Edit ›</span>
      </button>
    {/each}
  </div>
{/if}

<style>
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .lede {
    margin: 0;
    color: var(--text-dim);
    line-height: 1.5;
  }
  .error {
    color: var(--warn);
    font-size: 0.85rem;
    margin: 0;
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    text-align: left;
    padding: 0.8rem 1rem;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    width: 100%;
  }
  .row:hover {
    border-color: var(--border-strong);
  }
  .name {
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .badge {
    font-size: 0.65rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 0.08rem 0.35rem;
    border-radius: 4px;
    background: #1f5e38;
    color: #cfeede;
  }
  .badge.off {
    background: var(--surface-3);
    color: var(--text-dim);
  }
  .sub {
    font-size: 0.8rem;
    color: var(--text-dim);
    margin-top: 0.2rem;
  }
  .sched {
    font-size: 0.78rem;
    color: var(--accent);
    margin-top: 0.15rem;
  }
  .chev {
    color: var(--text-faint);
    font-size: 0.85rem;
    flex: 0 0 auto;
  }
  .muted {
    color: var(--text-dim);
  }
</style>
