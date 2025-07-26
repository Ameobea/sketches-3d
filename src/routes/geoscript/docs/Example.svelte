<script lang="ts">
  import type { EditorView } from 'codemirror';
  import type { PopulatedFnExample } from './types';
  import { buildEditor } from 'src/geoscript/editor';

  let { example }: { example: PopulatedFnExample } = $props();

  let open = $state(false);

  let codemirrorContainer = $state<HTMLDivElement | null>(null);
  let editorView = $state<EditorView | null>(null);

  $effect(() => {
    if (open) {
      if (editorView || !codemirrorContainer) {
        return;
      }

      const editor = buildEditor({
        container: codemirrorContainer,
        initialCode: example.version.source_code,
        readonly: true,
      });
      editorView = editor.editorView;
    } else {
      if (editorView) {
        editorView.destroy();
        editorView = null;
      }
    }
  });
</script>

<div class="root">
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <details bind:open>
    <summary>Example</summary>
    <div class="details-content">
      <div class="thumbnail-container">
        <a href={`/geotoy/edit/${example.composition.id}`}>
          <img
            src={example.version.thumbnail_url}
            alt={example.composition.description}
            crossorigin="anonymous"
            loading="lazy"
          />
        </a>
      </div>
      <div class="code-container">
        {#if open}
          <div class="codemirror-container" bind:this={codemirrorContainer}></div>
        {:else}
          <code>{example.version.source_code}</code>
        {/if}
      </div>
    </div>
    <div class="edit-link-wrapper">
      <p class="edit-link"><a href={`/geotoy/edit/${example.composition.id}`}>Open in editor</a></p>
    </div>
  </details>
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
    margin-bottom: 16px;
    border: 1px solid #555;
  }

  summary {
    cursor: pointer;
    font-weight: 600;
    padding: 4px 10px;
  }

  .code-container {
    max-height: calc(min(60vh, 800px));
    max-width: calc(min(100vw, 500px));
    overflow: auto;
  }

  .details-content {
    display: flex;
    flex-direction: row-reverse;
    flex-wrap: wrap;
  }

  .thumbnail-container {
    display: flex;
    flex: 1;
    align-items: center;

    img {
      aspect-ratio: 1;
      width: 100%;
      height: auto;
      object-fit: cover;
    }
  }

  .edit-link-wrapper {
    border-top: 1px solid #555;

    .edit-link {
      padding: 10px;
      margin: 0;

      a {
        color: #f0f0f0;
        text-decoration: underline;
      }

      a:hover {
        color: #cfcfcf;
      }
    }
  }

  .codemirror-container {
    height: 100%;
  }

  :global(.cm-editor) {
    height: 100%;
  }
</style>
