<script lang="ts">
  let {
    tags = $bindable(),
    id,
    placeholder = 'add tag…',
  }: { tags: string[]; id?: string; placeholder?: string } = $props();

  let draft = $state('');

  /** Mirrors the backend's canonicalization so chips read back exactly as they'll be stored. */
  const commit = () => {
    const tag = draft.trim().replace(/\s+/g, ' ').toLowerCase();
    draft = '';
    if (tag && !tags.includes(tag)) {
      tags = [...tags, tag];
    }
  };

  const handleKeydown = (evt: KeyboardEvent) => {
    if (evt.key === 'Enter' || evt.key === ',') {
      evt.preventDefault();
      commit();
    } else if (evt.key === 'Backspace' && !draft && tags.length) {
      tags = tags.slice(0, -1);
    }
  };
</script>

<div class="tags-input">
  {#each tags as tag (tag)}
    <span class="chip">
      {tag}
      <button type="button" title="remove" onclick={() => (tags = tags.filter(t => t !== tag))}>×</button>
    </span>
  {/each}
  <input
    {id}
    type="text"
    bind:value={draft}
    onkeydown={handleKeydown}
    onblur={commit}
    placeholder={tags.length ? '' : placeholder}
  />
</div>

<style>
  .tags-input {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    background-color: #1a1a1a;
    border: 1px solid #444;
    padding: 3px 4px;
  }
  .chip {
    display: flex;
    align-items: center;
    gap: 3px;
    background: #3a3a3a;
    border: 1px solid #555;
    padding: 1px 3px 1px 5px;
    font-size: 11px;
    white-space: nowrap;
  }
  .chip button {
    background: none;
    border: none;
    color: #aaa;
    cursor: pointer;
    padding: 0 2px;
    font-size: 13px;
    line-height: 1;
  }
  .chip button:hover {
    color: #fff;
  }
  input {
    flex: 1;
    min-width: 80px;
    background: none;
    border: none;
    color: #eee;
    padding: 3px 2px;
    font-size: 12px;
    font-family: inherit;
  }
  input:focus {
    outline: none;
  }
</style>
