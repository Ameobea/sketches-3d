<script lang="ts">
  import { page } from '$app/state';
</script>

<div class="root">
  <h1>{page.status}</h1>
  <p style="color: red">{page.error?.message}</p>
  <a href="/geotoy">back to home</a>
</div>

<style>
  .root {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 8px;
    gap: 20px;
  }
</style>
