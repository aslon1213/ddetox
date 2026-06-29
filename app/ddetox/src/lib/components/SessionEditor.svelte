<script lang="ts">
  import ScheduleBuilder from "$lib/components/ScheduleBuilder.svelte";
  import type { Session, WebsiteItem } from "$lib/types";

  // `draft` is an editable copy owned by the parent (passed via bind:). Cancelling
  // just discards it; saving snapshots it back through onSave.
  let {
    draft = $bindable(),
    library,
    onSave,
    onCancel,
    onDelete,
  }: {
    draft: Session;
    library: WebsiteItem[];
    onSave: (s: Session) => void;
    onCancel: () => void;
    onDelete: ((id: string) => void) | null;
  } = $props();

  let error = $state<string | null>(null);

  function toggleItem(id: string) {
    draft.item_ids = draft.item_ids.includes(id)
      ? draft.item_ids.filter((x) => x !== id)
      : [...draft.item_ids, id];
  }

  function save() {
    error = null;
    if (!draft.name.trim()) {
      error = "Name the session.";
      return;
    }
    if (draft.item_ids.length === 0) {
      error = "Pick at least one site to block.";
      return;
    }
    for (const rule of draft.rules) {
      for (const w of rule.windows) {
        if (w.end_min <= w.start_min) {
          error = "Every time window must end after it starts.";
          return;
        }
      }
    }
    draft.name = draft.name.trim();
    onSave($state.snapshot(draft) as Session);
  }
</script>

<div class="editor">
  <div class="top">
    <input class="name" placeholder="Session name (e.g. Work focus)" bind:value={draft.name} />
    <label class="enabled">
      <input type="checkbox" bind:checked={draft.enabled} /> Enabled
    </label>
  </div>

  <div class="block">
    <h3>Block these sites</h3>
    {#if library.length === 0}
      <p class="muted">Your library is empty — add sites first.</p>
    {:else}
      <div class="items">
        {#each library as item (item.id)}
          <label class="item" class:on={draft.item_ids.includes(item.id)}>
            <input
              type="checkbox"
              checked={draft.item_ids.includes(item.id)}
              onchange={() => toggleItem(item.id)}
            />
            {item.label}
          </label>
        {/each}
      </div>
    {/if}
  </div>

  <div class="block">
    <h3>Schedule</h3>
    <ScheduleBuilder bind:rules={draft.rules} />
  </div>

  {#if error}<p class="error">{error}</p>{/if}

  <div class="actions">
    <button class="primary" onclick={save}>Save session</button>
    <button onclick={onCancel}>Cancel</button>
    {#if onDelete}
      <button class="danger" onclick={() => onDelete?.(draft.id)}>Delete</button>
    {/if}
  </div>
</div>

<style>
  .editor {
    background: #161618;
    border-radius: 10px;
    padding: 1.1rem 1.2rem;
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }
  .top {
    display: flex;
    gap: 1rem;
    align-items: center;
  }
  .name {
    flex: 1;
    font-size: 1rem;
  }
  .enabled {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.88rem;
    color: #b6b6ba;
    white-space: nowrap;
  }
  h3 {
    font-size: 0.9rem;
    margin: 0 0 0.6rem;
    color: #c6c6ca;
  }
  .items {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.35rem 0.6rem;
    background: #1a1a1c;
    border: 1px solid #232327;
    border-radius: 6px;
    font-size: 0.85rem;
  }
  .item.on {
    border-color: #3a5b9a;
    background: #16203a;
  }
  .error {
    color: #ff9b9b;
    font-size: 0.82rem;
    margin: 0;
  }
  .actions {
    display: flex;
    gap: 0.5rem;
  }
  .primary {
    background: #1f5e38;
    border-color: #2a7d4b;
    color: #d8f3e4;
  }
  .danger {
    margin-left: auto;
    background: #3a1717;
    border-color: #5e2a2a;
    color: #f3d8d8;
  }
  .muted {
    color: #8a8a8e;
  }
</style>
