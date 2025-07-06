<script lang="ts">
  import type { PageData } from './$types';

  let { data }: { data: PageData } = $props();
</script>

<div class="root">
  <h2>{data.user.username}</h2>
  TODO
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
  }
</style>
