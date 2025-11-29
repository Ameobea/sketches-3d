<script lang="ts">
  import { resolve } from '$app/paths';
  import { logout, type User } from 'src/geoscript/geotoyAPIClient';

  let { me, showTitleLink = false }: { me: User | null; showTitleLink?: boolean } = $props();

  let isMenuOpen = $state(false);

  const toggleMenu = () => {
    isMenuOpen = !isMenuOpen;
  };

  const handleLogout = () => logout().then(() => window.location.reload());
</script>

<header class="header">
  {#if showTitleLink}
    <a href={resolve('/geotoy')} class="title-link"><h1 class="title">geotoy</h1></a>
  {:else}
    <h1 class="title">geotoy</h1>
  {/if}

  <div class="desktop-nav">
    <a href={resolve('/geotoy/edit')}>new</a>
    <a href={resolve('/geotoy/docs')}>docs</a>
    {#if me}
      <span class="logged-in-info">
        logged in as <a href={resolve(`/geotoy/user/${me.id}`)}>{me.username}</a>
      </span>
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_missing_attribute -->
      <a class="logout-link" onclick={handleLogout} role="button" tabindex="0">logout</a>
    {:else}
      <a href={resolve('/geotoy/login')}>login/register</a>
    {/if}
  </div>

  <button class="hamburger" onclick={toggleMenu} aria-label="menu">
    <div class="bar"></div>
    <div class="bar"></div>
    <div class="bar"></div>
  </button>

  {#if isMenuOpen}
    <div class="mobile-nav">
      {#if me}
        <span>
          logged in as <a class="inline-link" href={resolve(`/geotoy/user/${me.id}`)}>
            {me.username}
          </a>
        </span>
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_missing_attribute -->
        <a onclick={handleLogout} role="button" tabindex="0">logout</a>
      {:else}
        <a href={resolve('/geotoy/login')}>login/register</a>
      {/if}
      <a href={resolve('/geotoy/edit')}>new</a>
      <a href={resolve('/geotoy/docs')}>docs</a>
    </div>
  {/if}
</header>

<style>
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

  .title-link {
    text-decoration: none;
  }

  .title {
    font-size: 28px;
    line-height: 1;
    margin: 1px 0;
    font-weight: 400;
    text-align: center;
    color: #f0f0f0;
  }

  .title-link:hover .title {
    color: #fff;
    text-decoration: underline;
  }

  .desktop-nav {
    display: flex;
    gap: 16px;
    align-items: center;
  }

  .logged-in-info {
    border-left: 1px solid #444;
    padding-left: 24px;
    margin-left: 8px;
  }

  .logout-link {
    cursor: pointer;
    font-size: 12px;
    margin-left: 8px;
    margin-right: -4px;
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

  .mobile-nav .inline-link {
    display: inline;
    width: auto;
    border-bottom: none;
    padding: 0;
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
  }
</style>
