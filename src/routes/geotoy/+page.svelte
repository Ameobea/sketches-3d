<script lang="ts">
  import FrontPage from './FrontPage.svelte';
  import type { PageData } from './$types';

  let { data }: { data: PageData } = $props();
</script>

<div class="root">
  <FrontPage featuredCompositions={data.featuredCompositions} me={data.me} />
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
  }
</style>
