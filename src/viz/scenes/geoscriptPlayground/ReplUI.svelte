<script lang="ts">
  import type { Viz } from 'src/viz';
  import { buildGrayFossilRockMaterial } from 'src/viz/materials/GrayFossilRock/GrayFossilRockMaterial';
  import * as THREE from 'three';

  let {
    viz,
    repl,
    ctxPtr,
  }: {
    viz: Viz;
    repl: typeof import('/home/casey/dream/src/viz/wasmComp/geoscript_repl');
    ctxPtr: number;
  } = $props();

  let code = $state('');
  let err: string | null = $state(null);
  let renderedMeshes: THREE.Mesh[] = $state([]);

  const loader = new THREE.ImageBitmapLoader();
  const matPromise = buildGrayFossilRockMaterial(
    loader,
    { uvTransform: new THREE.Matrix3().scale(0.2, 0.2), color: 0xcccccc, mapDisableDistance: null },
    {},
    { useGeneratedUVs: true, useTriplanarMapping: false, tileBreaking: undefined }
  );

  const run = async () => {
    for (const mesh of renderedMeshes) {
      viz.scene.remove(mesh);
      mesh.geometry.dispose();
    }
    renderedMeshes = [];

    repl.geoscript_repl_reset(ctxPtr);
    try {
      repl.geoscript_repl_eval(ctxPtr, code);
    } catch (err) {
      console.error('Error evaluating code:', err);
      // TODO: this set isn't working for some reason
      err = `Error evaluating code: ${err}`;
      return;
    }
    err = repl.geoscript_repl_get_err(ctxPtr) || null;

    const renderedMeshCount = repl.geoscript_repl_get_rendered_mesh_count(ctxPtr);
    const newRenderedMeshes = [];
    for (let i = 0; i < renderedMeshCount; i++) {
      const verts = repl.geoscript_repl_get_rendered_mesh_vertices(ctxPtr, i);
      const indices = repl.geoscript_repl_get_rendered_mesh_indices(ctxPtr, i);
      const normals = repl.geoscript_repl_get_rendered_mesh_normals(ctxPtr, i);

      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute('position', new THREE.BufferAttribute(verts, 3));
      geometry.setIndex(new THREE.BufferAttribute(indices, 1));
      console.log({ normals });
      if (normals) {
        geometry.setAttribute('normal', new THREE.BufferAttribute(normals, 3));
      }

      const mat = await matPromise;
      const mesh = new THREE.Mesh(geometry, mat);
      mesh.castShadow = true;
      mesh.receiveShadow = true;
      viz.scene.add(mesh);
      newRenderedMeshes.push(mesh);
    }

    renderedMeshes = newRenderedMeshes;
  };
</script>

<div class="root">
  <textarea bind:value={code} rows="10" cols="50" style="resize: none;"></textarea>
  <div class="controls">
    <button onclick={run}>run</button>
    {#if err}
      <div class="error">
        <pre>{err}</pre>
      </div>
    {/if}
  </div>
</div>

<style lang="css">
  .root {
    height: calc(max(300px, 25vh));
    width: 100%;
    position: absolute;
    bottom: 0;
    display: flex;
    flex-direction: row;

    textarea {
      flex: 1;
      background: #141414;
      color: #f0f0f0;
      outline: none;
    }
  }

  .controls {
    min-width: 200px;
    max-width: calc(max(300px, 50vw));
    flex: 0.4;
    padding: 8px;
    border-top: 1px solid #444;
  }

  .error {
    color: red;
    background: #222;
    padding: 8px;
    border-radius: 4px;
    margin-top: 8px;
    overflow-y: auto;
    max-height: 100%;
  }
</style>
