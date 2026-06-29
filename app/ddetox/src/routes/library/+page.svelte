<script lang="ts">
  import LibraryEditor from "$lib/components/LibraryEditor.svelte";
</script>

<h1>Website library</h1>
<p class="lede">
  Reusable sites you can drop into sessions. Each site bundles its domains (and
  optional addresses). Editing here is local to the app — it only affects the
  daemon when a session that uses the site is active.
</p>

<LibraryEditor />

<style>
  .lede {
    margin: 0;
    color: #9a9a9e;
    line-height: 1.5;
  }
</style>
