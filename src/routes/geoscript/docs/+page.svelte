<script lang="ts">
  import type { PageData } from './$types';
  import FnDoc from './FnDoc.svelte';
  import './docs.css';
  import type { BuiltinFnDef } from './types';
  import fuzzysort from 'fuzzysort';

  let { data }: { data: PageData } = $props();

  let defsByModule: { moduleName: string; defs: { name: string; def: BuiltinFnDef }[] }[] = $derived.by(
    () => {
      const entries = Object.entries(data.builtinFnDefs).reduce(
        (acc, [name, def]) => {
          if (!acc[def.module]) {
            acc[def.module] = [];
          }
          acc[def.module].push({ name, def });
          return acc;
        },
        {} as Record<string, { name: string; def: BuiltinFnDef }[]>
      );

      for (const module in entries) {
        entries[module].sort((a, b) => a.name.localeCompare(b.name));
      }

      const flatEntries: { moduleName: string; defs: { name: string; def: BuiltinFnDef }[] }[] =
        Object.entries(entries).map(([moduleName, defs]) => ({
          moduleName,
          defs,
        }));
      flatEntries.sort((a, b) => a.moduleName.localeCompare(b.moduleName));
      return flatEntries;
    }
  );

  let allFns = $derived(defsByModule.flatMap(m => m.defs));

  let searchQuery = $state('');
  let searchResults = $state<Fuzzysort.KeyResults<(typeof allFns)[number]> | null>(null);
  let searchInputFocused = $state(false);
  let tocMenuOpen = $state(false);

  $effect(() => {
    if (searchQuery) {
      searchResults = fuzzysort.go(searchQuery, allFns, { key: 'name' });
    } else {
      searchResults = null;
    }
  });

  const toggleTocMenuOpen = () => {
    tocMenuOpen = !tocMenuOpen;
  };
</script>

<div class="root">
  <div class="header">
    <div class="toc" class:open={tocMenuOpen}>
      <span
        class="toc-title"
        onclick={toggleTocMenuOpen}
        onkeydown={(evt: KeyboardEvent) => {
          if (evt.key === 'Enter' || evt.key === ' ') {
            evt.preventDefault();
            toggleTocMenuOpen();
          }
        }}
        role="button"
        tabindex="0"
      >
        modules
      </span>
      <div class="toc-items">
        {#each defsByModule as { moduleName }}
          <a
            href="#module-{moduleName}"
            class="toc-item"
            onclick={() => {
              tocMenuOpen = false;
            }}
          >
            {moduleName}
          </a>
        {/each}
      </div>
    </div>
    <div class="search-container">
      <input
        type="text"
        placeholder="search functions"
        class="search-input"
        value={searchQuery}
        oninput={(e: Event) => {
          const target = e.target as HTMLInputElement;
          searchQuery = target.value;
        }}
        onfocus={() => {
          searchInputFocused = true;
        }}
        onblur={() =>
          setTimeout(() => {
            searchInputFocused = false;
          }, 150)}
      />
      {#if searchResults?.length && searchInputFocused}
        <div class="search-results">
          {#each searchResults as result (result.obj.name)}
            <a
              href="#{result.obj.name}"
              class="search-result-item"
              onclick={() => {
                searchQuery = '';
              }}
            >
              <span class="result-name">{result.obj.name}</span>
              <span class="result-module">{result.obj.def.module}</span>
            </a>
          {/each}
        </div>
      {/if}
    </div>
  </div>
  <div class="docs">
    {#each defsByModule as { moduleName, defs }}
      <h2 id={`module-${moduleName}`} class="module-name"><a href="#module-{moduleName}">{moduleName}</a></h2>
      {#each defs as { name, def }}
        <FnDoc {name} {def} />
      {/each}
    {/each}
  </div>
</div>

<style lang="css">
  @import url('https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600;700&display=swap');

  .root {
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    display: flex;
    flex-direction: column;
    border: 0;
    padding: 0;
  }

  .header {
    position: fixed;
    top: 0;
    background: #191818;
    padding: 12px 16px;
    z-index: 10;
    border-bottom: 1px solid #32302f;
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
    width: 100vw;
    max-width: 100vw;
  }

  .toc {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 1;
    min-width: 0;
  }

  .toc-title {
    font-weight: 600;
    color: #f0f0f0;
    white-space: nowrap;
  }

  .toc-items {
    display: flex;
    gap: 12px;
  }

  .toc-item {
    color: #83a598;
    text-decoration: none;
    white-space: nowrap;
  }

  .toc-item:hover {
    color: #b8bb26;
    text-decoration: underline;
  }

  .search-container {
    position: relative;
    min-width: 300px;
  }

  .search-input {
    width: 100%;
    padding: 8px 12px;
    background: #232323;
    border: 1px solid #32302f;
    color: #e0e0e0;
    font-family: inherit;
    margin-left: -70px;
  }

  .search-results {
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    background: #232323;
    border: 1px solid #32302f;
    border-top: none;
    max-height: 400px;
    overflow-y: auto;
  }

  .search-result-item {
    display: flex;
    justify-content: space-between;
    padding: 8px 12px;
    color: #e0e0e0;
    text-decoration: none;
    cursor: pointer;
  }

  .search-result-item:hover {
    background: #32302f;
  }

  .result-name {
    font-weight: 600;
  }

  .result-module {
    color: #83a598;
  }

  .docs {
    display: flex;
    flex-direction: column;
    max-width: 960px;
    margin: 0 auto;
    padding: 16px 8px 8px 8px;
    width: 100%;
    margin-top: 40px;
  }

  .module-name {
    font-size: 32px;
    font-weight: 600;
    margin-top: 16px;
    margin-bottom: 32px;
    color: #f0f0f0;
    scroll-margin-top: 70px;
    border-bottom: 1px solid #32302f;
    padding-bottom: 2px;
  }

  .module-name a {
    color: #f0f0f0;
    text-decoration: underline;
  }

  .module-name a:hover {
    color: #cfcfcf;
  }

  @media (max-width: 600px) {
    .header {
      flex-direction: column;
      align-items: stretch;
      width: calc(100vw - 12px);
      overflow-y: visible;
    }

    .search-input {
      margin-left: 0;
      max-width: calc(100% - 28px);
      margin-left: auto;
      margin-right: auto;
    }

    .toc {
      overflow-x: visible;
      flex-shrink: 0;
      position: relative;
    }

    .toc-title {
      cursor: pointer;
      user-select: none;
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding: 4px 0;
    }

    .toc-title::after {
      content: '▼';
      font-size: 0.8em;
      margin-left: 8px;
    }

    .toc.open .toc-title::after {
      transform: rotate(180deg);
    }

    .toc-items {
      display: none;
      position: absolute;
      top: 100%;
      left: 0;
      right: 0;
      background: #232323;
      border: 1px solid #32302f;
      max-height: 300px;
      overflow-y: auto;
      z-index: 20;
      flex-direction: column;
      gap: 0;
    }

    .toc.open .toc-items {
      display: flex;
    }

    .toc-items .toc-item {
      padding: 10px 12px;
      white-space: normal;
      border-bottom: 1px solid #32302f;
    }

    .toc-items .toc-item:last-child {
      border-bottom: none;
    }

    .docs {
      margin-top: 80px;
    }

    .module-name {
      scroll-margin-top: 110px;
    }
  }
</style>
