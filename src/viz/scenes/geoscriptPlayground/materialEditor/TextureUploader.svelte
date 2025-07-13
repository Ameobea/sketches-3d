<script lang="ts">
  import { uploadTexture, type Texture } from './textureStore';

  export let onupload = (texture: Texture) => {};

  let fileInput: HTMLInputElement | null;
  let urlInput = $state('');
  let isUploading = $state(false);

  const handleFileSelect = async (e: Event) => {
    const file = (e.target as HTMLInputElement).files?.[0];
    if (file) {
      isUploading = true;
      const newTexture = await uploadTexture(file);
      onupload(newTexture);
      isUploading = false;
    }
  };

  const handleUrlUpload = async () => {
    if (urlInput) {
      isUploading = true;
      const newTexture = await uploadTexture(urlInput);
      onupload(newTexture);
      isUploading = false;
      urlInput = '';
    }
  };

</script>

<div class="texture-uploader">
  <h3>upload new texture</h3>

  <div class="upload-option">
    <label for="file-upload">from file</label>
    <input id="file-upload" type="file" accept="image/*" on:change={handleFileSelect} bind:this={fileInput} />
  </div>

  <div class="or-divider">or</div>

  <div class="upload-option">
    <label for="url-upload">from url</label>
    <input id="url-upload" type="text" placeholder="https://..." bind:value={urlInput} />
    <button on:click={handleUrlUpload} disabled={!urlInput || isUploading}>upload</button>
  </div>

  {#if isUploading}
    <div class="loading-overlay">
      uploading...
    </div>
  {/if}
</div>

<style>
  .texture-uploader {
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  h3 {
      font-size: 14px;
      margin: 0 0 8px 0;
      text-align: center;
  }
  .upload-option {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  label {
      font-size: 12px;
  }
  .or-divider {
      text-align: center;
      color: #888;
      margin: 8px 0;
  }
  .loading-overlay {
      position: absolute;
      top: 0;
      left: 0;
      right: 0;
      bottom: 0;
      background: rgba(0,0,0,0.5);
      display: flex;
      align-items: center;
      justify-content: center;
  }
</style>