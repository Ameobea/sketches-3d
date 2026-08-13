<script lang="ts">
  import type { MenuSection } from 'src/geotoy/modes/mode';
  import { dismissOn } from './dismissOn';

  export interface Menu {
    title: string;
    sections: MenuSection[];
  }

  let {
    menus,
    title,
    barHeight,
    compact = false,
  }: {
    menus: Menu[];
    title: string;
    barHeight: number;
    /** Single-`☰` fallback; carries the same tree, so every action stays reachable. */
    compact?: boolean;
  } = $props();

  // The fallback is one menu whose sections are the four real menus, so both modes render
  // through the same markup. Sub-headers are dropped there; one section per menu is scannable.
  const compactMenu: Menu = $derived({
    title: '☰',
    sections: menus.map(m => ({ header: m.title, items: m.sections.flatMap(s => s.items) })),
  });
  const shown: Menu[] = $derived(compact ? [compactMenu] : menus);

  let openTitle = $state<string | null>(null);
  // Anchored on open rather than nested: the panel resolves `overflow-y` to `auto` (one axis
  // is `hidden`), so an absolutely-positioned dropdown gets *clipped* whenever the panel is
  // shorter than the menu. Dropping downward never overhangs the render in either layout.
  let anchor = $state({ left: 0, y: 0, flipped: false, maxHeight: 0 });

  const openAt = (title: string, btn: HTMLElement) => {
    const r = btn.getBoundingClientRect();
    const below = window.innerHeight - barHeight - r.bottom - 8;
    const above = r.top - 8;
    // Prefer downward — it never overhangs the render in either layout. Flip only when
    // below is genuinely too cramped (a panel dragged near its minimum).
    const flipped = below < 160 && above > below;
    anchor = {
      left: r.left,
      y: flipped ? window.innerHeight - r.top : r.bottom,
      flipped,
      maxHeight: Math.max(flipped ? above : below, 80),
    };
    openTitle = title;
  };

  $effect(() => {
    if (openTitle === null) return;
    return dismissOn('[data-menubar]', () => (openTitle = null));
  });
</script>

<div class={['menubar', compact ? 'compact' : '']} data-menubar>
  {#each shown as menu (menu.title)}
    <div class="slot">
      <button
        class="title"
        class:open={openTitle === menu.title}
        aria-label={compact ? 'menu' : undefined}
        onclick={e => {
          if (openTitle === menu.title) openTitle = null;
          else openAt(menu.title, e.currentTarget as HTMLElement);
        }}
        onmouseenter={e => {
          if (openTitle !== null) openAt(menu.title, e.currentTarget as HTMLElement);
        }}
      >
        {menu.title}
      </button>
      {#if openTitle === menu.title}
        <div
          class="dropdown"
          style={`left: ${anchor.left}px; ${anchor.flipped ? 'bottom' : 'top'}: ${anchor.y}px; max-height: ${anchor.maxHeight}px;`}
        >
          {#each menu.sections as section, si (si)}
            <div class="section" class:ruled={si > 0}>
              {#if section.header}<div class="section-header">{section.header}</div>{/if}
              {#each section.items as item (item.label)}
                <button
                  class="item"
                  disabled={item.disabled}
                  onclick={() => {
                    openTitle = null;
                    item.action();
                  }}
                >
                  <span class="label">{item.label}</span>
                  {#if item.state}<span class="state">{item.state}</span>{/if}
                  {#if item.shortcut}<span class="shortcut">{item.shortcut}</span>{/if}
                </button>
              {/each}
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/each}
  {#if !compact}
    <span class="comp-title" class:untitled={title === 'untitled'}>{title}</span>
  {/if}
</div>

<style>
  .menubar {
    display: flex;
    align-items: stretch;
    height: 26px;
    flex-shrink: 0;
    background: #141414;
    border-bottom: 1px solid #444;
    box-sizing: border-box;
  }

  /* Lives inside the run bar rather than atop the panel, so it carries no chrome. */
  .menubar.compact {
    height: auto;
    background: none;
    border-bottom: none;
  }

  .menubar.compact .title {
    border-right: none;
    font-size: 15px;
    padding: 0 8px;
  }

  .slot {
    position: relative;
    display: flex;
  }

  .title {
    background: none;
    border: none;
    border-right: 1px solid #262626;
    color: #aaa;
    font-family: inherit;
    font-size: 12px;
    padding: 0 10px;
    cursor: pointer;
  }

  .title:hover,
  .title.open {
    background: #222;
    color: #eee;
  }

  /* Drops *inside* the panel so it never overhangs the render. */
  .dropdown {
    position: fixed;
    width: 230px;
    background: #222;
    border: 1px solid #444;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    z-index: 6;
  }

  .section {
    display: flex;
    flex-direction: column;
    padding: 2px 0;
  }

  .section.ruled {
    border-top: 1px solid #333;
  }

  .section-header {
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: #666;
    padding: 3px 10px 1px;
  }

  .item {
    display: flex;
    align-items: baseline;
    gap: 8px;
    background: none;
    border: none;
    color: #ddd;
    font-family: inherit;
    font-size: 12px;
    text-align: left;
    padding: 4px 10px;
    cursor: pointer;
  }

  .item:hover:not(:disabled) {
    background: #2a2a2a;
  }

  .item:disabled {
    color: #666;
    cursor: default;
  }

  .label {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .state {
    color: #888;
    font-size: 11px;
  }

  .shortcut {
    color: #666;
    font-size: 11px;
    white-space: nowrap;
  }

  .comp-title {
    margin-left: auto;
    align-self: center;
    padding: 0 8px;
    font-size: 12px;
    color: #ddd;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .comp-title.untitled {
    color: #666;
  }
</style>
