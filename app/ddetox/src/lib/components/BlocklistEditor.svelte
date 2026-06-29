<script lang="ts">
  import type { Status } from "$lib/api";
  import { addDomains, removeDomains, addAddrs, removeAddrs } from "$lib/api";
  import { validateDomain, validateCidr } from "$lib/validate";

  let {
    status,
    locked,
    onChanged,
    onError,
  }: {
    status: Status | null;
    /** True while a committed session locks removals (daemon is the real gate). */
    locked: boolean;
    /** Re-fetch authoritative status from the daemon after a mutation. */
    onChanged: () => Promise<void>;
    onError: (msg: string) => void;
  } = $props();

  let domainInput = $state("");
  let cidrInput = $state("");
  let domainHint = $state<string | null>(null);
  let cidrHint = $state<string | null>(null);
  let busy = $state(false);

  // Run a mutation, then reconcile against the daemon. Never trust local state.
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

  async function onDomain(add: boolean) {
    const normalized = validateDomain(domainInput);
    if (!normalized) {
      domainHint =
        "Enter a valid domain: example.com, *.example.com (subdomains), or =example.com (exact)";
      return;
    }
    domainHint = null;
    domainInput = "";
    await run(() => (add ? addDomains([normalized]) : removeDomains([normalized])));
  }

  async function onCidr(add: boolean) {
    const normalized = validateCidr(cidrInput);
    if (!normalized) {
      cidrHint = "Enter a valid IP or CIDR, e.g. 10.0.0.0/8 or 1.2.3.4";
      return;
    }
    cidrHint = null;
    cidrInput = "";
    await run(() => (add ? addAddrs([normalized]) : removeAddrs([normalized])));
  }
</script>

<section class="editor">
  <div class="col">
    <div class="head">
      <h2>Domains</h2>
      <span class="count">{status?.blocked_domains ?? 0} blocked</span>
    </div>
    <form
      class="addrow"
      onsubmit={(e) => {
        e.preventDefault();
        onDomain(true);
      }}
    >
      <input
        placeholder="example.com · *.example.com · =example.com"
        bind:value={domainInput}
        oninput={() => (domainHint = null)}
        disabled={busy}
      />
      <button type="submit" class="primary" disabled={busy || !domainInput.trim()}>Add</button>
      <button
        type="button"
        class="ghost"
        title={locked ? "Locked during committed session" : "Remove this entry"}
        disabled={busy || locked || !domainInput.trim()}
        onclick={() => onDomain(false)}
      >
        Remove
      </button>
    </form>
    {#if domainHint}
      <p class="hint">{domainHint}</p>
    {/if}
  </div>

  <div class="col">
    <div class="head">
      <h2>Addresses (CIDR)</h2>
      <span class="count">{status?.blocked_cidrs ?? 0} blocked</span>
    </div>
    <form
      class="addrow"
      onsubmit={(e) => {
        e.preventDefault();
        onCidr(true);
      }}
    >
      <input
        placeholder="10.0.0.0/8 or 1.2.3.4"
        bind:value={cidrInput}
        oninput={() => (cidrHint = null)}
        disabled={busy}
      />
      <button type="submit" class="primary" disabled={busy || !cidrInput.trim()}>Add</button>
      <button
        type="button"
        class="ghost"
        title={locked ? "Locked during committed session" : "Remove this entry"}
        disabled={busy || locked || !cidrInput.trim()}
        onclick={() => onCidr(false)}
      >
        Remove
      </button>
    </form>
    {#if cidrHint}
      <p class="hint">{cidrHint}</p>
    {/if}
  </div>

  <p class="note">
    Domain forms: <code>example.com</code> blocks the host and all subdomains,
    <code>*.example.com</code> blocks subdomains only, and
    <code>=example.com</code> blocks the exact host. The daemon reports counts
    only — individual entries aren't listed; counts refresh after each change.
  </p>
</section>

<style>
  .editor {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 1.5rem;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 1.15rem 1.25rem;
    box-shadow: var(--shadow);
  }
  @media (max-width: 640px) {
    .editor {
      grid-template-columns: 1fr;
    }
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    margin-bottom: 0.6rem;
  }
  h2 {
    font-size: 1rem;
    margin: 0;
  }
  .count {
    font-size: 0.8rem;
    color: var(--text-dim);
    font-variant-numeric: tabular-nums;
  }
  .addrow {
    display: flex;
    gap: 0.5rem;
  }
  .addrow input {
    flex: 1;
    min-width: 0;
  }
  .ghost {
    background: transparent;
  }
  .hint {
    margin: 0.4rem 0 0;
    font-size: 0.8rem;
    color: #e6a23c;
  }
  .note {
    grid-column: 1 / -1;
    margin: 0;
    font-size: 0.8rem;
    color: var(--text-faint);
    line-height: 1.5;
  }
  .note code {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.76rem;
    color: var(--accent-soft-text);
    background: var(--accent-soft);
    padding: 0.05rem 0.3rem;
    border-radius: 4px;
  }
</style>
