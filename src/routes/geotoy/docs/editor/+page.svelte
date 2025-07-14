<script lang="ts">
  import { buildGeotoyKeymap, type KeymapEntry } from 'src/viz/scenes/geoscriptPlayground/keymap';

  const keymap = buildGeotoyKeymap();
  const keymapGroups: Record<string, KeymapEntry[]> = {};
  for (const entry of keymap) {
    if (!keymapGroups[entry.group ?? '']) {
      keymapGroups[entry.group ?? ''] = [];
    }
    keymapGroups[entry.group ?? ''].push(entry);
  }
</script>

<h2>Editor Reference + Shortcuts</h2>

<ul>
  {#each Object.entries(keymapGroups) as [group, entries] (group)}
    <li>
      <h3>{group || 'general'}</h3>
      <ul>
        {#each entries as { key, label } (key)}
          <li>
            <span class="key">{key}</span>
            :
            <span class="label">{label}</span>
          </li>
        {/each}
      </ul>
    </li>
  {/each}
</ul>

<style lang="css">
  .key {
    font-weight: 500;
  }

  .label {
    color: #b4b4b4;
  }

  .key,
  .label {
    font-size: 18px !important;
  }

  h3 {
    text-transform: uppercase;
  }

  ul {
    list-style: none;
  }
</style>
