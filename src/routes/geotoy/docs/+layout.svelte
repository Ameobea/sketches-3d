<script lang="ts">
  import './docs.css';

  export const prerender = true;
</script>

<div class="root">
  <slot />

  <div style="text-align: center; margin-top: 2em; font-size: 0.9em; color: #888">
    <a href="/geotoy">back to home</a>
  </div>
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
    height: 100vh;
    padding: 16px;
    max-width: 1000px;
    margin: auto;
    line-height: 2;
  }
</style>
