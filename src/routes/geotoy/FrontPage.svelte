<script lang="ts">
  import { logout, type Composition, type User } from 'src/geoscript/geoscriptAPIClient';

  export let me: User | null;
  export let featuredCompositions: Composition[];
</script>

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
<div class="root">
  {#each featuredCompositions as composition}
    <div class="composition-tile">
      <div class="composition-title"><a href={`/geotoy/edit/${composition.id}`}>{composition.title}</a></div>
      <div class="composition-description">{composition.description}</div>
      <div>
        author: <a href={`/geotoy/user/${composition.author_id}`}>{composition.author_username}</a>
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

  .root {
    min-height: calc(100vh - 38px);
    display: flex;
    flex-direction: column;
    gap: 8px;
    flex-wrap: wrap;
    padding: 8px;
    box-sizing: border-box;
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
    padding: 16px;
    display: flex;
    flex-direction: column;
    max-width: 400px;

    .composition-title {
      font-weight: bold;
      font-size: 24px;
      text-align: center;
      margin-bottom: 16px;
    }

    .composition-description {
      font-size: 14px;
      margin-bottom: 8px;
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
