<script lang="ts">
  import {
    createTexture,
    createTextureFromURL,
    updateTexture,
    deleteTexture,
    type TextureDescriptor,
    APIError,
  } from 'src/geoscript/geotoyAPIClient';
  import TagsInput from '../TagsInput.svelte';
  import { Textures } from './state.svelte';
  import { untrack } from 'svelte';
  import { logGeotoyEvent } from 'src/analytics';

  let {
    texture,
    onclose = () => {},
    onupload = (_texture: TextureDescriptor) => {},
    ondelete = () => {},
  }: {
    /** When set, the form edits this texture's metadata in place rather than creating a new one. */
    texture?: TextureDescriptor;
    onclose: () => void;
    onupload: (texture: TextureDescriptor) => void;
    ondelete?: () => void;
  } = $props();

  const isEdit = untrack(() => !!texture);

  let uploadMethod: 'file' | 'url' = $state('file');
  let name = $state(untrack(() => texture?.name ?? ''));
  let description = $state(untrack(() => texture?.description ?? ''));
  let tags = $state<string[]>(untrack(() => [...(texture?.tags ?? [])]));
  let isShared = $state(untrack(() => texture?.isShared ?? false));
  let fileInput = $state<HTMLInputElement | null>(null);
  let urlInput = $state('');
  let status = $state<
    { type: 'ok'; msg: string } | { type: 'error'; msg: string } | { type: 'loading' } | null
  >(null);

  const withStatus = async (okMsg: string, run: () => Promise<void>) => {
    status = { type: 'loading' };
    try {
      await run();
      status = { type: 'ok', msg: okMsg };
    } catch (e) {
      status = { type: 'error', msg: e instanceof APIError ? e.message : 'an unknown error occurred' };
    }
  };

  const handleSubmit = () =>
    withStatus(isEdit ? 'texture updated!' : 'texture uploaded!', async () => {
      let updated: TextureDescriptor;
      if (texture) {
        updated = await updateTexture(texture.id, { name, description, isShared, tags });
      } else if (uploadMethod === 'file') {
        if (!fileInput?.files?.length) {
          throw new APIError(400, 'no file selected');
        }
        updated = await createTexture({ name, description, isShared, tags }, fileInput.files[0]);
      } else {
        if (!urlInput) {
          throw new APIError(400, 'no URL provided');
        }
        updated = await createTextureFromURL({ name, description, isShared, tags }, urlInput);
      }
      Textures.textures[updated.id] = updated;
      if (!isEdit) {
        logGeotoyEvent('materials', 'texture_upload', { method: uploadMethod });
      }
      onupload(updated);
    });

  const handleDelete = () => {
    if (!texture || !confirm(`permanently delete texture “${texture.name}”?`)) {
      return;
    }
    return withStatus('texture deleted', async () => {
      await deleteTexture(texture.id);
      delete Textures.textures[texture.id];
      ondelete();
    });
  };
</script>

<div>
  <div class="texture-uploader">
    <div class="header">{isEdit ? 'edit texture' : 'upload new texture'}</div>
    <div class="content">
      {#if !isEdit}
        <div class="instructions">
          <ul>
            <li>most common image formats are accepted</li>
            <li>
              seamless/tileable texture are <i>highly</i>
              recommended
            </li>
            <li>textures should be square and ideally a power of two for best performance</li>
          </ul>
        </div>
      {/if}
      <div class="form-grid">
        <label for="name-input">name</label>
        <input id="name-input" type="text" bind:value={name} />

        <label for="description-input">description</label>
        <textarea
          id="description-input"
          rows="3"
          maxlength="2000"
          placeholder="what it looks like; credit / attribution / license"
          bind:value={description}
        ></textarea>

        <label for="tags-input">tags</label>
        <TagsInput id="tags-input" bind:tags />

        <label for="is-shared-checkbox">public</label>
        <input id="is-shared-checkbox" type="checkbox" bind:checked={isShared} />

        {#if !isEdit}
          <!-- svelte-ignore a11y_label_has_associated_control -->
          <label>method</label>
          <div class="toggle-group">
            <button class:selected={uploadMethod === 'file'} onclick={() => (uploadMethod = 'file')}>
              file
            </button>
            <button class:selected={uploadMethod === 'url'} onclick={() => (uploadMethod = 'url')}>
              url
            </button>
          </div>

          {#if uploadMethod === 'file'}
            <label for="file-input">file</label>
            <input id="file-input" type="file" bind:this={fileInput} />
          {:else}
            <label for="url-input">url</label>
            <input id="url-input" type="text" bind:value={urlInput} />
          {/if}
        {:else if texture?.sourceUrl}
          <!-- svelte-ignore a11y_label_has_associated_control -->
          <label>source</label>
          <a class="source-url" href={texture.sourceUrl} target="_blank" rel="noreferrer noopener">
            {texture.sourceUrl}
          </a>
        {/if}
      </div>
    </div>
    <div class="buttons">
      <div class="status-container">
        {#if status && (status.type === 'ok' || status.type === 'error')}
          <div class="status {status.type}">
            {status.msg}
          </div>
        {/if}
      </div>
      {#if isEdit}
        <button class="footer-button delete" onclick={handleDelete} disabled={status?.type === 'loading'}>
          delete
        </button>
      {/if}
      <button class="footer-button" onclick={onclose}>cancel</button>
      <button class="footer-button" onclick={handleSubmit} disabled={status?.type === 'loading'}>
        {#if status?.type === 'loading'}loading...{:else if isEdit}save{:else}submit{/if}
      </button>
    </div>
  </div>
</div>

<style>
  .texture-uploader {
    display: flex;
    flex: 1;
    flex-direction: column;
    height: 100%;
    background: #2a2a2a;
  }
  .header {
    padding: 8px;
    border-bottom: 1px solid #444;
    font-weight: bold;
  }
  .content {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 12px;
    flex-grow: 1;
    overflow-y: auto;
  }
  .instructions {
    font-size: 12px;
    color: #aaa;
  }
  .instructions ul {
    padding-left: 20px;
    margin: 4px 0 0;
  }
  .form-grid {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 8px;
    align-items: center;
  }
  label {
    font-size: 12px;
    text-align: right;
  }
  input[type='text'],
  input[type='file'],
  textarea {
    background-color: #1a1a1a;
    color: #eee;
    border: 1px solid #444;
    padding: 4px 6px;
    font-size: 12px;
    font-family: inherit;
  }
  textarea {
    resize: vertical;
    align-self: start;
  }
  .source-url {
    font-size: 11px;
    color: #7ab;
    overflow-wrap: anywhere;
  }
  .toggle-group {
    display: flex;
  }
  .toggle-group button {
    background: #333;
    border: 1px solid #555;
    color: #eee;
    padding: 4px 8px;
    cursor: pointer;
  }
  .toggle-group button.selected {
    background: #444;
    border-color: #777;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    padding: 8px;
    border-top: 1px solid #444;
    gap: 8px;
    align-items: center;
  }
  .status-container {
    flex-grow: 1;
  }
  .status {
    font-size: 12px;
  }
  .status.ok {
    color: #12cc12;
  }
  .status.error {
    color: red;
  }
  .footer-button.delete:hover {
    background: #5a2020;
  }
</style>
