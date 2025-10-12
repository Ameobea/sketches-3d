<script lang="ts">
  import {
    createTexture,
    createTextureFromURL,
    type TextureDescriptor,
    APIError,
  } from 'src/geoscript/geotoyAPIClient';
  import { Textures } from './state.svelte';

  let {
    onclose = () => {},
    onupload = (texture: TextureDescriptor) => {},
  }: {
    onclose: () => void;
    onupload: (texture: TextureDescriptor) => void;
  } = $props();

  let uploadMethod: 'file' | 'url' = $state('file');
  let name = $state('');
  let isShared = $state(false);
  let fileInput = $state<HTMLInputElement | null>(null);
  let urlInput = $state('');
  let status = $state<
    { type: 'ok'; msg: string } | { type: 'error'; msg: string } | { type: 'loading' } | null
  >(null);

  const handleSubmit = async () => {
    status = { type: 'loading' };
    try {
      let texture: TextureDescriptor;
      if (uploadMethod === 'file') {
        if (!fileInput?.files?.length) {
          status = { type: 'error', msg: 'no file selected' };
          return;
        }
        texture = await createTexture(name, fileInput.files[0], isShared);
      } else {
        if (!urlInput) {
          status = { type: 'error', msg: 'no URL provided' };
          return;
        }
        texture = await createTextureFromURL(name, urlInput, isShared);
      }
      Textures.textures[texture.id] = texture;
      onupload(texture);
      status = { type: 'ok', msg: 'texture uploaded!' };
    } catch (e) {
      if (e instanceof APIError) {
        status = { type: 'error', msg: e.message };
      } else {
        status = { type: 'error', msg: 'an unknown error occurred' };
      }
    }
  };
</script>

<div>
  <div class="texture-uploader">
    <div class="header">upload new texture</div>
    <div class="content">
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
      <div class="form-grid">
        <label for="name-input">name</label>
        <input id="name-input" type="text" bind:value={name} />

        <label for="is-shared-checkbox">public</label>
        <input id="is-shared-checkbox" type="checkbox" bind:checked={isShared} />

        <label>method</label>
        <div class="toggle-group">
          <button class:selected={uploadMethod === 'file'} onclick={() => (uploadMethod = 'file')}>
            file
          </button>
          <button class:selected={uploadMethod === 'url'} onclick={() => (uploadMethod = 'url')}>url</button>
        </div>

        {#if uploadMethod === 'file'}
          <label for="file-input">file</label>
          <input id="file-input" type="file" bind:this={fileInput} />
        {:else}
          <label for="url-input">url</label>
          <input id="url-input" type="text" bind:value={urlInput} />
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
      <button class="footer-button" onclick={onclose}>cancel</button>
      <button class="footer-button" onclick={handleSubmit} disabled={status?.type === 'loading'}>
        {#if status?.type === 'loading'}loading...{:else}submit{/if}
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
  input[type='file'] {
    background-color: #1a1a1a;
    color: #eee;
    border: 1px solid #444;
    padding: 4px 6px;
    font-size: 12px;
    font-family: inherit;
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
</style>
