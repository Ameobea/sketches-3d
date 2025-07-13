<script lang="ts">
  import { onMount } from 'svelte';

  import { buildEditor } from 'src/geoscript/editor';
  import quickstartCode from './quickstart.geo?raw';

  let codemirrorContainer = $state<HTMLDivElement | null>(null);

  onMount(() => {
    buildEditor({
      container: codemirrorContainer!,
      customKeymap: [],
      initialCode: quickstartCode,
      readonly: true,
      lineNumbers: false,
    });
  });
</script>

<div class="wrapper">
  <div class="root">
    <p>
      Geoscript is a DSL for procedural 3D geometry generation + manipulation. It's a functional-inspired
      language designed to have a concise and convenient syntax and influences from Rust, Python, TypeScript,
      and F#.
    </p>
    <div class="codemirror-container" bind:this={codemirrorContainer}></div>
  </div>
</div>

<style lang="css">
  :global(html) {
    overflow-y: auto;
  }

  :global(body) {
    margin: 0;
    padding: 0;
  }

  :global(.cm-gutters) {
    display: none !important;
  }

  :global(.cm-editor) {
    background: #1a1a1a !important;
    background-color: #1a1a1a !important;
  }

  :global(.cm-content) {
    padding-top: 0 !important;
  }

  .wrapper {
    background-color: #141414;
    min-height: 100vh;
  }

  .root {
    display: flex;
    flex-direction: column;
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    color: #fcfcfc;
    max-width: 1200px;
    margin-left: auto;
    margin-right: auto;
  }

  p {
    font-size: 16px;
    padding-left: 8px;
    padding-right: 8px;
  }

  .codemirror-container {
    font-size: 18px;
    background: #242424;
  }

  @media (max-width: 600px) {
    .codemirror-container {
      font-size: 14px;
    }

    p {
      font-size: 14px;
    }
  }
</style>
