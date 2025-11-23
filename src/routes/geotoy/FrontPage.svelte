<script lang="ts">
  import { resolve } from '$app/paths';
  import {
    logout,
    type Composition,
    type CompositionVersion,
    type User,
  } from 'src/geoscript/geotoyAPIClient';

  export let me: User | null;
  export let featuredCompositions: { comp: Composition; latest: Pick<CompositionVersion, 'thumbnail_url'> }[];
  export let currentPage: number;
  export let hasMore: boolean;

  let isMenuOpen = false;

  function toggleMenu() {
    isMenuOpen = !isMenuOpen;
  }

  $: prevPageUrl =
    currentPage > 1
      ? currentPage === 2
        ? ('/geotoy' as const)
        : (`/geotoy?page=${currentPage - 1}` as const)
      : null;

  $: nextPageUrl = hasMore ? (`/geotoy?page=${currentPage + 1}` as const) : null;
</script>

<div class="root">
  <header class="header">
    <h1 class="title">geotoy</h1>

    <div class="desktop-nav">
      <a href={resolve('/geotoy/edit')}>new</a>
      <a href={resolve('/geotoy/docs')}>docs</a>
      {#if me}
        <span style="border-left: 1px solid #444; padding-left: 24px; margin-left: 8px;">
          logged in as <a href={resolve(`/geotoy/user/${me.id}`)}>{me.username}</a>
        </span>

        <div>
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_missing_attribute -->
          <a
            style="cursor: pointer; font-size: 12px; margin-left: 8px; margin-right: -4px;"
            on:click={() => logout().then(() => window.location.reload())}
            role="button"
            tabindex="0"
          >
            logout
          </a>
        </div>
      {:else}
        <a href={resolve('/geotoy/login')}>login/register</a>
      {/if}
    </div>

    <button class="hamburger" on:click={toggleMenu} aria-label="menu">
      <div class="bar"></div>
      <div class="bar"></div>
      <div class="bar"></div>
    </button>

    {#if isMenuOpen}
      <div class="mobile-nav">
        {#if me}
          <span>
            logged in as <a style="padding: 0 !important" href={resolve(`/geotoy/user/${me.id}`)}>
              {me.username}
            </a>
          </span>
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_missing_attribute -->
          <a
            style="cursor: pointer;"
            on:click={() => logout().then(() => window.location.reload())}
            role="button"
            tabindex="0"
          >
            logout
          </a>
        {:else}
          <a href={resolve('/geotoy/login')}>login/register</a>
        {/if}
        <a href={resolve('/geotoy/edit')}>new</a>
        <a href={resolve('/geotoy/docs')}>docs</a>
      </div>
    {/if}
  </header>

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

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 8px 6px 8px;
    background: none;
    position: relative;
    z-index: 10;
    gap: 16px;
    border-bottom: 1px solid #282828;
    margin-left: -8px;
    margin-right: -8px;
  }

  .title {
    margin-top: -2px;
  }

  .desktop-nav {
    display: flex;
    gap: 16px;
    align-items: center;
  }

  .hamburger {
    display: none;
    flex-direction: column;
    justify-content: space-around;
    width: 24px;
    height: 24px;
    background: transparent;
    border: none;
    cursor: pointer;
    padding: 0;
    z-index: 11;
    margin-right: 2px;
  }

  .hamburger .bar {
    width: 24px;
    height: 2px;
    background-color: #f0f0f0;
    display: block;
  }

  .mobile-nav {
    display: flex;
    flex-direction: column;
    position: absolute;
    top: 100%;
    right: 0;
    background: #0d0d0d;
    border: 1px solid #282828;
    border-top: none;
    gap: 0;
    z-index: 5;
    align-items: flex-start;
    min-width: 160px;
  }

  .mobile-nav a,
  .mobile-nav span {
    font-size: 16px;
    display: block;
    width: 100%;
    border-bottom: 1px solid #232323;
    padding: 5px 8px;
    margin: 0;
    box-sizing: border-box;
  }

  .mobile-nav a:last-child,
  .mobile-nav span:last-child {
    border-bottom: none;
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
    .desktop-nav {
      display: none;
    }

    .hamburger {
      display: flex;
    }

    .header {
      padding: 4px;
      gap: 4px;
    }

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
