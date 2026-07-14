<script lang="ts">
  import { untrack } from 'svelte';
  import { createMaterial } from 'src/geoscript/geotoyAPIClient';
  import type { MaterialDef } from 'src/geoscript/materials';
  import TagsInput from '../TagsInput.svelte';

  let {
    material,
    onclose = () => {},
    onsave = () => {},
  }: {
    material: MaterialDef;
    onclose: () => void;
    onsave: () => void;
  } = $props();

  let name = $state(untrack(() => material.name));
  let description = $state('');
  let tags = $state<string[]>([]);
  let isShared = $state(false);
  let isSaving = $state(false);
  let error = $state<string | null>(null);

  const handleSave = async () => {
    isSaving = true;
    error = null;
    try {
      await createMaterial({ ...material, name }, isShared, { description, tags });
      onsave();
    } catch (e: any) {
      error = e.message;
    } finally {
      isSaving = false;
    }
  };
</script>

<div class="save-material-form">
  <h2>Save Material</h2>
  <form
    onsubmit={evt => {
      evt.preventDefault();
      handleSave();
    }}
  >
    <div class="form-group">
      <label for="material-name">name</label>
      <input id="material-name" type="text" bind:value={name} />
    </div>
    <div class="form-group">
      <label for="material-description">description</label>
      <textarea
        id="material-description"
        rows="3"
        maxlength="2000"
        placeholder="what it looks like; credit / attribution / license"
        bind:value={description}
      ></textarea>
    </div>
    <div class="form-group">
      <label for="material-tags">tags</label>
      <TagsInput id="material-tags" bind:tags />
    </div>
    <div class="form-group">
      <label for="material-shared">public</label>
      <input id="material-shared" type="checkbox" bind:checked={isShared} />
    </div>
    {#if error}
      <div class="error">{error}</div>
    {/if}
    <div class="buttons">
      <button type="button" onclick={onclose} disabled={isSaving}>Cancel</button>
      <button type="submit" disabled={isSaving}>{isSaving ? 'Saving...' : 'Save'}</button>
    </div>
  </form>
</div>

<style>
  .save-material-form {
    padding: 16px;
    background: #2a2a2a;
    color: #f0f0f0;
    border: 1px solid #444;
    width: 300px;
  }
  h2 {
    margin-top: 0;
    font-size: 16px;
  }
  .form-group {
    margin-bottom: 12px;
  }
  label {
    display: block;
    margin-bottom: 4px;
    font-size: 12px;
  }
  input[type='text'],
  textarea {
    width: 100%;
    box-sizing: border-box;
    background-color: #1a1a1a;
    color: #eee;
    border: 1px solid #444;
    padding: 4px 6px;
    font-size: 12px;
    font-family: inherit;
  }
  textarea {
    resize: vertical;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
  }
  button {
    background: #333;
    border: 1px solid #555;
    color: #f0f0f0;
    padding: 8px 12px;
    cursor: pointer;
  }
  button:hover {
    background: #3d3d3d;
  }
  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .error {
    color: #f44;
    font-size: 12px;
    margin-top: 8px;
  }
</style>
