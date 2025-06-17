<script lang="ts">
  import type { EditorView } from 'codemirror';
  import { onMount } from 'svelte';

  import { buildEditor } from 'src/viz/scenes/geoscriptPlayground/editor';
  import quickstartCode from './quickstart.geo?raw';

  let codemirrorContainer = $state<HTMLDivElement | null>(null);
  let editorView = $state<EditorView | null>(null);

  onMount(() => {
    const editor = buildEditor({
      container: codemirrorContainer!,
      customKeymap: [],
      initialCode: quickstartCode,
      readonly: true,
    });
    editorView = editor.editorView;
  });
</script>

<div class="root">
  <div class="codemirror-container" bind:this={codemirrorContainer}></div>
</div>

<style lang="css">
  :global(body) {
    margin: 0;
    padding: 0;
    background-color: #282828;
  }

  .root {
    display: flex;
    flex-direction: column;
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
  }
</style>
