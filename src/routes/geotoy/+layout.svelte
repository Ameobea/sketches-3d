<script lang="ts">
  let { children } = $props();
</script>

{@render children()}

<style>
  @import url('https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600;700&display=swap');

  :global(html) {
    box-sizing: border-box;
  }
  :global(*, *:before, *:after) {
    box-sizing: inherit;
  }

  :global(body) {
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    margin: 0;
    padding: 0;
    background: #141414;
    color: #f0f0f0;
    box-sizing: border-box;
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

  :global(input[type='text']),
  :global(input[type='password']),
  :global(input[type='email']),
  :global(input[type='number']) {
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    font-size: 13px;
    border: 1px solid #ccc;
    box-sizing: border-box;
    background: #222;
    color: #f0f0f0;
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

  :global(button) {
    background: #333;
    color: #f0f0f0;
    border: 1px solid #888;
    border-radius: 0;
    cursor: pointer;
    font-size: 14px;
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
  }

  :global(button:hover) {
    background: #262626;
    color: #fff;
  }

  :global(button:active) {
    background: #444;
    color: #fff;
  }

  :global(button:disabled) {
    background: #444;
    color: #aaa;
    cursor: default;
  }
</style>
