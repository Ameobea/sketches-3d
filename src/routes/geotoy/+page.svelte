<script lang="ts">
  import type { PageData } from './$types';
  import FrontPage from './FrontPage.svelte';

  let { data }: { data: PageData } = $props();
</script>

<div class="root">
  <FrontPage featuredCompositions={data.featuredCompositions} />
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
  }
</style>
