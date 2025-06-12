<script lang="ts">
  import type { PageData } from './$types';
  import FnDoc from './FnDoc.svelte';

  let { data }: { data: PageData } = $props();
  let entries = $derived(Object.entries(data.builtinFnDefs).sort(([a], [b]) => a.localeCompare(b)));
</script>

<div class="root">
  <div class="docs">
    {#each entries as [name, sigs]}
      <FnDoc {name} signatures={sigs} />
    {/each}
  </div>
</div>

<style lang="css">
  @import url('https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600;700&display=swap');

  .root {
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    display: flex;
    flex-direction: column;
    border: 0;
    padding: 0;
  }

  :global(body) {
    margin: 0;
    padding: 0;
    background: #141414;
  }

  .docs {
    display: flex;
    flex-direction: column;
    max-width: 960px;
    margin: 16px auto;
    padding: 16px 8px 8px 8px;
    background: #191818;
  }
</style>
