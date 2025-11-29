<script lang="ts">
  import { resolve } from '$app/paths';
  import type { Composition, CompositionVersion, User } from 'src/geoscript/geotoyAPIClient';
  import GeotoyHeader from './GeotoyHeader.svelte';

  let {
    me,
    featuredCompositions,
    currentPage,
    hasMore,
  }: {
    me: User | null;
    featuredCompositions: { comp: Composition; latest: Pick<CompositionVersion, 'thumbnail_url'> }[];
    currentPage: number;
    hasMore: boolean;
  } = $props();

  let prevPageUrl = $derived(
    currentPage > 1
      ? currentPage === 2
        ? ('/geotoy' as const)
        : (`/geotoy?page=${currentPage - 1}` as const)
      : null
  );

  let nextPageUrl = $derived(hasMore ? (`/geotoy?page=${currentPage + 1}` as const) : null);
</script>

<div class="root">
  <GeotoyHeader {me} />

  <div class="compositions-grid">
    {#each featuredCompositions as composition (composition.comp.id)}
      <div class="composition-tile">
        <div class="composition-title">
          <a href={resolve(`/geotoy/edit/${composition.comp.id}`)}>{composition.comp.title}</a>
        </div>
        {#if composition.latest.thumbnail_url}
          <a href={resolve(`/geotoy/edit/${composition.comp.id}`)}>
            <img
              src={composition.latest.thumbnail_url}
              alt={composition.comp.description}
              class="composition-thumbnail"
              crossorigin="anonymous"
              loading="lazy"
            />
          </a>
        {:else}
          <div
            class="composition-thumbnail"
            style="display: flex; flex-direction: column; justify-content: center; align-items: center; text-align: center; gap: 8px; background: #222 repeating-linear-gradient(-45deg, transparent, transparent 20px, #181818 20px, #181818 40px);"
          >
            <div>Thumbnail generating or not available</div>
            <div style="font-size: 16px; margin-top: 4px;">
              <a href={resolve(`/geotoy/edit/${composition.comp.id}`)}>Open</a>
            </div>
            <div style="color: #bbb; font-size: 14px; margin-top: 4px; font-style: italic;">
              {composition.comp.description}
            </div>
          </div>
        {/if}
        <div class="composition-author">
          author:
          <a href={resolve(`/geotoy/user/${composition.comp.author_id}`)}>
            {composition.comp.author_username}
          </a>
        </div>
      </div>
    {/each}
  </div>

  {#if currentPage > 1 || hasMore}
    <div class="pagination">
      {#if prevPageUrl}
        <a href={resolve(prevPageUrl)} class="pagination-button" aria-label="Previous page">← Previous</a>
      {:else}
        <span class="pagination-button disabled" aria-label="Previous page (disabled)">← Previous</span>
      {/if}
      <span class="page-indicator">Page {currentPage}</span>
      {#if nextPageUrl}
        <a href={resolve(nextPageUrl)} class="pagination-button" aria-label="Next page">Next →</a>
      {:else}
        <span class="pagination-button disabled" aria-label="Next page (disabled)">Next →</span>
      {/if}
    </div>
  {/if}

  <footer>
    <span>
      Geoscript and Geotoy by <a target="_blank" href="https://cprimozic.net">Casey Primozic</a>
    </span>
    <span><a target="_blank" href="https://github.com/ameobea/sketches-3d">100% Free + Open Source</a></span>
    <span><a href={resolve('/geotoy/credits')}>Credits + Acknowledgements</a></span>
  </footer>
</div>

<style lang="css">
  .root {
    min-height: 100vh;
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 0 8px 8px 8px;
    box-sizing: border-box;
  }

  .compositions-grid {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 8px;
  }

  .composition-tile {
    border: 1px solid #ccc;
    padding: 4px;
    display: flex;
    flex-direction: column;
    width: 100%;
    max-width: 400px;
    box-sizing: border-box;
  }

  .composition-title {
    font-weight: 400;
    font-size: 20px;
    text-align: center;
    margin-bottom: 4px;
  }

  .composition-thumbnail {
    width: 100%;
    height: auto;
    border: 1px solid #444;
    margin-bottom: 2px;
    aspect-ratio: 1;
    object-fit: cover;
    display: block;
  }

  .composition-author {
    margin-bottom: -2px;
    font-size: 14px;
  }

  .pagination {
    display: flex;
    justify-content: center;
    align-items: center;
    gap: 16px;
    padding: 16px 8px;
    margin-top: 8px;
  }

  .pagination-button {
    background: #1a1a1a;
    border: 1px solid #444;
    color: #f0f0f0;
    padding: 4px 8px;
    cursor: pointer;
    font-size: 15px;
    text-decoration: none;
    display: inline-block;
  }

  .pagination-button:hover:not(.disabled) {
    background: #2a2a2a;
    border-color: #666;
  }

  .pagination-button.disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .page-indicator {
    font-size: 15px;
    color: #ccc;
    min-width: 80px;
    text-align: center;
  }

  footer {
    margin-top: auto;
    display: flex;
    justify-content: space-around;
    align-items: center;
    padding: 4px 8px 0 8px;
    margin-left: -8px;
    margin-right: -8px;
    color: #ccc;
    font-size: 13px;
    border-top: 1px solid #282828;
    gap: 12px;
  }

  @media (max-width: 600px) {
    .pagination {
      gap: 8px;
      padding: 12px 4px;
    }

    .pagination-button {
      padding: 8px 12px;
      font-size: 14px;
    }

    .page-indicator {
      min-width: 60px;
      font-size: 14px;
    }

    footer {
      flex-direction: column;
      gap: 4px;
      text-align: center;
    }
  }
</style>
