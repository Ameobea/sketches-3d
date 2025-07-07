<script lang="ts">
  import {
    logout,
    type Composition,
    type CompositionVersion,
    type User,
  } from 'src/geoscript/geotoyAPIClient';
  import { getProxiedThumbnailURL } from './utils';

  export let me: User | null;
  export let featuredCompositions: { comp: Composition; latest: CompositionVersion }[];

  let isMenuOpen = false;

  function toggleMenu() {
    isMenuOpen = !isMenuOpen;
  }
</script>

<div class="root">
  <header class="header">
    <h1 class="title"><a href="/geotoy">geotoy</a></h1>

    <div class="desktop-nav">
      <a href="/geotoy/edit">new</a>
      <!-- svelte-ignore a11y_invalid_attribute -->
      <a href="/geotoy/docs">docs</a>
      {#if me}
        <span style="border-left: 1px solid #444; padding-left: 24px; margin-left: 8px;">
          logged in as <a href={`/geotoy/user/${me.id}`}>{me.username}</a>
        </span>

        <div>
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_missing_attribute -->
          <a
            style="cursor: pointer; font-size: 12px; margin-left: 8px; margin-right: -8px;"
            on:click={() => logout().then(() => window.location.reload())}
            role="button"
            tabindex="0"
          >
            logout
          </a>
        </div>
      {:else}
        <a href="/geotoy/login">login/register</a>
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
            logged in as <a style="padding: 0 !important" href={`/geotoy/user/${me.id}`}>{me.username}</a>
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
          <a href="/geotoy/login">login/register</a>
        {/if}
        <a href="/geotoy/edit">new</a>
        <!-- svelte-ignore a11y_invalid_attribute -->
        <a href="/geotoy/docs">docs</a>
      </div>
    {/if}
  </header>

  <div class="compositions-grid">
    {#each featuredCompositions as composition}
      <div class="composition-tile">
        <div class="composition-title">
          <a href={`/geotoy/edit/${composition.comp.id}`}>{composition.comp.title}</a>
        </div>
        {#if composition.latest.thumbnail_url}
          <a href={`/geotoy/edit/${composition.comp.id}`}>
            <img
              src={getProxiedThumbnailURL(composition.latest.thumbnail_url)}
              alt={composition.comp.description}
              class="composition-thumbnail"
            />
          </a>
        {:else}
          <div
            class="composition-thumbnail"
            style="display: flex; flex-direction: column; justify-content: center; align-items: center; text-align: center; gap: 8px; background: #222 repeating-linear-gradient(-45deg, transparent, transparent 20px, #181818 20px, #181818 40px);"
          >
            <div>Thumbnail generating or not available</div>
            <div style="font-size: 16px; margin-top: 4px;">
              <a href={`/geotoy/edit/${composition.comp.id}`}>Open</a>
            </div>
            <div style="color: #bbb; font-size: 14px; margin-top: 4px; font-style: italic;">
              {composition.comp.description}
            </div>
          </div>
        {/if}
        <div class="composition-author">
          author:
          <a href={`/geotoy/user/${composition.comp.author_id}`}>{composition.comp.author_username}</a>
        </div>
      </div>
    {/each}
  </div>
  <footer>
    <span>
      Geoscript and Geotoy by <a target="_blank" href="https://cprimozic.net">Casey Primozic</a>
    </span>
    <span><a target="_blank" href="https://github.com/ameobea/sketches-3d">100% Free + Open Source</a></span>
    <span><a href="/geotoy/credits">Credits + Acknowledgements</a></span>
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
  }

  .title a {
    text-decoration: none;
    color: inherit;
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

  .composition-tile a {
    text-decoration: none;
  }

  .composition-title {
    font-weight: bold;
    font-size: 24px;
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
    margin-bottom: -5px;
    font-size: 14px;
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

    footer {
      flex-direction: column;
      gap: 4px;
      text-align: center;
    }
  }
</style>
