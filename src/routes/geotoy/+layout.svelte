<script lang="ts">
  let { children } = $props();
</script>

{@render children()}

<style>
  @import url('https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600;700&display=swap');

  :global(body) {
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    margin: 0;
    padding: 0;
    background: #141414;
  }
</style>
