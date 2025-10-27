<script lang="ts">
  import * as THREE from 'three';
  import { onDestroy, onMount } from 'svelte';
  import type { EditorView, KeyBinding } from '@codemirror/view';
  import type * as Comlink from 'comlink';
  import { resolve } from '$app/paths';

  import type { Viz } from 'src/viz';
  import type { GeoscriptWorkerMethods } from 'src/geoscript/geoscriptWorker.worker';
  import { buildEditor } from '../../../geoscript/editor';
  import type { GeoscriptPlaygroundUserData } from './geoscriptPlayground.svelte';
  import SaveControls from './SaveControls.svelte';
  import { goto } from '$app/navigation';
  import { type ReplCtx, type RunStats } from './types';
  import ReplOutput from './ReplOutput.svelte';
  import ReplControls from './ReplControls.svelte';
  import ExportModal from './ExportModal.svelte';
  import { runGeoscript } from 'src/geoscript/runner/runner';
  import {
    HiddenMat,
    NormalMat,
    WireframeMat,
    type MaterialDef,
    type MaterialDefinitions,
  } from 'src/geoscript/materials';
  import MaterialEditor from './materialEditor/MaterialEditor.svelte';
  import { type CompositionVersionMetadata } from 'src/geoscript/geotoyAPIClient';
  import {
    clearSavedState,
    getIsDirty,
    getServerState,
    getView,
    loadState,
    saveState,
    setLastRunWasSuccessful,
  } from './persistence';
  import { getIsUVUnwrapLoaded } from 'src/viz/wasm/uv_unwrap/uvUnwrap';
  import ReadOnlyCompositionDetails from './ReadOnlyCompositionDetails.svelte';
  import { populateScene } from 'src/geoscript/runner/geoscriptRunner';
  import type { MatEntry, RenderedObject } from 'src/geoscript/runner/types';
  import {
    buildCustomMaterials,
    fetchAndSetTextures,
    getReferencedTextureIDs,
  } from './materialLoading.svelte';
  import { centerView, snapView, orbit } from './cameraControls';
  import { buildLightHelpers, toggleAxisHelpers, toggleLightHelpers } from './gizmos';

  let {
    viz,
    geoscriptWorker: repl,
    setReplCtx,
    userData,
    onHeightChange,
  }: {
    viz: Viz;
    geoscriptWorker: Comlink.Remote<GeoscriptWorkerMethods>;
    setReplCtx: (ctx: ReplCtx) => void;
    userData?: GeoscriptPlaygroundUserData;
    onHeightChange: (height: number, isCollapsed: boolean) => void;
  } = $props();

  const {
    code: initialCode,
    materials: initialMatDefs,
    lastRunWasSuccessful,
    view: initialView,
    preludeEjected: initialPreludeEjected,
  } = loadState(userData);

  let ctxPtr = $state<number | null>(null);

  let isDirty = $state(getIsDirty(userData));

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
  let lastCode = initialCode;

  onMount(() => {
    onHeightChange(height, isEditorCollapsed);

    repl.init().then(ptr => {
      ctxPtr = ptr;
    });
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

  let err: string | null = $state(null);
  let isRunning: boolean = $state(false);
  let runStats: RunStats | null = $state(null);
  let renderedObjects: RenderedObject[] = $state([]);
  let lightHelpers: THREE.Object3D[] = $state([]);

  let codemirrorContainer = $state<HTMLDivElement | null>(null);
  let editorView = $state<EditorView | null>(null);

  let didFirstRun = $state(false);
  $effect(() => {
    if (ctxPtr === null) {
      return;
    }

    if (didFirstRun) {
      return;
    }
    didFirstRun = true;

    // if the user closed the tab while the last run was in progress, avoid eagerly running it again in
    // case there was an infinite loop or something
    if (lastRunWasSuccessful) {
      run(initialCode);
    }
  });

  const beforeUnloadHandler = () => {
    if (editorView) {
      saveState(
        {
          code: editorView.state.doc.toString(),
          materials: materialDefinitions,
          view: getView(viz),
          preludeEjected,
        },
        userData
      );
    }
  };

  const setupEditor = () => {
    if (!codemirrorContainer) {
      if (editorView) {
        beforeUnloadHandler();
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
          centerView(viz, renderedObjects);
          return true;
        },
      },
      {
        key: 'Ctrl-s',
        run: () => {
          if (editorView) {
            saveState(
              {
                code: editorView.state.doc.toString(),
                materials: materialDefinitions,
                view: getView(viz),
                preludeEjected,
              },
              userData
            );
          }
          return true;
        },
      },
    ];

    const editor = buildEditor({
      container: codemirrorContainer,
      customKeymap,
      initialCode: lastCode,
      onDocChange: () => {
        isDirty = true;
      },
    });
    editorView = editor.editorView;
  };

  onDestroy(() => {
    if (editorView) {
      beforeUnloadHandler();
      editorView.destroy();
    }
  });

  $effect(setupEditor);

  let materialOverride = $state<'wireframe' | 'normal' | null>(null);

  const toggleWireframe = () => {
    materialOverride = materialOverride === 'wireframe' ? null : 'wireframe';
    for (const obj of renderedObjects) {
      if (obj instanceof THREE.Mesh) {
        const mat =
          materialOverride === 'wireframe'
            ? WireframeMat
            : (customMaterialsByName[obj.userData.materialName]?.resolved ?? HiddenMat);
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
            ? NormalMat
            : (customMaterialsByName[obj.userData.materialName]?.resolved ?? HiddenMat);
        obj.material = mat;
      }
    }
  };

  let materialEditorOpen = $state(false);
  let materialDefinitions = $state<MaterialDefinitions>(initialMatDefs);
  let preludeEjected = $state(initialPreludeEjected);

  onMount(() => {
    const referencedTextureIDs = getReferencedTextureIDs(materialDefinitions.materials);
    if (referencedTextureIDs.length > 0) {
      fetchAndSetTextures(loader, referencedTextureIDs);
    }
  });

  $effect(() => {
    if (ctxPtr === null) {
      return;
    }

    // TODO: only do this if these have changed
    repl.setMaterials(
      ctxPtr,
      materialDefinitions.defaultMaterialID,
      Object.values(materialDefinitions.materials).map(mat => mat.name)
    );
  });

  const loader = new THREE.ImageBitmapLoader();
  let customMaterials: Record<string, MatEntry> = $derived.by(() =>
    // `$state.snapshot` seems required here in order to trigger this derived to actually run when things change
    buildCustomMaterials(
      loader,
      $state.snapshot(materialDefinitions.materials) as Record<string, MaterialDef>,
      viz
    )
  );

  // avoid a ton of before render callbacks from being stuck around, which also prevents
  // old materials from being garbage collected
  $effect(() => {
    const customMatVals = Object.values(customMaterials);
    return () => {
      for (const matEntry of customMatVals) {
        if (matEntry.beforeRenderCb) {
          viz.unregisterBeforeRenderCb(matEntry.beforeRenderCb);
        }
      }
    };
  });

  let didInitMats = false;
  $effect(() => {
    // force dependency
    if ($state.snapshot(materialDefinitions)) {
      if (!didInitMats) {
        didInitMats = true;
      } else {
        isDirty = true;
      }
    } else {
      throw new Error('unreachable');
    }
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
    if (isRunning || ctxPtr === null) {
      return;
    }

    const finalCode = (() => {
      if (typeof code === 'string') {
        return code;
      }
      if (editorView) {
        return editorView.state.doc.toString();
      }
      return lastCode;
    })();

    beforeUnloadHandler();

    isRunning = true;
    err = null;

    for (const obj of renderedObjects) {
      viz.scene.remove(obj);
      if (obj instanceof THREE.Mesh || obj instanceof THREE.Line) {
        obj.geometry.dispose();
      }
    }
    renderedObjects = [];
    runStats = null;

    const matsByName: Record<string, { def: MaterialDef; mat: MatEntry }> = {};
    for (const [id, def] of Object.entries(materialDefinitions.materials)) {
      matsByName[def.name] = { def, mat: customMaterials[id] };
    }

    setLastRunWasSuccessful(false, userData);
    const result = await runGeoscript({
      code: finalCode,
      ctxPtr,
      repl,
      materials: matsByName,
      includePrelude: !preludeEjected,
      materialOverride,
      renderMode: userData?.renderMode ?? false,
    });

    if (result.error) {
      err = result.error;
      isRunning = false;
      return;
    }

    setLastRunWasSuccessful(true, userData);
    runStats = result.stats;
    renderedObjects = populateScene(viz.scene, result);

    for (const helper of lightHelpers) {
      viz.scene.remove(helper);
    }
    if (localStorage['geoscript-light-helpers'] === 'true') {
      lightHelpers = buildLightHelpers(viz, renderedObjects);
    } else {
      lightHelpers = [];
    }

    isRunning = false;
  };

  const rerun = async (onlyIfUVUnwrapperNotLoaded: boolean): Promise<void> => {
    if (onlyIfUVUnwrapperNotLoaded && getIsUVUnwrapLoaded()) {
      return;
    }
    return run(editorView?.state.doc.toString() ?? lastCode);
  };

  const toggleEditorCollapsed = () => {
    if (editorView) {
      lastCode = editorView.state.doc.toString();
      saveState(
        {
          code: lastCode,
          materials: materialDefinitions,
          view: getView(viz),
          preludeEjected,
        },
        userData
      );
    }
    isEditorCollapsed = !isEditorCollapsed;
    onHeightChange(height, isEditorCollapsed);
  };

  const ejectPrelude = async (editorView: EditorView) => {
    const prelude = await repl.getPrelude();
    editorView.dispatch({
      changes: { from: 0, insert: prelude + '\n//-- end prelude\n\n' },
    });
  };

  const togglePreludeEjected = async () => {
    if (!editorView) {
      return;
    }

    if (!preludeEjected) {
      await ejectPrelude(editorView);
    }
    preludeEjected = !preludeEjected;

    run(editorView.state.doc.toString());
  };

  let exportDialog = $state<HTMLDialogElement | null>(null);
  const onExport = () => {
    exportDialog?.showModal();
  };

  const setView = async (view: CompositionVersionMetadata['view']) => {
    while (!viz.orbitControls) {
      await new Promise(resolve => setTimeout(resolve, 10));
    }

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
  };

  const clearLocalChanges = () => {
    if (isDirty && !confirm('Really clear local changes?')) {
      return;
    }

    clearSavedState(userData);

    const serverState = getServerState(userData);

    if (editorView) {
      editorView.dispatch({
        changes: { from: 0, to: editorView.state.doc.length, insert: serverState.code },
      });
    }
    didInitMats = false;

    materialDefinitions = serverState.materials;
    const referencedTextureIDs = getReferencedTextureIDs(materialDefinitions.materials);
    fetchAndSetTextures(loader, referencedTextureIDs).then(() => {
      didInitMats = false;
      materialDefinitions = { ...serverState.materials };
    });

    setView(serverState.view);
    preludeEjected = serverState.preludeEjected;

    run(serverState.code);

    saveState(
      {
        code: serverState.code,
        materials: serverState.materials,
        view: serverState.view,
        preludeEjected: serverState.preludeEjected,
      },
      userData
    );

    isDirty = false;
  };

  const wrappedToggleAxesHelpers = () => toggleAxisHelpers(viz);
  const wrappedToggleLightHelpers = () => {
    lightHelpers = toggleLightHelpers(viz, renderedObjects, lightHelpers);
  };

  onMount(() => {
    if (userData?.renderMode) {
      const stats = document.getElementById('viz-stats');
      if (stats) {
        stats.style.display = 'none';
      }
    }

    setTimeout(() => setView(initialView));

    setReplCtx({
      centerView: () => centerView(viz, renderedObjects),
      toggleWireframe,
      toggleNormalMat,
      toggleLightHelpers: wrappedToggleLightHelpers,
      toggleAxesHelper: wrappedToggleAxesHelpers,
      getLastRunOutcome: () => lastRunOutcome,
      getAreAllMaterialsLoaded: () => Object.values(customMaterials).every(mat => mat.resolved),
      run,
      snapView: axis => snapView(viz, axis),
      orbit: (axis, angle) => orbit(viz, axis, angle),
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

    if (isDirty) {
      if (!confirm('You have unsaved changes. Really leave page?')) {
        return;
      }
    }

    goto(resolve('/geotoy'));
  };
</script>

<svelte:window bind:innerWidth />

<ExportModal bind:dialog={exportDialog} {renderedObjects} />
<MaterialEditor
  bind:isOpen={materialEditorOpen}
  bind:materials={materialDefinitions}
  {rerun}
  {repl}
  {ctxPtr}
  me={userData?.me}
/>

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
      {clearLocalChanges}
      toggleAxisHelpers={wrappedToggleAxesHelpers}
      toggleLightHelpers={wrappedToggleLightHelpers}
      {isDirty}
      {preludeEjected}
      {togglePreludeEjected}
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
            {clearLocalChanges}
            toggleAxisHelpers={wrappedToggleAxesHelpers}
            toggleLightHelpers={wrappedToggleLightHelpers}
            {isDirty}
            {preludeEjected}
            {togglePreludeEjected}
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
            {preludeEjected}
            onSave={() => {
              isDirty = false;
              saveState(
                {
                  code: editorView?.state.doc.toString() || '',
                  materials: materialDefinitions,
                  view: getView(viz),
                  preludeEjected,
                },
                userData
              );
            }}
          />
        {:else if userData?.initialComposition}
          <ReadOnlyCompositionDetails
            comp={userData.initialComposition.comp}
            version={userData.initialComposition.version}
          />
        {:else if !userData?.me}
          <div class="not-logged-in" style="border-top: 1px solid #333">
            <span style="color: #ddd">you must be logged in to save/share compositions</span>
            <div>
              <a href={resolve('/geotoy/login')}>log in</a>
              /
              <a href={resolve('/geotoy/register')}>register</a>
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
    max-width: 100vw;
    overflow-x: hidden;
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
    z-index: 2;
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

    .output {
      overflow-x: hidden;
    }
  }
</style>
