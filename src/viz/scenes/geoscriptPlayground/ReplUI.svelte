<script lang="ts" module>
  export interface ReplCtx {
    centerView: () => void;
    toggleWireframe: () => void;
  }

  interface RunStats {
    runtimeMs: number;
    renderedMeshCount: number;
    renderedPathCount: number;
    totalVtxCount: number;
    totalFaceCount: number;
  }

  const DefaultCameraPos = new THREE.Vector3(10, 10, 10);
  const DefaultCameraTarget = new THREE.Vector3(0, 0, 0);

  const IntFormatter = new Intl.NumberFormat(undefined, {
    style: 'decimal',
    maximumFractionDigits: 0,
  });
</script>

<script lang="ts">
  import type { Viz } from 'src/viz';
  import * as THREE from 'three';
  import { onMount } from 'svelte';
  import { EditorView, type KeyBinding } from '@codemirror/view';
  import type * as Comlink from 'comlink';

  import type { GeoscriptWorkerMethods } from 'src/viz/wasmComp/geoscriptWorker.worker';
  import { buildEditor } from './editor';

  let {
    viz,
    geoscriptWorker: repl,
    ctxPtr,
    setReplCtx,
    baseMat,
  }: {
    viz: Viz;
    geoscriptWorker: Comlink.Remote<GeoscriptWorkerMethods>;
    ctxPtr: number;
    setReplCtx: (ctx: ReplCtx) => void;
    baseMat: THREE.Material;
  } = $props();

  let err: string | null = $state(null);
  let runStats: RunStats | null = $state(null);
  let renderedMeshes: (
    | THREE.Mesh<THREE.BufferGeometry, THREE.Material>
    | THREE.Line<THREE.BufferGeometry, THREE.Material>
  )[] = $state([]);

  let codemirrorContainer: HTMLDivElement | null = $state(null);
  let editorView: EditorView | null = $state(null);
  let activeMat: THREE.Material = $state(baseMat);
  const lineMat = new THREE.LineBasicMaterial({
    color: 0x00ff00,
    linewidth: 2,
  });
  const wireframeMat = new THREE.MeshBasicMaterial({
    color: 0xdf00df,
    wireframe: true,
  });

  const computeCompositeBoundingBox = (
    meshes: (
      | THREE.Mesh<THREE.BufferGeometry, THREE.Material>
      | THREE.Line<THREE.BufferGeometry, THREE.Material>
    )[]
  ): THREE.Box3 => {
    const box = new THREE.Box3();
    for (const mesh of meshes) {
      mesh.geometry.computeBoundingBox();
      const meshBox = mesh.geometry.boundingBox;
      if (meshBox) box.union(meshBox.applyMatrix4(mesh.matrixWorld));
    }
    return box;
  };

  const centerView = async () => {
    if (!viz) {
      return;
    }

    while (!viz.orbitControls) {
      await new Promise(resolve => setTimeout(resolve, 10));
    }

    if (!renderedMeshes.length) {
      viz.camera.position.copy(DefaultCameraPos);
      viz.orbitControls!.target.copy(DefaultCameraTarget);
      viz.camera.lookAt(DefaultCameraTarget);
      viz.orbitControls!.update();
      return;
    }

    const compositeBbox = computeCompositeBoundingBox(renderedMeshes);
    const center = new THREE.Vector3();
    compositeBbox.getCenter(center);
    const size = new THREE.Vector3();
    compositeBbox.getSize(size);

    // try to keep the same look direction
    const lookDir = new THREE.Vector3();
    lookDir.copy(viz.orbitControls!.target).sub(viz.camera.position).normalize();
    const maxDim = Math.max(size.x, size.y, size.z);
    const distance = maxDim * 1.2 + 1;
    viz.camera.position.copy(center).sub(lookDir.multiplyScalar(distance));
    viz.orbitControls!.target.copy(center);
    viz.camera.lookAt(center);
    viz.orbitControls!.update();
  };

  const toggleWireframe = async () => {
    if (activeMat && activeMat instanceof THREE.MeshBasicMaterial) {
      activeMat = baseMat;
    } else {
      activeMat = wireframeMat;
    }
    for (const mesh of renderedMeshes) {
      if (mesh instanceof THREE.Mesh) {
        mesh.material = activeMat;
      }
    }
  };

  const run = async () => {
    if (!editorView) {
      return;
    }

    const code = editorView.state.doc.toString();

    beforeUnloadHandler();

    for (const mesh of renderedMeshes) {
      viz.scene.remove(mesh);
      mesh.geometry.dispose();
    }
    renderedMeshes = [];

    await repl.reset(ctxPtr);
    runStats = null;
    const startTime = performance.now();
    try {
      await repl.eval(ctxPtr, code);
    } catch (err) {
      console.error('Error evaluating code:', err);
      // TODO: this set isn't working for some reason
      err = `Error evaluating code: ${err}`;
      return;
    }
    err = (await repl.getErr(ctxPtr)) || null;
    if (err) {
      return;
    }

    const localRunStats: RunStats = {
      runtimeMs: performance.now() - startTime,
      renderedMeshCount: 0,
      renderedPathCount: 0,
      totalVtxCount: 0,
      totalFaceCount: 0,
    };

    localRunStats.renderedMeshCount = await repl.getRenderedMeshCount(ctxPtr);
    const newRenderedMeshes = [];
    for (let i = 0; i < localRunStats.renderedMeshCount; i++) {
      const { verts, indices, normals } = await repl.getRenderedMesh(ctxPtr, i);

      localRunStats.totalVtxCount += verts.length / 3;
      localRunStats.totalFaceCount += indices.length / 3;

      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute('position', new THREE.BufferAttribute(verts, 3));
      geometry.setIndex(new THREE.BufferAttribute(indices, 1));
      if (normals) {
        geometry.setAttribute('normal', new THREE.BufferAttribute(normals, 3));
      }

      const mesh = new THREE.Mesh(geometry, activeMat);
      mesh.castShadow = true;
      mesh.receiveShadow = true;
      viz.scene.add(mesh);
      newRenderedMeshes.push(mesh);
    }

    localRunStats.renderedPathCount = await repl.getRenderedPathCount(ctxPtr);
    for (let i = 0; i < localRunStats.renderedPathCount; i++) {
      const pathVerts: Float32Array = await repl.getRenderedPathVerts(ctxPtr, i);
      localRunStats.totalVtxCount += pathVerts.length / 3;
      localRunStats.totalFaceCount += pathVerts.length / 3 - 1;
      const pathGeometry = new THREE.BufferGeometry();
      pathGeometry.setAttribute('position', new THREE.BufferAttribute(pathVerts, 3));
      const pathMaterial = lineMat;
      const pathMesh = new THREE.Line(pathGeometry, pathMaterial);
      pathMesh.castShadow = false;
      pathMesh.receiveShadow = false;
      viz.scene.add(pathMesh);
      newRenderedMeshes.push(pathMesh);
    }

    renderedMeshes = newRenderedMeshes;
    runStats = localRunStats;
  };

  const beforeUnloadHandler = () => {
    if (editorView) {
      localStorage.lastGeoscriptPlaygroundCode = editorView.state.doc.toString();
    }
  };

  onMount(() => {
    const customKeymap: readonly KeyBinding[] = [
      {
        key: 'Ctrl-Enter',
        run: () => {
          run();
          return true;
        },
      },
      {
        key: 'Ctrl-.',
        run: () => {
          centerView();
          return true;
        },
      },
      {
        key: 'Ctrl-s',
        run: () => {
          if (editorView) {
            localStorage.lastGeoscriptPlaygroundCode = editorView.state.doc.toString();
          }
          return true;
        },
      },
    ];

    const editor = buildEditor({
      container: codemirrorContainer!,
      customKeymap,
      initialCode: localStorage.lastGeoscriptPlaygroundCode || 'box(8) + (box(8) + vec3(4, 4, -4)) | render',
    });
    editorView = editor.editorView;

    setReplCtx({ centerView, toggleWireframe });

    window.addEventListener('beforeunload', beforeUnloadHandler);

    return () => {
      if (editorView) {
        localStorage.lastGeoscriptPlaygroundCode = editorView.state.doc.toString();
        editorView.destroy();
      }
      for (const mesh of renderedMeshes) {
        viz.scene.remove(mesh);
        mesh.geometry.dispose();
      }

      window.removeEventListener('beforeunload', beforeUnloadHandler);
    };
  });
</script>

<div class="root">
  <div
    bind:this={codemirrorContainer}
    class="codemirror-wrapper"
    style="display: flex; flex: 1; background: #222;"
  ></div>
  <div class="controls">
    <button onclick={run}>run</button>
    {#if err}
      <div class="error">{err}</div>
    {/if}
    {#if runStats}
      <div class="run-stats">
        <span style="color: #12cc12">Program ran successfully</span>
        <ul>
          <li>Runtime: {runStats.runtimeMs.toFixed(2)} ms</li>
          {#if runStats.renderedMeshCount > 0 || runStats.renderedPathCount === 0}
            <li>Rendered Meshes: {IntFormatter.format(runStats.renderedMeshCount)}</li>
          {/if}
          {#if runStats.renderedPathCount > 0}
            <li>Rendered Paths: {IntFormatter.format(runStats.renderedPathCount)}</li>
          {/if}
          <li>Total Vertices: {IntFormatter.format(runStats.totalVtxCount)}</li>
          <li>Total Faces: {IntFormatter.format(runStats.totalFaceCount)}</li>
        </ul>
      </div>
    {/if}
  </div>
</div>

<style lang="css">
  @import url('https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600;700&display=swap');

  .root {
    height: calc(max(250px, 25vh));
    width: 100%;
    position: absolute;
    bottom: 0;
    display: flex;
    flex-direction: row;
    color: #efefef;
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    font-size: 15px;
  }

  .codemirror-wrapper {
    display: flex;
    flex: 1;
    width: 100%;
    min-width: 0;
    overflow-x: auto;
    background: #222;
  }

  :global(.codemirror-wrapper > div) {
    display: flex;
    flex: 1;
    width: 100%;
    min-width: 0;
    box-sizing: border-box;
  }

  .controls {
    display: flex;
    flex-direction: column;
    min-width: 200px;
    flex: 0.4;
    padding: 8px;
    border-top: 1px solid #444;
  }

  .error {
    color: red;
    background: #222;
    padding: 16px 8px;
    margin-top: 8px;
    overflow-y: auto;
    overflow-x: hidden;
    max-height: 200px;
    white-space: pre-wrap;
    overflow-wrap: break-word;
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
  }

  .run-stats {
    margin-top: 8px;
    padding: 8px;
  }

  button {
    background: #333;
    color: #f0f0f0;
    border: 1px solid #888;
    border-radius: 0;
    padding: 8px 12px;
    cursor: pointer;
    font-size: 14px;
  }
</style>
