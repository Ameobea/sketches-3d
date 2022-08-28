<script lang="ts">
  import { onMount } from 'svelte';

  let engine: typeof import('../../viz/wasmComp/engine') | null = null;
  let textures: { orig: string; normal: string; normalBytes: Uint8Array } | null = null;

  onMount(async () => {
    const engineLoaded = await import('../../viz/wasmComp/engine').then(async mod => {
      await mod.default();
      return mod;
    });
    engine = engineLoaded;
  });

  const handleDroppedFile = async (
    evt: Event & {
      currentTarget: EventTarget & HTMLInputElement;
    }
  ) => {
    const file = evt.currentTarget.files?.[0];
    if (!file) {
      return;
    }

    const img = document.createElement('img');
    img.src = URL.createObjectURL(file);
    await new Promise(resolve => {
      img.onload = resolve;
    });
    const width = img.width;
    const height = img.height;

    const canvas = document.createElement('canvas');
    const ctx = canvas.getContext('2d')!;
    canvas.width = width;
    canvas.height = height;
    ctx.drawImage(img, 0, 0);
    const imageData = ctx.getImageData(0, 0, width, height);

    const normalMapBytes: Uint8Array = engine!.gen_normal_map_from_texture(
      new Uint8Array(imageData.data.buffer),
      height,
      width
    );
    const normalMapImageData = new ImageData(new Uint8ClampedArray(normalMapBytes.buffer), width, height);
    ctx.putImageData(normalMapImageData, 0, 0);
    const canvasDataURL = canvas.toDataURL();

    textures = {
      orig: img.src,
      normal: canvasDataURL,
      normalBytes: normalMapBytes,
    };
  };
</script>

{#if engine}
  <div class="root">
    <input type="file" id="file" accept="image/png, image/jpeg" on:change={handleDroppedFile} />
    {#if textures}
      <div>
        <img src={textures.orig} alt="orig" />
        <img src={textures.normal} alt="norm" />
      </div>
    {/if}
  </div>
{:else}
  <div class="root">
    <p>Loading engine...</p>
  </div>
{/if}

<style lang="css">
  .root {
    display: flex;
    flex-direction: column;
  }
</style>
