<script lang="ts">
  import { resolve } from '$app/paths';
  import type { EditorView } from 'codemirror';
  import { buildEditor } from 'src/geoscript/editor';

  let { data } = $props();

  let codemirrorContainer = $state<HTMLDivElement | null>(null);
  let editorView = $state<EditorView | null>(null);

  let editUrl = $derived(`/geotoy/edit/${data.composition.id}` as const);

  $effect(() => {
    if (editorView || !codemirrorContainer) {
      return;
    }

    const editor = buildEditor({
      container: codemirrorContainer,
      initialCode: data.version.source_code,
      readonly: true,
    });
    editorView = editor.editorView;
  });
</script>

<svelte:head>
  <title>{data.composition.title} — Geotoy Embed</title>
</svelte:head>

<div class="embed-root">
  {#if data.showTitle || data.showAuthor || data.showDescription}
    <div class="metadata">
      {#if data.showTitle}
        <a href={resolve(editUrl)} class="title">{data.composition.title}</a>
      {/if}
      {#if data.showAuthor}
        <span class="author">by {data.composition.author_username}</span>
      {/if}
      {#if data.showDescription && data.composition.description}
        <span class="description">{data.composition.description}</span>
      {/if}
    </div>
  {/if}

  <div class="content">
    <div class="code-panel">
      <div class="codemirror-container" bind:this={codemirrorContainer}></div>
    </div>
    <div class="thumbnail-panel">
      <a href={resolve(editUrl)}>
        {#if data.version.thumbnail_url}
          <img src={data.version.thumbnail_url} alt={data.composition.title} crossorigin="anonymous" />
        {:else}
          <div class="no-thumbnail">No preview</div>
        {/if}
      </a>
    </div>
  </div>
</div>

<style lang="css">
  .embed-root {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
    background: #0d0d0d;
  }

  .metadata {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px 12px;
    border-bottom: 1px solid #333;
    flex-shrink: 0;
  }

  .title {
    font-size: 14px;
    font-weight: 600;
    color: #e0e0e0;
    text-decoration: none;
  }

  .title:hover {
    color: #fff;
    text-decoration: underline;
  }

  .author {
    font-size: 11px;
    color: #888;
  }

  .description {
    font-size: 11px;
    color: #999;
  }

  .content {
    display: flex;
    flex-direction: row;
    flex: 1;
    min-height: 0;
  }

  .code-panel {
    flex: 1;
    min-width: 0;
    overflow: auto;
  }

  .codemirror-container {
    height: 100%;
  }

  :global(.embed-root .cm-editor) {
    height: 100%;
  }

  .thumbnail-panel {
    flex-shrink: 0;
    width: 280px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-left: 1px solid #333;
    background: #1a1a1a;
  }

  .thumbnail-panel a {
    display: block;
    width: 100%;
  }

  .thumbnail-panel img {
    width: 100%;
    aspect-ratio: 1;
    object-fit: cover;
    display: block;
  }

  .no-thumbnail {
    aspect-ratio: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #666;
    font-size: 12px;
  }

  @media (max-width: 500px) {
    .content {
      flex-direction: column-reverse;
    }

    .thumbnail-panel {
      width: 100%;
      border-left: none;
      border-bottom: 1px solid #333;
    }

    .thumbnail-panel img {
      aspect-ratio: 16 / 9;
    }
  }
</style>
