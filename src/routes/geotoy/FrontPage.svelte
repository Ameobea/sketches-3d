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
</script>

<div class="root">
  <header class="header">
    <h1 class="title">geotoy</h1>
    <div class="login-register">
      {#if me}
        <span>logged in as {me.username}</span>
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_missing_attribute -->
        <a
          style="cursor: pointer;"
          onclick={() => logout().then(() => window.location.reload())}
          role="button"
          tabindex="0"
        >
          logout
        </a>
      {:else}
        <a href="/geotoy/login">login/register</a>
      {/if}
    </div>
  </header>

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
        author: <a href={`/geotoy/user/${composition.comp.author_id}`}>{composition.comp.author_username}</a>
      </div>
    </div>
  {/each}
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
    gap: 8px;
    flex-wrap: wrap;
    padding: 0 8px 8px 8px;
    box-sizing: border-box;
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 4px 8px 6px 8px;
    background: none;
    position: relative;
    z-index: 1;
    gap: 16px;
    border-bottom: 1px solid #282828;
  }

  .title {
    margin: 0 auto;
    flex: 1;
    text-align: center;
  }

  .login-register {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    font-size: 14px;
    position: static;
    top: unset;
    right: unset;
    min-width: 120px;
  }

  .composition-tile {
    border: 1px solid #ccc;
    padding: 0px 2px 8px 2px;
    display: flex;
    flex-direction: column;
    max-width: 400px;

    .composition-title {
      font-weight: bold;
      font-size: 24px;
      text-align: center;
      margin-bottom: 4px;
    }

    .composition-thumbnail {
      width: 392px;
      height: 392px;
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

    a {
      text-decoration: none;
    }
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
    .header {
      padding: 4px;
      gap: 4px;
      justify-content: flex-start;
    }

    .title {
      margin: 0;
      text-align: left;
      margin-top: -4px;
    }

    footer {
      flex-direction: column;
      gap: 4px;
      text-align: center;
    }
  }
</style>
