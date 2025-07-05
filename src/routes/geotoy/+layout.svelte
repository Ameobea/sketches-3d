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
    color: #f0f0f0;
  }

  :global(h1) {
    font-size: 28px;
    line-height: 1;
    margin: 1px 0;
    font-weight: 400;
    text-align: center;
  }

  :global(h2) {
    font-size: 24px;
    line-height: 1;
    margin: 8px 0;
    font-weight: 400;
    text-align: center;
  }

  :global(a) {
    color: #7ecfff;
    text-decoration: underline;
    transition: color 0.2s;
  }

  :global(a:hover) {
    color: #fff;
    text-decoration: underline;
  }
</style>
