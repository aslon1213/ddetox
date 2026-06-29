<script lang="ts">
  import { getLibrary, saveLibrary, reconcileNow, newId } from "$lib/config";
  import { validateDomain, validateCidr } from "$lib/validate";
  import type { WebsiteItem } from "$lib/types";

  let items = $state<WebsiteItem[]>([]);
  let editingId = $state<string | null>(null);
  let label = $state("");
  let domainsText = $state("");
  let cidrsText = $state("");
  let error = $state<string | null>(null);
  let note = $state<string | null>(null);

  async function load() {
    try {
      items = await getLibrary();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }
  $effect(() => {
    load();
  });

  function lines(text: string): string[] {
    return text
      .split(/[\n,]/)
      .map((s) => s.trim())
      .filter((s) => s.length > 0);
  }

  function resetForm() {
    editingId = null;
    label = "";
    domainsText = "";
    cidrsText = "";
    error = null;
  }

  function edit(item: WebsiteItem) {
    editingId = item.id;
    label = item.label;
    domainsText = item.domains.join("\n");
    cidrsText = item.cidrs.join("\n");
    error = null;
  }

  async function persist(next: WebsiteItem[]) {
    items = next;
    await saveLibrary(next);
    try {
      await reconcileNow();
    } catch {
      // Daemon offline is soft; the background loop will catch up.
    }
  }

  async function save() {
    error = null;
    if (!label.trim()) {
      error = "Give the site a label.";
      return;
    }

    const domains: string[] = [];
    for (const raw of lines(domainsText)) {
      const v = validateDomain(raw);
      if (!v) {
        error = `Invalid domain: ${raw}`;
        return;
      }
      domains.push(v);
    }
    const cidrs: string[] = [];
    for (const raw of lines(cidrsText)) {
      const v = validateCidr(raw);
      if (!v) {
        error = `Invalid CIDR/IP: ${raw}`;
        return;
      }
      cidrs.push(v);
    }
    if (domains.length === 0 && cidrs.length === 0) {
      error = "Add at least one domain or address.";
      return;
    }

    const item: WebsiteItem = {
      id: editingId ?? newId(),
      label: label.trim(),
      domains,
      cidrs,
    };
    const next = editingId
      ? items.map((i) => (i.id === editingId ? item : i))
      : [...items, item];
    await persist(next);
    note = editingId ? "Updated." : "Added.";
    resetForm();
  }

  async function remove(id: string) {
    await persist(items.filter((i) => i.id !== id));
    if (editingId === id) resetForm();
  }
</script>

<section class="wrap">
  <div class="editor">
    <h2>{editingId ? "Edit site" : "New site"}</h2>
    <label>
      Label
      <input placeholder="e.g. Reddit" bind:value={label} />
    </label>
    <label>
      Domains <span class="hint">one per line; <code>*.</code> wildcards ok</span>
      <textarea rows="3" placeholder={"reddit.com\n*.reddit.com"} bind:value={domainsText}></textarea>
    </label>
    <label>
      Addresses (CIDR) <span class="hint">one per line; optional</span>
      <textarea rows="2" placeholder={"10.0.0.0/8"} bind:value={cidrsText}></textarea>
    </label>
    {#if error}<p class="error">{error}</p>{/if}
    <div class="actions">
      <button class="primary" onclick={save}>{editingId ? "Save changes" : "Add site"}</button>
      {#if editingId}<button onclick={resetForm}>Cancel</button>{/if}
    </div>
  </div>

  <div class="list">
    <h2>Library ({items.length})</h2>
    {#if items.length === 0}
      <p class="muted">No sites yet. Add one on the left.</p>
    {/if}
    {#each items as item (item.id)}
      <div class="row" class:editing={editingId === item.id}>
        <div class="info">
          <div class="label">{item.label}</div>
          <div class="sub">
            {item.domains.length} domain(s) · {item.cidrs.length} address(es)
          </div>
          <div class="entries">{[...item.domains, ...item.cidrs].join(", ")}</div>
        </div>
        <div class="rowactions">
          <button onclick={() => edit(item)}>Edit</button>
          <button class="ghost" onclick={() => remove(item.id)}>Delete</button>
        </div>
      </div>
    {/each}
  </div>
</section>

<style>
  .wrap {
    display: grid;
    grid-template-columns: 320px 1fr;
    gap: 1.5rem;
    align-items: start;
  }
  @media (max-width: 720px) {
    .wrap {
      grid-template-columns: 1fr;
    }
  }
  h2 {
    font-size: 1rem;
    margin: 0 0 0.8rem;
  }
  .editor {
    background: #161618;
    border-radius: 10px;
    padding: 1.1rem 1.2rem;
    position: sticky;
    top: 4rem;
  }
  label {
    display: block;
    font-size: 0.85rem;
    color: #b6b6ba;
    margin-bottom: 0.8rem;
  }
  label input,
  label textarea {
    display: block;
    width: 100%;
    box-sizing: border-box;
    margin-top: 0.3rem;
    resize: vertical;
  }
  .hint {
    color: #6f6f73;
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
  .error {
    color: #ff9b9b;
    font-size: 0.82rem;
    margin: 0 0 0.6rem;
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .row {
    display: flex;
    justify-content: space-between;
    gap: 0.8rem;
    padding: 0.7rem 0.9rem;
    background: #1a1a1c;
    border-radius: 8px;
    border: 1px solid transparent;
  }
  .row.editing {
    border-color: #3a5b9a;
  }
  .label {
    font-weight: 600;
  }
  .sub {
    font-size: 0.78rem;
    color: #8a8a8e;
    margin-top: 0.1rem;
  }
  .entries {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.78rem;
    color: #9a9a9e;
    margin-top: 0.3rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 42ch;
  }
  .rowactions {
    display: flex;
    gap: 0.4rem;
    align-items: flex-start;
    flex: 0 0 auto;
  }
  .muted {
    color: #8a8a8e;
  }
</style>
