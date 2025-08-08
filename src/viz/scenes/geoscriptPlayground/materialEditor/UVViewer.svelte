<script lang="ts">
  import { onMount } from 'svelte';
  import type * as Comlink from 'comlink';

  import type { GeoscriptWorkerMethods } from 'src/geoscript/geoscriptWorker.worker';
  import { buildUVUnwrapDistortionSVG, initUVUnwrap } from 'src/viz/wasm/uv_unwrap/uvUnwrap';
  import type { MaterialDef } from 'src/geoscript/materials';
  import UvPropertiesEditor from './UVPropertiesEditor.svelte';

  let {
    onclose,
    repl,
    ctxPtr,
    matDef,
    rerun,
  }: {
    onclose: () => void;
    repl: Comlink.Remote<GeoscriptWorkerMethods>;
    ctxPtr: number;
    matDef: Extract<MaterialDef, { type: 'physical' }>;
    rerun: (onlyIfUVUnwrapperNotLoaded: boolean) => Promise<void>;
  } = $props();

  let selectedMeshIx = $state<number | null>(null);
  let svgData = $state<string | null>(null);
  let isComputingSVG = $state(false);
  let meshIndices = $state<Uint32Array | null>(null);

  const onSelectMesh = async (ix: number | null) => {
    if (isComputingSVG) {
      return;
    }
    isComputingSVG = true;

    try {
      // check to see if meshes have been removed
      meshIndices = await repl.getRenderedMeshIndicesWithMaterial(ctxPtr, matDef.name);
      console.log({ meshIndices, name: matDef.name });
      if (meshIndices.length === 0) {
        selectedMeshIx = null;
        return;
      }
      if (ix === null) {
        ix = meshIndices[0];
      }
      if (!meshIndices.includes(ix)) {
        console.warn('mesh index not found in current list of meshes with this material');
        selectedMeshIx = null;
      } else {
        selectedMeshIx = ix;
      }
      if (selectedMeshIx === null) {
        return;
      }

      if (matDef.textureMapping?.type !== 'uv') {
        throw new Error('material passed here should always have a uv texture mapping');
      }
      const { numCones: nCones, flattenToDisk, mapToSphere } = matDef.textureMapping;

      const { verts, indices } = await repl.getRenderedMesh(ctxPtr, selectedMeshIx);

      const unwrapRes = buildUVUnwrapDistortionSVG(verts, indices, nCones, flattenToDisk, mapToSphere);
      if (unwrapRes.type === 'error') {
        // TODO: handle error state
        return;
      } else {
        svgData = unwrapRes.out;
      }
    } finally {
      isComputingSVG = false;
    }
  };

  const refresh = () => onSelectMesh(selectedMeshIx);

  onMount(() => {
    (async () => {
      await initUVUnwrap();
      onSelectMesh(null);
    })();
  });
</script>

<div style="display: flex; flex-direction: column; height: 100%; flex: 1">
  <div class="root">
    <div class="mesh-index-list">
      {#if meshIndices !== null}
        {#if meshIndices?.length === 0}
          <span style="padding: 4px">no meshes found with this material</span>
        {:else}
          {#each meshIndices as ix (ix)}
            <button class:selected={selectedMeshIx === ix} onclick={() => onSelectMesh(ix)}>
              {ix + 1}
            </button>
          {/each}
        {/if}
      {:else}
        <span>Loading...</span>
      {/if}
      <div style="margin-top: auto">
        <UvPropertiesEditor
          material={matDef}
          rerun={async onlyIfUVUnwrapperNotLoaded => {
            await rerun(onlyIfUVUnwrapperNotLoaded);
            refresh();
          }}
        />
      </div>
    </div>
    {#if svgData}
      <div class="uv-preview">
        {@html svgData}
      </div>
    {:else}
      Loading...
    {/if}
  </div>
  <div class="actions">
    <button onclick={onclose}>back</button>
    <button disabled={isComputingSVG || selectedMeshIx === null} onclick={refresh}>refresh</button>
  </div>
</div>

<style lang="css">
  .root {
    display: flex;
    flex-direction: row;
    min-height: 346px;
  }

  .mesh-index-list {
    width: 50px;
    display: flex;
    flex-direction: column;

    button {
      flex: 0;
      padding: 4px;
      text-align: center;
      border: none;
      background-color: #444;
      cursor: pointer;
      border: 1px solid #666;
    }

    button:not(:last-child) {
      border-bottom: none;
    }

    button.selected {
      background-color: #aaa;
    }
  }

  .uv-preview {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    max-width: 300px;
    margin-left: auto;
    margin-right: 10px;
  }

  .actions {
    button {
      width: 80px;
    }
  }
</style>
