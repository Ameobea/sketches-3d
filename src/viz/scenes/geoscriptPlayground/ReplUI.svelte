<script lang="ts">
  import type { Viz } from 'src/viz';
  import * as THREE from 'three';
  import { onDestroy, onMount } from 'svelte';
  import { EditorView, type KeyBinding } from '@codemirror/view';
  import type * as Comlink from 'comlink';

  import type { GeoscriptWorkerMethods } from 'src/geoscript/geoscriptWorker.worker';
  import { buildEditor } from '../../../geoscript/editor';
  import { buildAndAddLight } from './lights';
  import type { GeoscriptPlaygroundUserData } from './geoscriptPlayground.svelte';
  import SaveControls from './SaveControls.svelte';
  import { goto } from '$app/navigation';
  import { DefaultCameraPos, DefaultCameraTarget, type ReplCtx, type RunStats } from './types';
  import ReplOutput from './ReplOutput.svelte';
  import ReplControls from './ReplControls.svelte';
  import ExportModal from './ExportModal.svelte';
  import {
    buildDefaultMaterialDefinitions,
    buildMaterial,
    type MaterialDefinitions,
  } from 'src/geoscript/materials';
  import MaterialEditor from './materialEditor/MaterialEditor.svelte';
  import { getMultipleTextures, type TextureID } from 'src/geoscript/geotoyAPIClient';
  import { Textures } from './materialEditor/state.svelte';

  let {
    viz,
    geoscriptWorker: repl,
    ctxPtr,
    setReplCtx,
    userData,
    onHeightChange,
  }: {
    viz: Viz;
    geoscriptWorker: Comlink.Remote<GeoscriptWorkerMethods>;
    ctxPtr: number;
    setReplCtx: (ctx: ReplCtx) => void;
    userData?: GeoscriptPlaygroundUserData;
    onHeightChange: (height: number, isCollapsed: boolean) => void;
  } = $props();

  let innerWidth = $state(window.innerWidth);
  let isEditorCollapsed = $state(
    (() => {
      const raw = localStorage.getItem('geoscriptEditorCollapsed');
      return typeof raw === 'string' ? raw === 'true' : innerWidth < 768;
    })()
  );
  $effect(() => {
    localStorage.setItem('geoscriptEditorCollapsed', isEditorCollapsed ? 'true' : 'false');
  });
  $effect(() => {
    if (innerWidth >= 768 && isEditorCollapsed) {
      isEditorCollapsed = false;
      onHeightChange(height, isEditorCollapsed);
    }
  });

  let height = $state(
    Number(localStorage.getItem('geoscript-repl-height')) || Math.max(250, 0.25 * window.innerHeight)
  );

  onMount(() => {
    onHeightChange(height, isEditorCollapsed);
  });

  const handleMousedown = (e: MouseEvent) => {
    e.preventDefault();

    const handleMousemove = (e: MouseEvent) => {
      const newHeight = Math.min(window.innerHeight * 0.9, Math.max(100, window.innerHeight - e.clientY));
      height = newHeight;
      onHeightChange(height, isEditorCollapsed);
    };

    const handleMouseup = () => {
      window.removeEventListener('mousemove', handleMousemove);
      window.removeEventListener('mouseup', handleMouseup);
    };

    window.addEventListener('mousemove', handleMousemove);
    window.addEventListener('mouseup', handleMouseup);
  };

  let initComposition = $derived(userData?.initialComposition);

  let localStorageKeySuffix = $derived(
    (() => {
      if (!initComposition) {
        return '';
      }

      return `-${initComposition.comp.id}-${initComposition.version.id}`;
    })()
  );

  let err: string | null = $state(null);
  let isRunning: boolean = $state(false);
  let runStats: RunStats | null = $state(null);
  const includePrelude = true;
  let renderedObjects: (
    | THREE.Mesh<THREE.BufferGeometry, THREE.Material>
    | THREE.Line<THREE.BufferGeometry, THREE.Material>
    | THREE.Light
  )[] = $state([]);

  let codemirrorContainer = $state<HTMLDivElement | null>(null);
  let editorView = $state<EditorView | null>(null);

  let lastSavedSrc = $state<string | null>(null);
  const DefaultCode = 'box(8) | (box(8) + vec3(4, 4, -4)) | render';
  const initialCode = $derived<string>(
    localStorage[`lastGeoscriptPlaygroundCode${localStorageKeySuffix}`] ||
      (typeof lastSavedSrc === 'string' ? lastSavedSrc : initComposition?.version.source_code) ||
      DefaultCode
  );

  let didFirstRun = $state(false);
  $effect(() => {
    if (didFirstRun) {
      return;
    }
    didFirstRun = true;

    // if the user closed the tab while the last run was in progress, avoid eagerly running it again in
    // case there was an infinite loop or something
    if (localStorage[`lastGeoscriptRunCompleted${localStorageKeySuffix}`] !== 'false') {
      run(initialCode);
    }
  });

  const beforeUnloadHandler = () => {
    localStorage[`lastGeoscriptPlaygroundCode${localStorageKeySuffix}`] = editorView
      ? editorView.state.doc.toString()
      : lastSrc;
    localStorage[`geoscriptPlaygroundMaterials${localStorageKeySuffix}`] =
      JSON.stringify(materialDefinitions);
  };

  // svelte-ignore state_referenced_locally
  let lastSrc = $state(initialCode);
  const setupEditor = () => {
    if (!codemirrorContainer) {
      if (editorView) {
        beforeUnloadHandler();
        lastSrc = editorView.state.doc.toString();
        localStorage[`lastGeoscriptPlaygroundCode${localStorageKeySuffix}`] = editorView.state.doc.toString();
        editorView.destroy();
        editorView = null;
      }
      return;
    }

    if (editorView) {
      return;
    }

    const customKeymap: readonly KeyBinding[] = [
      {
        key: 'Ctrl-Enter',
        run: () => {
          if (!editorView) {
            return true;
          }
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
            localStorage[`lastGeoscriptPlaygroundCode${localStorageKeySuffix}`] =
              editorView.state.doc.toString();
          }
          return true;
        },
      },
    ];

    const editor = buildEditor({
      container: codemirrorContainer,
      customKeymap,
      initialCode,
    });
    editorView = editor.editorView;
  };

  onDestroy(() => setupEditor());

  $effect(setupEditor);

  let materialOverride = $state<'wireframe' | 'normal' | null>(null);
  const lineMat = new THREE.LineBasicMaterial({
    color: 0x00ff00,
    linewidth: 2,
  });
  const hiddenMat = new THREE.MeshBasicMaterial({ transparent: true, opacity: 0 });
  const fallbackMat = new THREE.MeshBasicMaterial({
    color: 0x888888,
  });
  const wireframeMat = new THREE.MeshBasicMaterial({
    color: 0xdf00df,
    wireframe: true,
  });
  const normalMat = new THREE.MeshNormalMaterial();

  const computeCompositeBoundingBox = (
    objects: (
      | THREE.Mesh<THREE.BufferGeometry, THREE.Material>
      | THREE.Line<THREE.BufferGeometry, THREE.Material>
      | THREE.Light
    )[]
  ): THREE.Box3 => {
    const box = new THREE.Box3();
    for (const obj of objects) {
      if (!(obj instanceof THREE.Mesh || obj instanceof THREE.Line)) {
        continue;
      }

      obj.geometry.computeBoundingBox();
      const meshBox = obj.geometry.boundingBox;
      if (meshBox) box.union(meshBox.applyMatrix4(obj.matrixWorld));
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

    if (!renderedObjects.length) {
      viz.camera.position.copy(DefaultCameraPos);
      viz.orbitControls!.target.copy(DefaultCameraTarget);
      viz.camera.lookAt(DefaultCameraTarget);
      viz.orbitControls!.update();
      return;
    }

    const compositeBbox = computeCompositeBoundingBox(renderedObjects);
    const boundingSphere = new THREE.Sphere();
    compositeBbox.getBoundingSphere(boundingSphere);
    const center = boundingSphere.center;
    const radius = boundingSphere.radius;

    // try to keep the same look direction
    const lookDir = new THREE.Vector3();
    lookDir.copy(viz.camera.position).sub(viz.orbitControls!.target);

    if (lookDir.lengthSq() === 0) {
      // If camera and target are at the same spot, use a default direction
      lookDir.set(1, 1, 1);
    }
    lookDir.normalize();

    const camera = viz.camera as THREE.PerspectiveCamera;
    let distance;

    if (!camera.isPerspectiveCamera) {
      console.warn('centerView only works with PerspectiveCamera, falling back to old method');
      const size = new THREE.Vector3();
      compositeBbox.getSize(size);
      const maxDim = Math.max(size.x, size.y, size.z);
      distance = maxDim * 1.2 + 1;
    } else {
      const vfov = THREE.MathUtils.degToRad(camera.fov);
      const hfov = 2 * Math.atan(Math.tan(vfov / 2) * camera.aspect);
      const fov = Math.min(vfov, hfov);

      // Compute distance to fit bounding sphere in view.
      distance = radius / Math.sin(fov / 2);

      // Add a little padding so the object is not touching the screen edge
      distance *= 1.1;
    }

    viz.camera.position.copy(center).add(lookDir.multiplyScalar(distance));
    viz.orbitControls!.target.copy(center);
    viz.camera.lookAt(center);
    viz.orbitControls!.update();
  };

  const toggleWireframe = () => {
    materialOverride = materialOverride === 'wireframe' ? null : 'wireframe';
    for (const obj of renderedObjects) {
      if (obj instanceof THREE.Mesh) {
        const mat =
          materialOverride === 'wireframe'
            ? wireframeMat
            : (customMaterialsByName[obj.userData.materialName]?.resolved ?? hiddenMat);
        obj.material = mat;
      }
    }
  };

  const toggleNormalMat = () => {
    materialOverride = materialOverride === 'normal' ? null : 'normal';
    for (const obj of renderedObjects) {
      if (obj instanceof THREE.Mesh) {
        const mat =
          materialOverride === 'normal'
            ? normalMat
            : (customMaterialsByName[obj.userData.materialName]?.resolved ?? hiddenMat);
        obj.material = mat;
      }
    }
  };

  let materialEditorOpen = $state(false);
  const initialMatDefs = ((): MaterialDefinitions => {
    if (userData?.initialComposition?.version?.metadata?.materials) {
      return userData.initialComposition.version.metadata.materials;
    }

    const matDefs = localStorage[`geoscriptPlaygroundMaterials${localStorageKeySuffix}`];
    if (typeof matDefs === 'string') {
      try {
        return JSON.parse(matDefs);
      } catch (err) {
        console.warn('Error parsing saved material definitions:', err);
      }
    }

    return buildDefaultMaterialDefinitions();
  })();
  let materialDefinitions = $state<MaterialDefinitions>(initialMatDefs);

  onMount(() => {
    const referencedTextureIDs: TextureID[] = [];
    for (const mat of Object.values(materialDefinitions.materials)) {
      if (mat.type === 'basic') {
        continue;
      }

      if (mat.map) {
        referencedTextureIDs.push(mat.map);
      }
      if (mat.normalMap) {
        referencedTextureIDs.push(mat.normalMap);
      }
      if (mat.roughnessMap) {
        referencedTextureIDs.push(mat.roughnessMap);
      }
    }

    if (referencedTextureIDs.length > 0) {
      const adminToken = new URLSearchParams(window.location.search).get('admin_token') ?? undefined;
      getMultipleTextures(referencedTextureIDs, undefined, adminToken).then(textures => {
        const allTextures = { ...Textures.textures };
        for (const texture of textures) {
          allTextures[texture.id] = texture;
        }
        Textures.textures = allTextures;
      });
    }
  });

  $effect(() => {
    // TODO: only do this if these have changed
    repl.setMaterials(
      ctxPtr,
      materialDefinitions.defaultMaterialID,
      Object.values(materialDefinitions.materials).map(mat => mat.name)
    );
  });

  const loader = new THREE.ImageBitmapLoader();
  let customMaterials: Record<string, { promise: Promise<THREE.Material>; resolved: THREE.Material | null }> =
    $derived.by(() => {
      const builtMats: Record<string, { promise: Promise<THREE.Material>; resolved: THREE.Material | null }> =
        {};
      // TODO: needs hashing to avoid re-building materials that haven't changed
      // `$state.snapshot` seems required here in order to trigger this derived to actually run when things change
      for (const [id, def] of Object.entries($state.snapshot(materialDefinitions.materials))) {
        const matP = buildMaterial(loader, def, id);
        const entry: { promise: Promise<THREE.Material>; resolved: THREE.Material | null } = {
          promise: matP,
          resolved: null,
        };
        matP.then(mat => {
          entry.resolved = mat;
        });
        builtMats[id] = entry;
      }
      return builtMats;
    });
  let customMaterialsByName: Record<
    string,
    { promise: Promise<THREE.Material>; resolved: THREE.Material | null }
  > = $derived.by(() => {
    const matsByName: Record<string, { promise: Promise<THREE.Material>; resolved: THREE.Material | null }> =
      {};
    for (const [id, def] of Object.entries($state.snapshot(materialDefinitions.materials))) {
      matsByName[def.name] = customMaterials[id];
    }
    return matsByName;
  });

  $effect(() => {
    for (const obj of renderedObjects) {
      if (!(obj instanceof THREE.Mesh)) {
        continue;
      }

      for (const [id, matEntry] of Object.entries(customMaterials)) {
        if (obj.material.name === id) {
          if (matEntry.resolved) {
            obj.material = matEntry.resolved;
          } else {
            matEntry.promise.then(mat => {
              obj.material = mat;
            });
          }
          break;
        }
      }
    }
  });

  let lastRunOutcome = $derived(
    (() => {
      if (err) {
        return { type: 'err' as const, err };
      }
      if (runStats) {
        return { type: 'ok' as const, stats: runStats };
      }
      return null;
    })()
  );

  const run = async (code?: string) => {
    if (isRunning) {
      return;
    }

    if (typeof code !== 'string') {
      if (!editorView) {
        return;
      }
      code = editorView.state.doc.toString();
    }

    beforeUnloadHandler();

    isRunning = true;
    for (const obj of renderedObjects) {
      viz.scene.remove(obj);
      if (obj instanceof THREE.Mesh || obj instanceof THREE.Line) {
        obj.geometry.dispose();
      }
    }
    renderedObjects = [];

    await repl.reset(ctxPtr);
    runStats = null;
    const startTime = performance.now();
    localStorage[`lastGeoscriptRunCompleted${localStorageKeySuffix}`] = 'false';
    try {
      await repl.eval(ctxPtr, code, includePrelude);
    } catch (err) {
      console.error('Error evaluating code:', err);
      // TODO: this set isn't working for some reason
      err = `Error evaluating code: ${err}`;
      isRunning = false;
      return;
    } finally {
      localStorage[`lastGeoscriptRunCompleted${localStorageKeySuffix}`] = 'true';
    }
    err = (await repl.getErr(ctxPtr)) || null;
    if (err) {
      isRunning = false;
      return;
    }

    const localRunStats: RunStats = {
      runtimeMs: performance.now() - startTime,
      renderedMeshCount: 0,
      renderedPathCount: 0,
      renderedLightCount: 0,
      totalVtxCount: 0,
      totalFaceCount: 0,
    };

    localRunStats.renderedMeshCount = await repl.getRenderedMeshCount(ctxPtr);
    const newRenderedMeshes: (
      | THREE.Mesh<THREE.BufferGeometry, THREE.Material>
      | THREE.Line<THREE.BufferGeometry, THREE.Material>
      | THREE.Light
    )[] = [];
    for (let i = 0; i < localRunStats.renderedMeshCount; i += 1) {
      const {
        transform,
        verts,
        indices,
        normals,
        material: materialName,
      } = await repl.getRenderedMesh(ctxPtr, i);

      localRunStats.totalVtxCount += verts.length / 3;
      localRunStats.totalFaceCount += indices.length / 3;

      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute('position', new THREE.BufferAttribute(verts, 3));
      geometry.setIndex(new THREE.BufferAttribute(indices, 1));
      if (normals) {
        geometry.setAttribute('normal', new THREE.BufferAttribute(normals, 3));
      }

      const matEntry = (() => {
        if (!materialName) {
          return { resolved: fallbackMat, promise: null };
        }

        const matEntry = customMaterialsByName[materialName];
        if (!matEntry) {
          console.warn(`mesh referenced undefined material: "${materialName}"`);
          return { resolved: fallbackMat, promise: null };
        }

        return matEntry;
      })();

      const mesh: THREE.Mesh<THREE.BufferGeometry, THREE.Material> = new THREE.Mesh(
        geometry,
        matEntry.resolved ?? hiddenMat
      );
      mesh.userData.materialName = materialName;
      if (!matEntry.resolved && matEntry.promise) {
        matEntry.promise.then(mat => {
          mesh.material = mat;
        });
      }
      mesh.applyMatrix4(new THREE.Matrix4().fromArray(transform));
      mesh.castShadow = true;
      mesh.receiveShadow = true;
      viz.scene.add(mesh);
      newRenderedMeshes.push(mesh);
    }

    localRunStats.renderedPathCount = await repl.getRenderedPathCount(ctxPtr);
    for (let i = 0; i < localRunStats.renderedPathCount; i += 1) {
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

    localRunStats.renderedLightCount = await repl.getRenderedLightCount(ctxPtr);
    for (let i = 0; i < localRunStats.renderedLightCount; i += 1) {
      const light = await repl.getRenderedLight(ctxPtr, i);
      const builtLight = buildAndAddLight(viz, light, userData?.renderMode ?? false);
      newRenderedMeshes.push(builtLight);
    }

    renderedObjects = newRenderedMeshes;
    runStats = localRunStats;
    isRunning = false;
  };

  const toggleEditorCollapsed = () => {
    isEditorCollapsed = !isEditorCollapsed;
    onHeightChange(height, isEditorCollapsed);
  };

  let exportDialog = $state<HTMLDialogElement | null>(null);
  const onExport = () => {
    exportDialog?.showModal();
  };

  onMount(() => {
    if (userData?.renderMode) {
      const stats = document.getElementById('viz-stats');
      if (stats) {
        stats.style.display = 'none';
      }
    }

    const view = userData?.initialComposition?.version?.metadata?.view;
    if (view && viz && viz.camera && viz.orbitControls) {
      viz.camera.position.set(...view.cameraPosition);
      viz.orbitControls.target.set(...view.target);
      if ('fov' in viz.camera && view.fov !== undefined) {
        viz.camera.fov = view.fov;
        viz.camera.updateProjectionMatrix();
      }
      if ('zoom' in viz.camera && view.zoom !== undefined) {
        viz.camera.zoom = view.zoom;
        viz.camera.updateProjectionMatrix();
      }
      viz.camera.lookAt(viz.orbitControls.target);
      viz.orbitControls.update();
    }

    setReplCtx({
      centerView,
      toggleWireframe,
      toggleNormalMat,
      getLastRunOutcome: () => lastRunOutcome,
      getAreAllMaterialsLoaded: () => Object.values(customMaterials).every(mat => mat.resolved),
    });

    window.addEventListener('beforeunload', beforeUnloadHandler);

    return () => {
      for (const mesh of renderedObjects) {
        viz.scene.remove(mesh);
        if (mesh instanceof THREE.Mesh || mesh instanceof THREE.Line) {
          mesh.geometry.dispose();
        }
      }

      window.removeEventListener('beforeunload', beforeUnloadHandler);
    };
  });

  const goHome = () => {
    beforeUnloadHandler();

    const curCode = editorView?.state.doc.toString() || lastSrc;
    const dirty = curCode !== initialCode && curCode !== DefaultCode;
    if (dirty) {
      if (!confirm('You have unsaved changes. Really leave page?')) {
        return;
      }
    }

    goto('/geotoy');
  };
</script>

<svelte:window bind:innerWidth />

<ExportModal bind:dialog={exportDialog} {renderedObjects} />
<MaterialEditor bind:isOpen={materialEditorOpen} bind:materials={materialDefinitions} />

{#if isEditorCollapsed}
  <div
    class="root collapsed"
    style={`${userData?.renderMode ? 'visibility: hidden; height: 0;' : ''} height: 36px;`}
  >
    <ReplControls
      {isRunning}
      {isEditorCollapsed}
      {run}
      {toggleEditorCollapsed}
      {goHome}
      {err}
      {onExport}
      toggleMaterialEditorOpen={() => (materialEditorOpen = true)}
    />
  </div>
{:else}
  <div
    class="root"
    style={`${userData?.renderMode ? 'visibility: hidden; height: 0;' : ''} height: ${height}px;`}
  >
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div class="dragger" role="separator" aria-orientation="horizontal" onmousedown={handleMousedown}></div>
    <div class="editor-container">
      <div
        bind:this={codemirrorContainer}
        class="codemirror-wrapper"
        style="flex: 1; background: #222;"
      ></div>
      <div class="controls">
        <div class="output">
          <ReplControls
            {isRunning}
            {isEditorCollapsed}
            {run}
            {toggleEditorCollapsed}
            {goHome}
            {err}
            {onExport}
            toggleMaterialEditorOpen={() => {
              materialEditorOpen = !materialEditorOpen;
            }}
          />
          <ReplOutput {err} {runStats} />
        </div>
        {#if userData?.me && (!userData.initialComposition || userData.me.id === userData.initialComposition.comp.author_id)}
          <SaveControls
            comp={userData.initialComposition?.comp}
            getCurrentCode={() => editorView?.state.doc.toString() || ''}
            materials={materialDefinitions}
            {viz}
            onSave={(src: string) => {
              lastSavedSrc = src;
            }}
          />
        {:else if userData?.me}
          <div class="not-logged-in" style="border-top: 1px solid #333">
            <span style="color: #ddd">you must be logged in to save/share compositions</span>
            <div>
              <a href="/geotoy/login">log in</a>
              /
              <a href="/geotoy/register">register</a>
            </div>
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style lang="css">
  @import url('https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600;700&display=swap');

  .root {
    width: 100%;
    position: absolute;
    bottom: 0;
    display: flex;
    flex-direction: column;
    color: #efefef;
    font-family: 'IBM Plex Mono', 'Hack', 'Roboto Mono', 'Courier New', Courier, monospace;
    font-size: 15px;
  }

  .dragger {
    width: 100%;
    height: 5px;
    position: absolute;
    top: -2px;
    left: 0;
    cursor: ns-resize;
  }

  .editor-container {
    display: flex;
    flex-direction: row;
    flex: 1;
    min-height: 0;
  }

  .output {
    display: flex;
    flex-direction: column;
    flex: 1;
    padding: 8px;
    overflow-y: auto;
    min-height: 80px;
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

  :global(.cm-content) {
    padding-top: 0 !important;
  }

  .controls {
    display: flex;
    flex-direction: column;
    min-width: 200px;
    flex: 0.4;
    border-top: 1px solid #444;
    overflow-y: auto;
  }

  .not-logged-in {
    font-size: 13px;
    padding: 8px;
  }

  @media (max-width: 768px) {
    .editor-container {
      flex-direction: column;
    }

    .output {
      padding: 4px;
    }

    .codemirror-wrapper {
      flex: 1;
    }

    .controls {
      flex: 1;
      border-top: none;
      border-left: 1px solid #444;
    }

    .not-logged-in {
      font-size: 12px;
      padding: 4px;
    }
  }
</style>
