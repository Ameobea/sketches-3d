<script lang="ts">
  import type { SettingAction } from './ControlPanel/types';

  let { actions }: { actions: SettingAction[] } = $props();

  let root = $state<HTMLElement | null>(null);
  // Fixed-positioned so it escapes the scrolling panel's overflow clip.
  let pop = $state<{ left: number; top: number } | null>(null);

  const toggle = (e: MouseEvent) => {
    if (pop) {
      pop = null;
      return;
    }
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    pop = { left: r.right, top: r.bottom + 2 };
  };

  $effect(() => {
    if (!pop) return;
    const close = (e: Event) => {
      if (!root?.contains(e.target as Node)) pop = null;
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') pop = null;
    };
    window.addEventListener('pointerdown', close, true);
    window.addEventListener('keydown', onKey, true);
    return () => {
      window.removeEventListener('pointerdown', close, true);
      window.removeEventListener('keydown', onKey, true);
    };
  });
</script>

{#if actions.length > 0}
  <div class="row-menu" bind:this={root}>
    <button class="row-menu-btn" type="button" title="actions" class:open={!!pop} onclick={toggle}>⋯</button>
    {#if pop}
      <div class="row-menu-pop" style:left="{pop.left}px" style:top="{pop.top}px">
        {#each actions as a (a.label)}
          <button
            class="row-menu-item"
            type="button"
            disabled={a.disabled}
            title={a.title}
            onclick={() => {
              pop = null;
              a.action();
            }}
          >
            {a.label}
          </button>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .row-menu {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
  }

  .row-menu-btn {
    width: 16px;
    height: 16px;
    padding: 0;
    border: none;
    background: transparent;
    color: #8a8a8a;
    font: inherit;
    line-height: 1;
    cursor: pointer;
  }
  .row-menu-btn:hover,
  .row-menu-btn.open {
    color: #eee;
  }

  .row-menu-pop {
    position: fixed;
    z-index: 50;
    transform: translateX(-100%);
    min-width: 150px;
    background: #1e1e1e;
    border: 1px solid #555;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.6);
    font-size: 11px;
  }

  .row-menu-item {
    display: block;
    width: 100%;
    padding: 4px 8px;
    border: none;
    background: transparent;
    color: #ddd;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .row-menu-item:hover:not(:disabled) {
    background: #333;
  }
  .row-menu-item:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
