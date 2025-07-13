<script lang="ts">
  let showMenu = false;

  const toggleMenu = () => {
    showMenu = !showMenu;
  };

  const handleClickOutside = (evt: MouseEvent) => {
    if (evt.target instanceof HTMLElement && !evt.target.closest('.actions-menu-container')) {
      showMenu = false;
    }
  };
</script>

<svelte:window on:click={handleClickOutside} />

<div class="actions-menu-container">
  <button onclick={toggleMenu} class="menu-button">☰</button>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  {#if showMenu}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="menu" onclick={toggleMenu}>
      <slot />
    </div>
  {/if}
</div>

<style>
  .actions-menu-container {
    position: relative;
  }

  .menu-button {
    background: none;
    border: none;
    color: #f0f0f0;
    font-size: 24px;
    cursor: pointer;
    padding: 0 8px;
    margin-right: -8px;
  }

  .menu {
    position: absolute;
    top: 100%;
    right: 0;
    background: #222;
    border: 1px solid #444;
    z-index: 10;
    min-width: 150px;
  }
</style>
